use super::{
    super::environment::{CompletionEnvironment, CompletionLocation},
    super::error::CompletionError,
    super::utils::{CompletionTemplate, dispatch_completion_query},
    blank_node_property::{context_join, sibling_properties},
};
use crate::server::{Server, lsp::CompletionList};
use futures::{channel::oneshot, lock::Mutex};
use ll_sparql_parser::{
    SyntaxToken,
    ast::{AstNode, BlankPropertyList},
};
use std::rc::Rc;

#[cfg(not(target_arch = "wasm32"))]
use tokio::task::spawn_local;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;

pub async fn completions(
    server_rc: Rc<Mutex<Server>>,
    environment: &CompletionEnvironment,
) -> Result<CompletionList, CompletionError> {
    let CompletionLocation::BlankNodeObject(prop_list) = &environment.location else {
        return Err(CompletionError::Resolve(format!(
            "unexpected completion location: {:?}",
            environment.location
        )));
    };
    let mut template_context = environment.template_context().await;
    // NOTE: the context talks about the enclosing subject, the local context about the
    //       blank node. The join is the triple that links the two.
    let join = context_join(prop_list);
    template_context.insert(
        "local_context",
        &local_context(prop_list, environment.anchor_token.as_ref(), join.is_some()),
    );
    if let Some(join) = join {
        let mut context = environment.context.clone().unwrap_or_default();
        // NOTE: the enclosing triple is the trigger node of the context computation and
        //       therefore not part of it. Its other properties would be lost otherwise.
        context.raw_inject = match sibling_properties(prop_list) {
            Some(siblings) => format!("{} . {}", siblings, join),
            None => join,
        };
        template_context.insert("context", &context);
    }

    // NOTE: The context insensitive query is dispatched in parallel, it is only used
    //       when the context sensitive one fails.
    let (sender, receiver) = oneshot::channel::<CompletionList>();
    let server_rc_1 = server_rc.clone();
    let template_context_1 = template_context.clone();
    let environment_1 = environment.clone();
    spawn_local(async move {
        match dispatch_completion_query(
            server_rc_1,
            &environment_1,
            template_context_1,
            CompletionTemplate::ObjectCompletionContextInsensitive,
            false,
        )
        .await
        {
            Ok(res) => {
                if let Err(_err) = sender.send(res) {
                    // NOTE: This happens if the context sensitive completion succeeds first.
                }
            }
            Err(err) => {
                tracing::error!("Context insensitive completion query failed:\n{:?}", err);
            }
        };
    });

    match dispatch_completion_query(
        server_rc,
        environment,
        template_context,
        CompletionTemplate::ObjectCompletionContextSensitive,
        false,
    )
    .await
    {
        Ok(res) => Ok(res),
        Err(err) => {
            tracing::error!("Context sensitive completion query failed:\n{:?}", err);
            receiver.await.map_err(|_e| err)
        }
    }
}

/// Build the local context for a blank node object completion:
/// the property list of the blank node, clipped at the anchor token.
///
/// The blank node is bound to `?qls_blank_node` when it is linked to the context by
/// a [`context_join`], otherwise it stays an anonymous `[]`.
fn local_context(
    prop_list: &BlankPropertyList,
    anchor_token: Option<&SyntaxToken>,
    joined: bool,
) -> Option<String> {
    // NOTE: the anchor token has to be inside the property list, otherwise there is no
    //       meaningful local context to build.
    if !prop_list
        .property_list()?
        .syntax()
        .text_range()
        .contains_range(anchor_token?.text_range())
    {
        return None;
    }
    let subject = if joined { "?qls_blank_node" } else { "[]" };
    let clipped_text = prop_list
        .property_list()?
        .text_until(anchor_token?.text_range().end());
    Some(format!("{} {} ?qls_entity", subject, clipped_text))
}

#[cfg(test)]
mod tests {
    use super::{context_join, local_context, sibling_properties};
    use indoc::indoc;
    use ll_sparql_parser::{
        SyntaxToken,
        ast::{AstNode, BlankPropertyList},
        parse_query,
    };

    fn blank_property_list(query: &str) -> BlankPropertyList {
        let (root, _) = parse_query(query);
        root.descendants()
            .find_map(BlankPropertyList::cast)
            .expect("query contains a blank node property list")
    }

    /// Innermost (last in document order) blank node property list of `query`.
    fn innermost_blank_property_list(query: &str) -> BlankPropertyList {
        let (root, _) = parse_query(query);
        root.descendants()
            .filter_map(BlankPropertyList::cast)
            .last()
            .expect("query contains a blank node property list")
    }

    /// Last token of `query` with the text `anchor_text`.
    fn anchor(query: &str, anchor_text: &str) -> SyntaxToken {
        let (root, _) = parse_query(query);
        root.descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.text() == anchor_text)
            .last()
            .unwrap_or_else(|| panic!("query contains the token {anchor_text}"))
    }

    #[test]
    fn context_is_clipped_at_the_anchor_token() {
        let query = "SELECT * { ?s ?p [ a :Foo ; :bar ?x ] }";
        assert_eq!(
            local_context(
                &blank_property_list(query),
                Some(&anchor(query, ":bar")),
                false
            )
            .as_deref(),
            Some("[] a :Foo ; :bar ?qls_entity")
        );
    }

    #[test]
    fn full_property_list_is_kept_when_the_anchor_is_the_last_token() {
        let query = "SELECT * { ?s ?p [ a :Foo ; :bar ] }";
        assert_eq!(
            local_context(
                &blank_property_list(query),
                Some(&anchor(query, ":bar")),
                false
            )
            .as_deref(),
            Some("[] a :Foo ; :bar ?qls_entity")
        );
    }

    #[test]
    fn anchor_outside_the_property_list_yields_no_context() {
        // NOTE: this used to panic (assert + unchecked slice) instead of falling
        //       back to no local context.
        let query = "SELECT * { ?s ?p [ a :Foo ] . ?x ?y ?z }";
        assert_eq!(
            local_context(
                &blank_property_list(query),
                Some(&anchor(query, "?y")),
                false
            ),
            None
        );
    }

    #[test]
    fn a_joined_blank_node_is_bound_to_a_variable() {
        // NOTE: with a join the blank node has to be named, otherwise the context and
        //       the local context would talk about two unrelated nodes.
        let query = "SELECT * { ?s :p [ :bar ] }";
        assert_eq!(
            local_context(
                &blank_property_list(query),
                Some(&anchor(query, ":bar")),
                true
            )
            .as_deref(),
            Some("?qls_blank_node :bar ?qls_entity")
        );
    }

    #[test]
    fn a_joined_blank_node_keeps_the_preceding_properties() {
        let query = "SELECT * { ?s :p [ a :Foo ; :bar ] }";
        assert_eq!(
            local_context(
                &blank_property_list(query),
                Some(&anchor(query, ":bar")),
                true
            )
            .as_deref(),
            Some("?qls_blank_node a :Foo ; :bar ?qls_entity")
        );
    }

    #[test]
    fn join_links_the_subject_to_the_blank_node() {
        let query = "SELECT * { ?s :p [ :bar ] }";
        assert_eq!(
            context_join(&blank_property_list(query)).as_deref(),
            Some("?s :p ?qls_blank_node")
        );
    }

    #[test]
    fn join_uses_the_verb_the_blank_node_hangs_off() {
        let query = "SELECT * { ?s :q <urn:o> ; :p [ :bar ] }";
        assert_eq!(
            context_join(&blank_property_list(query)).as_deref(),
            Some("?s :p ?qls_blank_node")
        );
    }

    #[test]
    fn join_ignores_sibling_objects() {
        let query = "SELECT * { ?s :p <urn:o> , [ :bar ] }";
        assert_eq!(
            context_join(&blank_property_list(query)).as_deref(),
            Some("?s :p ?qls_blank_node")
        );
    }

    #[test]
    fn join_of_a_subject_blank_node_is_none() {
        // NOTE: the blank node is the subject, so there is no incoming link to build.
        let query = "SELECT * { [ :bar ] }";
        assert_eq!(context_join(&blank_property_list(query)), None);
    }

    #[test]
    fn join_of_a_nested_blank_node_is_none() {
        // NOTE: the enclosing subject is itself a blank node, so there is nothing
        //       in the (outer) context to join against.
        let query = "SELECT * { ?s :p [ :q [ :bar ] ] }";
        assert_eq!(context_join(&innermost_blank_property_list(query)), None);
    }

    #[test]
    fn join_of_a_blank_node_inside_a_collection_is_none() {
        let query = "SELECT * { ?s :p ( <urn:a> [ :bar ] ) }";
        assert_eq!(context_join(&blank_property_list(query)), None);
    }

    #[test]
    fn siblings_of_the_enclosing_triple_are_kept() {
        let query = "SELECT * { ?s :a <urn:a> ; :p [ :bar ] }";
        assert_eq!(
            sibling_properties(&blank_property_list(query)).as_deref(),
            Some("?s :a <urn:a>")
        );
    }

    #[test]
    fn siblings_of_a_lone_property_are_none() {
        let query = "SELECT * { ?s :p [ :bar ] }";
        assert_eq!(sibling_properties(&blank_property_list(query)), None);
    }

    #[test]
    fn siblings_of_a_labeled_subject() {
        let query = indoc!(
            r#"PREFIX unio: <http://uni-freiburg.de/ontology/>
               PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
               SELECT * WHERE {
                 ?a rdfs:label "Informatik"@de ;
                    unio:teachingUnit [ unio:faculty ?faculty ] .
               }"#
        );
        assert_eq!(
            sibling_properties(&blank_property_list(query)).as_deref(),
            Some(r#"?a rdfs:label "Informatik"@de"#)
        );
        assert_eq!(
            context_join(&blank_property_list(query)).as_deref(),
            Some("?a unio:teachingUnit ?qls_blank_node")
        );
    }
}
