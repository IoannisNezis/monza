use std::rc::Rc;

use super::{
    super::environment::{CompletionEnvironment, CompletionLocation},
    super::error::CompletionError,
    super::utils::{CompletionTemplate, dispatch_completion_query},
};
use crate::server::{Server, lsp::CompletionList, message_handler::completion::utils::reduce_path};
use futures::{channel::oneshot, lock::Mutex};
use ll_sparql_parser::{
    SyntaxToken,
    ast::{AstNode, BlankPropertyList, Triple},
    syntax_kind::SyntaxKind::{self},
};
use std::collections::HashSet;

#[cfg(not(target_arch = "wasm32"))]
use tokio::task::spawn_local;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;

pub async fn completions(
    server_rc: Rc<Mutex<Server>>,
    environment: &CompletionEnvironment,
) -> Result<CompletionList, CompletionError> {
    let property_list = expect_completion_location(&environment.location)?;
    let mut template_context = environment.template_context().await;
    template_context.insert(
        "local_context",
        &local_context(
            property_list,
            &environment.continuations,
            environment.anchor_token.as_ref(),
        ),
    );
    // NOTE: the context talks about the enclosing subject, the local context about the
    //       blank node. The join is the triple that links the two.
    if let Some(join) = context_join(property_list) {
        let mut context = environment.context.clone().unwrap_or_default();
        // NOTE: the enclosing triple is the trigger node of the context computation and
        //          therefore not part of it. Its other properties would be lost otherwise.
        context.raw_inject = match sibling_properties(property_list) {
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
            CompletionTemplate::PredicateCompletionContextInsensitive,
            true,
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
        CompletionTemplate::PredicateCompletionContextSensitive,
        true,
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

fn expect_completion_location(
    location: &CompletionLocation,
) -> Result<&BlankPropertyList, CompletionError> {
    if let CompletionLocation::BlankNodeProperty(prop_list) = location {
        Ok(prop_list)
    } else {
        Err(CompletionError::Resolve(format!(
            "unexpected completion location: {:?}",
            location
        )))
    }
}

/// Build the link between the `context` and the `local_context` of a blank node
/// property completion: the triple that connects the enclosing subject to the
/// blank node, with the blank node bound to `?qls_blank_node`.
///
/// ```sparql
/// SELECT * { ?s :p [ :bar/ ] }
/// ```
/// yields `?s :p ?qls_blank_node`.
///
/// `None` when the blank node is not the direct object of a triple, e.g. when it
/// is the subject, nested in another blank node or an element of a collection.
pub(super) fn context_join(prop_list: &BlankPropertyList) -> Option<String> {
    // NOTE: walk up to the object list the blank node is an element of.
    //       Anything else on the way up means the blank node is not the object
    //       of a triple with a subject we could join against.
    // WARNING: `ancestors()` yields the node itself first, which is a barrier kind,
    //          hence the hop to the parent.
    let object_list = prop_list
        .syntax()
        .parent()?
        .ancestors()
        // NOTE: crossing one of these means the blank node is not the object of a
        //       triple with a subject to join against.
        .take_while(|ancestor| {
            !matches!(
                ancestor.kind(),
                SyntaxKind::CollectionPath
                    | SyntaxKind::Collection
                    | SyntaxKind::BlankNodePropertyListPath
                    | SyntaxKind::BlankNodePropertyList
                    | SyntaxKind::TriplesSameSubjectPath
                    | SyntaxKind::TriplesSameSubject
            )
        })
        .find(|ancestor| {
            matches!(
                ancestor.kind(),
                SyntaxKind::ObjectListPath | SyntaxKind::ObjectList
            )
        })?;
    // NOTE: grammar: PropertyListPathNotEmpty => (VerbPath | VerbSimple) ObjectListPath ...
    //       so the verb of the blank node is the sibling right before its object list.
    let verb = object_list.prev_sibling()?;
    // WARNING: a nested blank node property list also has a PropertyList(Path)NotEmpty
    //          parent, the cast keeps only the ones that carry a real subject.
    let triple = Triple::cast(object_list.parent()?.parent()?)?;
    let subject = triple.subject()?.text();
    Some(format!("{} {} ?qls_blank_node", subject, verb.text()))
}

/// The properties of the enclosing triple that do **not** lead to the blank node.
///
/// ```sparql
/// SELECT * { ?s :a <urn:a> ; :p [ :bar/ ] }
/// ```
/// yields `?s :a <urn:a>`.
///
/// NOTE: these are needed because the enclosing triple is the trigger node of the
///       context computation and therefore excluded from the context itself.
pub(super) fn sibling_properties(prop_list: &BlankPropertyList) -> Option<String> {
    let blank_node_range = prop_list.syntax().text_range();
    let triple = prop_list.triple()?;
    let subject = triple.subject()?.text();
    let properties: Vec<String> = triple
        .properties_list_path()?
        .properties()
        .iter()
        // INFO: a property without object is still being typed, it can not be injected.
        .filter(|property| property.object.is_some())
        .filter(|property| !property.text_range().contains_range(blank_node_range))
        .map(|property| property.text())
        .collect();
    if properties.is_empty() {
        return None;
    }
    Some(format!("{} {}", subject, properties.join(" ; ")))
}

/// Build the local context for a blank node property completion:
/// the properties of the blank node, with the property at the cursor reduced to
/// plain triples.
fn local_context(
    prop_list: &BlankPropertyList,
    continuations: &HashSet<SyntaxKind>,
    anchor_token: Option<&SyntaxToken>,
) -> Option<String> {
    // NOTE: a property list continuation means the user is starting a fresh property,
    //       so there is nothing to reduce and the context is the bare blank node.
    let continues_property_list = continuations.contains(&SyntaxKind::PropertyListPath)
        || continuations.contains(&SyntaxKind::PropertyListPathNotEmpty);
    let Some(property_list) = prop_list
        .property_list()
        .filter(|_| !continues_property_list)
    else {
        return Some("?qls_blank_node ?qls_entity []".to_string());
    };
    if continuations.contains(&SyntaxKind::VerbPath) {
        return Some("?qls_blank_node ?qls_entity []".to_string());
    }
    let anchor_end = anchor_token?.text_range().end();
    let properties = property_list.properties();
    let (last_prop, prev_props) = properties.split_last()?;
    let reduced_last = reduce_path("?qls_blank_node", Some(&last_prop.verb), "[]", anchor_end)?;
    if prev_props.is_empty() {
        Some(reduced_last)
    } else {
        Some(format!(
            "?qls_blank_node {} . {}",
            prev_props
                .iter()
                .map(|prop| prop.text())
                .collect::<Vec<_>>()
                .join(" ; "),
            reduced_last
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{context_join, local_context, sibling_properties};
    use indoc::indoc;
    use ll_sparql_parser::{
        SyntaxToken,
        ast::{AstNode, BlankPropertyList},
        parse_query,
        syntax_kind::SyntaxKind,
    };
    use std::collections::HashSet;

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

    fn continuations(kinds: &[SyntaxKind]) -> HashSet<SyntaxKind> {
        kinds.iter().copied().collect()
    }

    #[test]
    fn missing_anchor_token_yields_no_context() {
        // NOTE: this used to panic on `anchor_token.unwrap()`.
        let query = "SELECT * { ?s ?p [ a :Foo ] }";
        assert_eq!(
            local_context(&blank_property_list(query), &continuations(&[]), None),
            None
        );
    }

    #[test]
    fn property_list_continuation_yields_the_bare_blank_node() {
        let query = "SELECT * { ?s ?p [ a :Foo ; ] }";
        assert_eq!(
            local_context(
                &blank_property_list(query),
                &continuations(&[SyntaxKind::PropertyListPathNotEmpty]),
                Some(&anchor(query, ";")),
            )
            .as_deref(),
            Some("?qls_blank_node ?qls_entity []")
        );
    }

    #[test]
    fn verb_path_continuation_yields_the_bare_blank_node() {
        let query = "SELECT * { ?s ?p [ a :Foo ] }";
        assert_eq!(
            local_context(
                &blank_property_list(query),
                &continuations(&[SyntaxKind::VerbPath]),
                Some(&anchor(query, ":Foo")),
            )
            .as_deref(),
            Some("?qls_blank_node ?qls_entity []")
        );
    }

    #[test]
    fn single_property_is_reduced() {
        let query = "SELECT * { ?s ?p [ :bar/ ] }";
        assert_eq!(
            local_context(
                &blank_property_list(query),
                &continuations(&[]),
                Some(&anchor(query, "/")),
            )
            .as_deref(),
            Some("?qls_blank_node :bar ?qls_inner . ?qls_inner ?qls_entity []")
        );
    }

    #[test]
    fn preceding_properties_are_kept() {
        let query = "SELECT * { ?s ?p [ a :Foo ; :bar/ ] }";
        assert_eq!(
            local_context(
                &blank_property_list(query),
                &continuations(&[]),
                Some(&anchor(query, "/")),
            )
            .as_deref(),
            Some(
                "?qls_blank_node a :Foo . ?qls_blank_node :bar ?qls_inner . ?qls_inner ?qls_entity []"
            )
        );
    }

    #[test]
    fn join_links_the_subject_to_the_blank_node() {
        let query = "SELECT * { ?s :p [ :bar/ ] }";
        assert_eq!(
            context_join(&blank_property_list(query)).as_deref(),
            Some("?s :p ?qls_blank_node")
        );
    }

    #[test]
    fn join_keeps_an_iri_subject() {
        let query = "SELECT * { <urn:s> :p [ ] }";
        assert_eq!(
            context_join(&blank_property_list(query)).as_deref(),
            Some("<urn:s> :p ?qls_blank_node")
        );
    }

    #[test]
    fn join_keeps_a_variable_predicate() {
        let query = "SELECT * { ?s ?p [ ] }";
        assert_eq!(
            context_join(&blank_property_list(query)).as_deref(),
            Some("?s ?p ?qls_blank_node")
        );
    }

    #[test]
    fn join_keeps_a_property_path_predicate() {
        let query = "SELECT * { ?s :a/:b [ ] }";
        assert_eq!(
            context_join(&blank_property_list(query)).as_deref(),
            Some("?s :a/:b ?qls_blank_node")
        );
    }

    #[test]
    fn join_uses_the_verb_the_blank_node_hangs_off() {
        // NOTE: the blank node is the object of the *second* property of the list.
        let query = "SELECT * { ?s :q <urn:o> ; :p [ ] }";
        assert_eq!(
            context_join(&blank_property_list(query)).as_deref(),
            Some("?s :p ?qls_blank_node")
        );
    }

    #[test]
    fn join_ignores_sibling_objects() {
        let query = "SELECT * { ?s :p <urn:o> , [ ] }";
        assert_eq!(
            context_join(&blank_property_list(query)).as_deref(),
            Some("?s :p ?qls_blank_node")
        );
    }

    #[test]
    fn join_of_a_subject_blank_node_is_none() {
        // NOTE: the blank node is the subject, so there is no incoming link to build.
        let query = "SELECT * { [ :bar/ ] }";
        assert_eq!(context_join(&blank_property_list(query)), None);
    }

    #[test]
    fn join_of_a_nested_blank_node_is_none() {
        // NOTE: the enclosing subject is itself a blank node, so there is nothing
        //       in the (outer) context to join against.
        let query = "SELECT * { ?s :p [ :q [ :bar/ ] ] }";
        assert_eq!(context_join(&innermost_blank_property_list(query)), None);
    }

    #[test]
    fn join_of_a_middle_object_in_the_list() {
        // NOTE: the blank node sits between two other objects of the same verb.
        let query = "SELECT * { ?s :p <urn:a> , [ ] , <urn:b> }";
        assert_eq!(
            context_join(&blank_property_list(query)).as_deref(),
            Some("?s :p ?qls_blank_node")
        );
    }

    #[test]
    fn join_of_a_leading_object_in_the_list() {
        let query = "SELECT * { ?s :p [ ] , <urn:a> , <urn:b> }";
        assert_eq!(
            context_join(&blank_property_list(query)).as_deref(),
            Some("?s :p ?qls_blank_node")
        );
    }

    #[test]
    fn join_of_an_object_list_under_the_second_verb() {
        // NOTE: both a property list and an object list have to be traversed.
        let query = "SELECT * { ?s :q <urn:o> ; :p <urn:a> , [ :bar/ ] }";
        assert_eq!(
            context_join(&blank_property_list(query)).as_deref(),
            Some("?s :p ?qls_blank_node")
        );
    }

    #[test]
    fn join_keeps_an_inverse_path_predicate() {
        let query = "SELECT * { ?s ^:p [ ] }";
        assert_eq!(
            context_join(&blank_property_list(query)).as_deref(),
            Some("?s ^:p ?qls_blank_node")
        );
    }

    #[test]
    fn join_keeps_an_alternative_path_predicate() {
        let query = "SELECT * { ?s :a|:b [ ] }";
        assert_eq!(
            context_join(&blank_property_list(query)).as_deref(),
            Some("?s :a|:b ?qls_blank_node")
        );
    }

    #[test]
    fn join_keeps_a_modified_path_predicate() {
        let query = "SELECT * { ?s :a/:b* [ ] }";
        assert_eq!(
            context_join(&blank_property_list(query)).as_deref(),
            Some("?s :a/:b* ?qls_blank_node")
        );
    }

    #[test]
    fn join_keeps_a_grouped_path_predicate() {
        let query = "SELECT * { ?s (:a|:b)+ [ ] }";
        assert_eq!(
            context_join(&blank_property_list(query)).as_deref(),
            Some("?s (:a|:b)+ ?qls_blank_node")
        );
    }

    #[test]
    fn join_of_a_blank_node_inside_a_collection_is_none() {
        // NOTE: inside an RDF collection the blank node is not the object of `:p`,
        //       it is an element of the list, so there is no single triple to join.
        let query = "SELECT * { ?s :p ( <urn:a> [ :bar/ ] ) }";
        assert_eq!(context_join(&blank_property_list(query)), None);
    }

    #[test]
    fn siblings_of_the_enclosing_triple_are_kept() {
        let query = "SELECT * { ?s :a <urn:a> ; :p [ :bar/ ] }";
        assert_eq!(
            sibling_properties(&blank_property_list(query)).as_deref(),
            Some("?s :a <urn:a>")
        );
    }

    #[test]
    fn siblings_of_a_lone_property_are_none() {
        let query = "SELECT * { ?s :p [ :bar/ ] }";
        assert_eq!(sibling_properties(&blank_property_list(query)), None);
    }

    #[test]
    fn siblings_keep_all_preceding_properties() {
        let query = "SELECT * { ?s :a <urn:a> ; :b ?x ; :p [ :bar/ ] }";
        assert_eq!(
            sibling_properties(&blank_property_list(query)).as_deref(),
            Some("?s :a <urn:a> ; :b ?x")
        );
    }

    #[test]
    fn siblings_drop_the_property_being_typed() {
        // NOTE: the trailing `:b` has no object yet, injecting it would not parse.
        let query = "SELECT * { ?s :a <urn:a> ; :p [ ?x ?y ] ; :b }";
        assert_eq!(
            sibling_properties(&blank_property_list(query)).as_deref(),
            Some("?s :a <urn:a>")
        );
    }

    #[test]
    fn siblings_of_a_labeled_subject() {
        // NOTE: the reported case: the label triple used to be lost completely.
        let query = indoc!(
            r#"PREFIX unio: <http://uni-freiburg.de/ontology/>
               PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
               SELECT * WHERE {
                 ?a rdfs:label "Informatik"@de ;
                    unio:teachingUnit [ unio:faculty ?faculty ;
                     ] .
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
