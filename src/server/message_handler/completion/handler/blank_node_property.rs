use std::rc::Rc;

use super::{
    super::environment::{CompletionEnvironment, CompletionLocation},
    super::error::CompletionError,
    super::utils::{CompletionTemplate, dispatch_completion_query},
};
use crate::server::{Server, lsp::CompletionList, message_handler::completion::utils::reduce_path};
use futures::lock::Mutex;
use ll_sparql_parser::{SyntaxToken, ast::BlankPropertyList, syntax_kind::SyntaxKind};
use std::collections::HashSet;

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
        CompletionTemplate::PredicateCompletionContextInsensitive,
        true,
    )
    .await
}

fn local_context(environment: &CompletionEnvironment) -> Option<String> {
    let CompletionLocation::BlankNodeProperty(prop_list) = &environment.location else {
        return None;
    };
    build_local_context(
        prop_list,
        &environment.continuations,
        environment.anchor_token.as_ref(),
    )
}

/// Build the local context for a blank node property completion:
/// the properties of the blank node, with the property at the cursor reduced to
/// plain triples.
fn build_local_context(
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
        return Some("[] ?qls_entity []".to_string());
    };
    if continuations.contains(&SyntaxKind::VerbPath) {
        return Some("[] ?qls_entity []".to_string());
    }
    let anchor_end = anchor_token?.text_range().end();
    let properties = property_list.properties();
    let (last_prop, prev_props) = properties.split_last()?;
    let reduced_last = reduce_path("[]", Some(&last_prop.verb), "[]", anchor_end)?;
    if prev_props.is_empty() {
        Some(reduced_last)
    } else {
        Some(format!(
            "[] {} . {}",
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
    use super::build_local_context;
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
            build_local_context(&blank_property_list(query), &continuations(&[]), None),
            None
        );
    }

    #[test]
    fn property_list_continuation_yields_the_bare_blank_node() {
        let query = "SELECT * { ?s ?p [ a :Foo ; ] }";
        assert_eq!(
            build_local_context(
                &blank_property_list(query),
                &continuations(&[SyntaxKind::PropertyListPathNotEmpty]),
                Some(&anchor(query, ";")),
            )
            .as_deref(),
            Some("[] ?qls_entity []")
        );
    }

    #[test]
    fn verb_path_continuation_yields_the_bare_blank_node() {
        let query = "SELECT * { ?s ?p [ a :Foo ] }";
        assert_eq!(
            build_local_context(
                &blank_property_list(query),
                &continuations(&[SyntaxKind::VerbPath]),
                Some(&anchor(query, ":Foo")),
            )
            .as_deref(),
            Some("[] ?qls_entity []")
        );
    }

    #[test]
    fn single_property_is_reduced() {
        let query = "SELECT * { ?s ?p [ :bar/ ] }";
        assert_eq!(
            build_local_context(
                &blank_property_list(query),
                &continuations(&[]),
                Some(&anchor(query, "/")),
            )
            .as_deref(),
            Some("[] :bar ?qls_inner . ?qls_inner ?qls_entity []")
        );
    }

    #[test]
    fn preceding_properties_are_kept() {
        let query = "SELECT * { ?s ?p [ a :Foo ; :bar/ ] }";
        assert_eq!(
            build_local_context(
                &blank_property_list(query),
                &continuations(&[]),
                Some(&anchor(query, "/")),
            )
            .as_deref(),
            Some("[] a :Foo . [] :bar ?qls_inner . ?qls_inner ?qls_entity []")
        );
    }
}
