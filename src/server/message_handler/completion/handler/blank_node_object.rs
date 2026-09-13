use super::{
    super::environment::{CompletionEnvironment, CompletionLocation},
    super::error::CompletionError,
    super::utils::{CompletionTemplate, dispatch_completion_query},
};
use crate::server::{Server, lsp::CompletionList};
use futures::lock::Mutex;
use ll_sparql_parser::{
    SyntaxToken,
    ast::{AstNode, PropertyListPath},
};
use std::rc::Rc;

pub async fn completions(
    server_rc: Rc<Mutex<Server>>,
    environment: &CompletionEnvironment,
) -> Result<CompletionList, CompletionError> {
    let mut template_context = environment.template_context().await;
    template_context.insert("local_context", &local_context(environment));
    dispatch_completion_query(
        server_rc,
        environment,
        template_context,
        CompletionTemplate::ObjectCompletionContextInsensitive,
        false,
    )
    .await
}

fn local_context(environment: &CompletionEnvironment) -> Option<String> {
    let CompletionLocation::BlankNodeObject(blank_node_props) = &environment.location else {
        return None;
    };
    build_local_context(
        &blank_node_props.property_list()?,
        environment.anchor_token.as_ref()?,
    )
}

/// Build the local context for a blank node object completion:
/// the property list of the blank node, clipped at the anchor token.
fn build_local_context(
    property_list: &PropertyListPath,
    anchor_token: &SyntaxToken,
) -> Option<String> {
    // NOTE: the anchor token has to be inside the property list, otherwise there is no
    //       meaningful local context to build.
    if !property_list
        .syntax()
        .text_range()
        .contains_range(anchor_token.text_range())
    {
        return None;
    }
    let clipped_text = property_list.text_until(anchor_token.text_range().end());
    Some(format!("[] {} ?qls_entity", clipped_text))
}

#[cfg(test)]
mod tests {
    use super::build_local_context;
    use ll_sparql_parser::{
        SyntaxToken,
        ast::{AstNode, BlankPropertyList, PropertyListPath},
        parse_query,
    };

    fn blank_node_property_list(query: &str) -> PropertyListPath {
        let (root, _) = parse_query(query);
        root.descendants()
            .find_map(BlankPropertyList::cast)
            .expect("query contains a blank node property list")
            .property_list()
            .expect("blank node has a property list")
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
            build_local_context(&blank_node_property_list(query), &anchor(query, ":bar"))
                .as_deref(),
            Some("[] a :Foo ; :bar ?qls_entity")
        );
    }

    #[test]
    fn full_property_list_is_kept_when_the_anchor_is_the_last_token() {
        let query = "SELECT * { ?s ?p [ a :Foo ; :bar ] }";
        assert_eq!(
            build_local_context(&blank_node_property_list(query), &anchor(query, ":bar"))
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
            build_local_context(&blank_node_property_list(query), &anchor(query, "?y")),
            None
        );
    }
}
