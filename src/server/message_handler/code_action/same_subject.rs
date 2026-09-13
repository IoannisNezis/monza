use std::collections::HashSet;

use crate::{
    FormatSettings,
    server::{
        Server,
        lsp::{
            CodeAction,
            base_types::LSPAny,
            diagnostic::Diagnostic,
            errors::{ErrorCode, LSPError},
            textdocument::{Position, Range, TextDocumentItem, TextEdit},
        },
        message_handler::{
            diagnostic::same_subject::find_all_triple_groups,
            indent::{column_at_offset, predicate_alignment_column},
        },
    },
};
use ll_sparql_parser::{
    Direction, SyntaxNode,
    ast::{AstNode, QueryUnit, Triple},
    syntax_kind::SyntaxKind,
};
use text_size::{TextRange, TextSize};

pub(crate) fn contract_all_triple_groups(
    document: &TextDocumentItem,
    root: SyntaxNode,
    format_settings: &FormatSettings,
) -> Result<Option<CodeAction>, LSPError> {
    let query_unit = {
        match QueryUnit::cast(root) {
            Some(x) => x,
            None => return Ok(None),
        }
    };
    let groups = find_all_triple_groups(&query_unit);

    let mut code_action = CodeAction::new("Contract all triples with same subject", None);
    let mut empty = true;
    let mut ranges: HashSet<Range> = HashSet::new();
    for action in groups
        .into_iter()
        .map(|(_subject, group)| contract_triples(group, document, format_settings))
    {
        for edit in action?
            .and_then(|action| action.edit.changes)
            .map(|changes| {
                changes
                    .into_iter()
                    .flat_map(|(uri, changes)| {
                        assert_eq!(document.uri, uri);
                        changes
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
        {
            if !ranges.contains(&edit.range) {
                ranges.insert(edit.range.clone());
                code_action.add_edit(&document.uri, edit);
                empty = false;
            }
        }
    }
    Ok((!empty).then_some(code_action))
}

type SameSubjectData = Vec<TextRange>;

pub(super) fn contract_triples_from_diagnostic(
    server: &Server,
    document_uri: &str,
    diagnostic: Diagnostic,
) -> Result<Option<CodeAction>, LSPError> {
    let data = extract_data(&diagnostic).ok_or(LSPError::new(
        ErrorCode::InvalidParams,
        "The same-subject diagnostic should have a array of ranges as data",
    ))?;
    let document = server.state.get_document(document_uri)?;
    let root = server.state.get_cached_parse_tree(document_uri)?.tree;
    let mut triples = Vec::new();
    for range in data {
        if !root.text_range().contains_range(range) {
            return Err(LSPError::new(
                ErrorCode::InvalidParams,
                &format!("The range {:?} is not contained in the document.", range),
            ));
        }
        let node = root
            .covering_element(range)
            .into_node()
            .ok_or(LSPError::new(
                ErrorCode::InvalidParams,
                &format!("The range {:?} does cover a token and not a node", range),
            ))?;
        let triple = Triple::cast(node).ok_or(LSPError::new(
            ErrorCode::InvalidParams,
            &format!("The range {:?} does not cover a triple", range),
        ))?;
        if triple.has_error() {
            return Err(LSPError::new(
                ErrorCode::InvalidParams,
                &format!("The range {:?} covers a triple that contains errors", range),
            ));
        }
        triples.push(triple);
    }
    contract_triples(triples, document, &server.settings.format)
}

pub(crate) fn contract_triples(
    triples: Vec<Triple>,
    document: &TextDocumentItem,
    format_settings: &FormatSettings,
) -> Result<Option<CodeAction>, LSPError> {
    let mut code_action = CodeAction::new(
        "contract triples with same subject",
        Some(crate::server::lsp::CodeActionKind::QuickFix),
    );
    for triple in triples.iter().skip(1) {
        let range = triple.syntax().text_range();
        let start = triple
            .syntax()
            .parent()
            .and_then(|parent| parent.prev_sibling_or_token())
            .and_then(|prev| {
                (prev.kind() == SyntaxKind::WHITESPACE).then_some(prev.text_range().start())
            })
            .unwrap_or(range.start());
        let end = triple
            .syntax()
            .siblings_with_tokens(Direction::Next)
            .skip(1)
            .find(|element| matches!(element.kind(), SyntaxKind::WHITESPACE | SyntaxKind::Dot))
            .map(|next| {
                next.next_sibling_or_token()
                    .and_then(|next_next| {
                        (next_next.kind() == SyntaxKind::Dot).then_some(next_next)
                    })
                    .unwrap_or(next)
            })
            .map(|next| next.text_range().end())
            .unwrap_or(range.end());
        code_action.add_edit(
            &document.uri,
            TextEdit::new(
                Range::from_byte_offset_range(TextRange::new(start, end), &document.text).unwrap(),
                "",
            ),
        );
    }
    if let Some(triple) = triples.first() {
        let indent_string = {
            let indentation = if format_settings.align_predicates {
                triple
                    .syntax()
                    .first_token()
                    .and_then(|tok| predicate_alignment_column(&tok, &document.text))
                    .expect("valid triple has a predicate alignment column")
            } else {
                let offset: usize = triple
                    .subject()
                    .map(|subject| subject.syntax().text_range().start().into())
                    .unwrap_or(0);
                column_at_offset(&document.text, offset)
                    + format_settings.tab_size.unwrap_or(2) as usize
            };

            " ".repeat(indentation)
        };
        code_action.add_edit(
            &document.uri,
            TextEdit::new(
                Range::empty(
                    Position::from_byte_index(triple.syntax().text_range().end(), &document.text)
                        .expect("The text rang of a node should be within the text"),
                ),
                &format!(
                    " ;\n{}{}",
                    indent_string,
                    triples
                        .iter()
                        .skip(1)
                        .filter_map(|triple| triple.properties_list_path())
                        .map(|plp| plp.text())
                        .collect::<Vec<_>>()
                        .join(&format!(" ;\n{}", indent_string))
                ),
            ),
        );
    }
    Ok(Some(code_action))
}

fn extract_data(diagnostic: &Diagnostic) -> Option<SameSubjectData> {
    let data = diagnostic.data.as_ref()?;
    if let LSPAny::LSPArray(ranges) = data {
        let mut res = Vec::new();
        for range in ranges.iter() {
            if let LSPAny::LSPObject(map) = range {
                let start = map.get("start");
                let end = map.get("end");
                if let (Some(LSPAny::Uinteger(start)), Some(LSPAny::Uinteger(end))) = (start, end) {
                    res.push(TextRange::new(TextSize::new(*start), TextSize::new(*end)));
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        Some(res)
    } else {
        None
    }
}
