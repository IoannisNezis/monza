mod state;

use std::collections::HashSet;

use log::{error, info};
pub use state::*;

use tree_sitter_c2rust::{Node, Query, QueryCursor};

use crate::lsp::textdocument::{Position, Range};

fn collect_all_unique_captures(node: Node, query_str: &str, text: &String) -> Vec<String> {
    match Query::new(&tree_sitter_sparql::language(), query_str) {
        Ok(query) => QueryCursor::new()
            .captures(&query, node, text.as_bytes())
            .map(|(query_match, capture_index)| {
                query_match.captures[capture_index]
                    .node
                    .utf8_text(text.as_bytes())
                    .unwrap()
                    .split_at(1)
                    .1
                    .to_string()
            })
            .collect::<HashSet<String>>()
            .into_iter()
            .collect(),

        Err(_) => {
            error!("Building a tree-sitter query failed: {}", query_str);
            vec![]
        }
    }
}

pub fn get_all_variables(analyis_state: &AnalysisState, uri: &String) -> Vec<String> {
    match analyis_state.get_state(uri) {
        Some((document, Some(tree))) => {
            collect_all_unique_captures(tree.root_node(), "(VAR) @variable", &document.text)
        }
        Some((_document, None)) => {
            info!("Could not compute variables for {}: No tree availible", uri);
            vec![]
        }
        None => {
            error!("Could not compute variables for {}: No such document", uri);
            vec![]
        }
    }
}

pub fn get_kind_at_position(
    analyis_state: &AnalysisState,
    uri: &String,
    position: &Position,
) -> Option<&'static str> {
    match analyis_state.get_tree(uri) {
        Some(tree) => {
            let point = position.to_point();
            Some(
                tree.root_node()
                    .descendant_for_point_range(point, point)?
                    .kind(),
            )
        }
        None => None,
    }
}

pub fn get_declared_namspaces(analyis_state: &AnalysisState, uri: &String) -> Vec<(String, Range)> {
    match analyis_state.get_state(uri) {
        Some((document, Some(tree))) => {
            match Query::new(
                &tree_sitter_sparql::language(),
                "(PrefixDecl (PNAME_NS) @namespace)",
            ) {
                Ok(query) => QueryCursor::new()
                    .captures(&query, tree.root_node(), document.text.as_bytes())
                    .map(|(query_match, capture_index)| {
                        let node = query_match.captures[capture_index].node;
                        (
                            node.utf8_text(document.text.as_bytes())
                                .unwrap()
                                .to_string(),
                            Range::from_node(node),
                        )
                    })
                    .collect(),

                Err(_) => {
                    error!(
                        "Could not compute declared namspaces for {}: Query could not be build",
                        uri
                    );
                    vec![]
                }
            }
        }
        Some((_document, None)) => {
            info!(
                "Could not compute declared namespaces for {}: No tree availible",
                uri
            );
            vec![]
        }
        None => {
            error!(
                "Could not compute declared namspaces for {}: No such document",
                uri
            );
            vec![]
        }
    }
}

pub fn get_used_namspaces(analyis_state: &AnalysisState, uri: &String) -> Vec<(String, Range)> {
    match analyis_state.get_state(uri) {
        Some((document, Some(tree))) => {
            match Query::new(
                &tree_sitter_sparql::language(),
                "(PrefixedName (PNAME_NS) @namespace)",
            ) {
                Ok(query) => QueryCursor::new()
                    .captures(&query, tree.root_node(), document.text.as_bytes())
                    .map(|(query_match, capture_index)| {
                        let node = query_match.captures[capture_index].node;
                        (
                            node.utf8_text(document.text.as_bytes())
                                .unwrap()
                                .to_string(),
                            Range::from_node(node),
                        )
                    })
                    .collect(),

                Err(_) => {
                    error!(
                        "Could not compute declared namspaces for {}: Query could not be build",
                        uri
                    );
                    vec![]
                }
            }
        }
        Some((_document, None)) => {
            info!(
                "Could not compute declared namespaces for {}: No tree availible",
                uri
            );
            vec![]
        }
        None => {
            error!(
                "Could not compute declared namspaces for {}: No such document",
                uri
            );
            vec![]
        }
    }
}

pub(crate) fn get_unused_prefixes(
    analysis_state: &AnalysisState,
    uri: &String,
) -> impl Iterator<Item = (String, Range)> {
    let declared_namespaces = get_declared_namspaces(analysis_state, uri);
    let declared_namespaces_set: HashSet<String> = declared_namespaces
        .iter()
        .map(|(namespace, _range)| namespace)
        .cloned()
        .collect();
    let used_namespaces = get_used_namspaces(analysis_state, uri);
    let used_namespaces_set: HashSet<String> = used_namespaces
        .iter()
        .map(|(namespace, _range)| namespace)
        .cloned()
        .collect();

    declared_namespaces
        .iter()
        .filter_map(move |(declared_namespace, range)| {
            (&declared_namespaces_set - &used_namespaces_set)
                .contains(declared_namespace)
                .then_some((declared_namespace.clone(), range.clone()))
        })
        .collect::<Vec<(String, Range)>>()
        .into_iter()
}

pub(crate) fn get_undeclared_prefixes(
    analysis_state: &AnalysisState,
    uri: &String,
) -> impl Iterator<Item = (String, Range)> {
    let declared_namespaces = get_declared_namspaces(analysis_state, uri);
    let declared_namespaces_set: HashSet<String> = declared_namespaces
        .iter()
        .map(|(namespace, _range)| namespace)
        .cloned()
        .collect();
    let used_namespaces = get_used_namspaces(analysis_state, uri);
    let used_namespaces_set: HashSet<String> = used_namespaces
        .iter()
        .map(|(namespace, _range)| namespace)
        .cloned()
        .collect();

    used_namespaces
        .iter()
        .filter_map(|(declared_namespace, range)| {
            (&used_namespaces_set - &declared_namespaces_set)
                .contains(declared_namespace)
                .then_some((declared_namespace.clone(), range.clone()))
        })
        .collect::<Vec<(String, Range)>>()
        .into_iter()
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use crate::{
        analysis::{
            get_declared_namspaces, get_undeclared_prefixes, get_unused_prefixes,
            get_used_namspaces, AnalysisState,
        },
        lsp::textdocument::TextDocumentItem,
    };

    #[test]
    fn declared_namespaces() {
        let mut state = AnalysisState::new();
        state.add_document(TextDocumentItem::new(
            "uri",
            indoc!(
                "PREFIX wdt: <iri>
                 PREFIX wd: <iri>
                 PREFIX wdt: <iri>

                 SELECT * {}"
            ),
        ));
        let declared_namesapces = get_declared_namspaces(&state, &"uri".to_string());
        assert_eq!(
            declared_namesapces
                .iter()
                .map(|(namespace, _range)| namespace)
                .collect::<Vec<&String>>(),
            vec!["wdt:", "wd:", "wdt:"]
        );
    }

    #[test]
    fn used_namespaces() {
        let mut state = AnalysisState::new();
        state.add_document(TextDocumentItem::new(
            "uri",
            indoc!("SELECT * {?a wdt:P32 ?b. ?a wd:p32 ?b. ?a wdt:P31 ?b}"),
        ));
        let declared_namesapces = get_used_namspaces(&state, &"uri".to_string());
        assert_eq!(
            declared_namesapces
                .iter()
                .map(|(namespace, _range)| namespace)
                .collect::<Vec<&String>>(),
            vec!["wdt:", "wd:", "wdt:"]
        );
    }

    #[test]
    fn undeclared_namespaces() {
        let mut state = AnalysisState::new();
        state.add_document(TextDocumentItem::new(
            "uri",
            indoc!("SELECT * {x:y y:p x:x}"),
        ));
        let declared_namesapces: Vec<String> = get_undeclared_prefixes(&state, &"uri".to_string())
            .map(|(namespace, _range)| namespace)
            .collect();
        assert_eq!(declared_namesapces, vec!["x:", "y:", "x:"]);
    }
    #[test]
    fn unused_namespaces() {
        let mut state = AnalysisState::new();
        state.add_document(TextDocumentItem::new(
            "uri",
            indoc!(
                "PREFIX wdt: <>
                 PREFIX wdt: <>

                 SELECT * {}"
            ),
        ));
        let declared_namesapces: Vec<String> = get_unused_prefixes(&state, &"uri".to_string())
            .map(|(namespace, _range)| namespace)
            .collect();
        assert_eq!(declared_namesapces, vec!["wdt:", "wdt:"]);
    }
}
