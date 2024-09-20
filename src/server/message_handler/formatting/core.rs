use core::alloc;
use std::{collections::HashSet, usize};

use log::{error, warn};
use tree_sitter::{Node, Query, QueryCursor, Tree, TreeCursor};

use crate::{
    lsp::{
        textdocument::{TextDocumentItem, TextEdit},
        FormattingOptions,
    },
    server::configuration::FormatSettings,
};

use super::utils::KEYWORDS;

pub(super) fn format_textdoument(
    document: &TextDocumentItem,
    tree: &Tree,
    settings: &FormatSettings,
    options: &FormattingOptions,
) -> Vec<TextEdit> {
    let range = document.get_full_range();
    let indent_string = match settings.insert_spaces {
        true => " ".repeat(settings.tab_size as usize),
        false => "\t".to_string(),
    };
    let text = format_helper(
        &document.text,
        &mut tree.walk(),
        0,
        &indent_string,
        "",
        settings,
    );
    vec![TextEdit::new(range, text)]
}

pub(super) fn format_helper(
    text: &String,
    cursor: &mut TreeCursor,
    indentation: usize,
    indent_base: &str,
    extra_indent: &str,
    settings: &FormatSettings,
) -> String {
    let indent_str = &indent_base.repeat(indentation);
    let indent_str_small = match indentation {
        0 => String::new(),
        i => (&indent_base).repeat(i - 1),
    };
    let line_break = "\n".to_string() + &indent_str;
    let line_break_small = "\n".to_string() + &indent_str_small;

    match cursor.node().kind() {
        "unit" => separate_children_by(text, &cursor.node(), &line_break, 0, indent_base, settings)
            .replace("→", "")
            .replace("←", ""),
        "Update" => separate_children_by(text, &cursor.node(), " ", 0, indent_base, settings)
            .replace("; ", ";\n"),
        "Prologue" | "GroupOrUnionGraphPattern" | "MinusGraphPattern" => {
            let mut formatted_string = separate_children_by(
                text,
                &cursor.node(),
                &line_break,
                indentation,
                indent_base,
                settings,
            );
            if settings.align_prefixes {
                formatted_string = align_prefixes(formatted_string, &cursor.node(), text);
            }
            formatted_string.replace("→", "").replace("←", "")
                + match settings.separate_prolouge {
                    true => "\n",
                    false => "",
                }
        }
        "Modify" => separate_children_by(
            text,
            &cursor.node(),
            &line_break,
            indentation,
            indent_base,
            settings,
        )
        .replace("WITH\n", "WITH ")
        .replace("WHERE\n{", "WHERE {"),
        "BaseDecl"
        | "PrefixDecl"
        | "SelectClause"
        | "SubSelect"
        | "DatasetClause"
        | "DefaultGraphClause"
        | "NamedGraphClause"
        | "TriplesSameSubject"
        | "property"
        | "OptionalGraphPattern"
        | "GraphGraphPattern"
        | "ServiceGraphPattern"
        | "binary_expression"
        | "InlineData"
        | "ValuesClause"
        | "DataBlock"
        | "GroupClause"
        | "GroupCondition"
        | "HavingClause"
        | "HavingCondition"
        | "OrderClause"
        | "LimitClause"
        | "OffsetClause"
        | "ExistsFunc"
        | "NotExistsFunc"
        | "Filter"
        | "Bind"
        | "Load"
        | "Clear"
        | "Drop"
        | "Add"
        | "Move"
        | "Copy"
        | "Create"
        | "InsertData"
        | "DeleteData"
        | "DeleteWhere"
        | "GraphRef"
        | "GraphRefAll"
        | "GraphOrDefault"
        | "DeleteClause"
        | "InsertClause"
        | "UsingClause"
        | "PropertyListNotEmpty"
        | "Path" => separate_children_by(
            text,
            &cursor.node(),
            " ",
            indentation,
            indent_base,
            settings,
        ),
        "assignment" => separate_children_by(
            text,
            &cursor.node(),
            " ",
            indentation,
            indent_base,
            settings,
        )
        .replace("( ", "(")
        .replace(" )", ")"),
        "WhereClause" => {
            (match settings.where_new_line {
                true => line_break,
                false => String::new(),
            } + &separate_children_by(
                text,
                &cursor.node(),
                " ",
                indentation,
                indent_base,
                settings,
            ))
        }

        "ConstructQuery" => separate_children_by(
            text,
            &cursor.node(),
            "\n",
            indentation,
            indent_base,
            settings,
        )
        .replace("\n{", " {"),
        "SelectQuery" | "DescribeQuery" | "AskQuery" => separate_children_by(
            text,
            &cursor.node(),
            " ",
            indentation,
            indent_base,
            settings,
        )
        .replace("} \n", "}\n"),

        "ObjectList" | "ExpressionList" | "SubstringExpression" | "RegexExpression" | "ArgList" => {
            separate_children_by(text, &cursor.node(), "", 0, indent_base, settings)
                .replace(",", ", ")
        }
        "TriplesSameSubjectPath" => match cursor.node().child_count() {
            2 => {
                let subject = cursor
                    .node()
                    .child(0)
                    .expect("The first node has to exist, i just checked if there are 2.");
                let range = subject.range();
                let predicate = cursor
                    .node()
                    .child(1)
                    .expect("The second node has to exist, i just checked if there are 2.");
                let indent_str = match settings.align_predicates {
                    true => " ".repeat(range.end_point.column - range.start_point.column + 1),
                    false => indent_base.to_string(),
                };

                subject.utf8_text(text.as_bytes()).unwrap().to_string()
                    + " "
                    + &format_helper(
                        text,
                        &mut predicate.walk(),
                        indentation,
                        indent_base,
                        &indent_str,
                        settings,
                    )
            }
            _ => separate_children_by(
                text,
                &cursor.node(),
                " ",
                indentation,
                indent_base,
                settings,
            ),
        },
        "PropertyListPathNotEmpty" => separate_children_by(
            text,
            &cursor.node(),
            " ",
            indentation,
            indent_base,
            settings,
        )
        .replace("; ", &(";".to_string() + &line_break + extra_indent)),
        "GroupGraphPattern" | "BrackettedExpression" | "ConstructTemplate" | "QuadData" => {
            separate_children_by(
                text,
                &cursor.node(),
                "",
                indentation + 1,
                indent_base,
                settings,
            )
            .replace("{→", &("{".to_string() + &line_break + indent_base))
            .replace("→", indent_base)
            .replace("←}", &(line_break + "}"))
            .replace("←", "")
        }
        "OrderCondition" | "Aggregate" | "BuildInCall" | "FunctionCall" | "PathSequence"
        | "PathEltOrInverse" | "PathElt" | "PathPrimary" => {
            separate_children_by(text, &cursor.node(), "", indentation, indent_base, settings)
        }
        "QuadsNotTriples" => separate_children_by(
            text,
            &cursor.node(),
            " ",
            indentation + 1,
            indent_base,
            settings,
        ),
        "TriplesTemplateBlock" => separate_children_by(
            text,
            &cursor.node(),
            &line_break,
            indentation,
            indent_base,
            settings,
        )
        .replace(&(line_break + "}"), &(line_break_small + "}")),
        "GroupGraphPatternSub" | "ConstructTriples" | "Quads" => {
            line_break.clone()
                + &separate_children_by(
                    text,
                    &cursor.node(),
                    &line_break,
                    indentation,
                    indent_base,
                    settings,
                )
                .replace(&(line_break + "."), " .")
                .replace("→", "")
                .replace("←", "")
                + &line_break_small
        }
        "SolutionModifier" => {
            line_break.clone()
                + &separate_children_by(
                    text,
                    &cursor.node(),
                    &line_break,
                    indentation,
                    indent_base,
                    settings,
                )
        }
        "LimitOffsetClauses" => separate_children_by(
            text,
            &cursor.node(),
            &line_break,
            indentation,
            indent_base,
            settings,
        ),
        "TriplesBlock" | "TriplesTemplate" => separate_children_by(
            text,
            &cursor.node(),
            " ",
            indentation,
            indent_base,
            settings,
        )
        .replace(". ", &(".".to_string() + &line_break))
        .replace("→", "")
        .replace("← ", &line_break),
        // NOTE: Here a marker chars (→ and ←) are used to mark the start and end of a comment node.
        // Later these markers are removed.
        // This assumes that the marker char does not appear in queries.
        "comment" => "→".to_string() + cursor.node().utf8_text(text.as_bytes()).unwrap() + "←",
        keyword if (KEYWORDS.contains(&keyword)) => match settings.capitalize_keywords {
            true => keyword.to_string(),
            false => cursor
                .node()
                .utf8_text(text.as_bytes())
                .unwrap()
                .to_string(),
        },
        "PNAME_NS" | "IRIREF" | "VAR" | "INTEGER" | "DECIMAL" | "String" | "NIL"
        | "BLANK_NODE_LABEL" | "RdfLiteral" | "PrefixedName" | "PathMod" | "(" | ")" | "{"
        | "}" | "." | "," | ";" | "*" | "+" | "-" | "/" | "<" | ">" | "=" | ">=" | "<=" | "!="
        | "||" | "&&" | "|" | "^" => cursor
            .node()
            .utf8_text(text.as_bytes())
            .unwrap()
            .to_string(),
        // "ERROR" => {
        //     cursor.node().child
        //     if let Some(child) = cursor.node().child(0) {
        //         format_helper(
        //             text,
        //             &mut child.walk(),
        //             indentation,
        //             indent_base,
        //             extra_indent,
        //         )
        //     } else {
        //         String::new()
        //     }
        // }
        other => {
            warn!("found unknown node kind while formatting: {}", other);
            cursor
                .node()
                .utf8_text(text.as_bytes())
                .unwrap()
                .to_string()
        }
    }
}

fn separate_children_by(
    text: &String,
    node: &Node,
    seperator: &str,
    indentation: usize,
    indent_base: &str,
    settings: &FormatSettings,
) -> String {
    node.children(&mut node.walk())
        .map(|node| {
            format_helper(
                text,
                &mut node.walk(),
                indentation,
                indent_base,
                "",
                settings,
            )
        })
        .collect::<Vec<String>>()
        .join(seperator)
}

fn align_prefixes(mut formatted_string: String, node: &Node, text: &String) -> String {
    if let Ok(query) = Query::new(
        &tree_sitter_sparql::language(),
        "(PrefixDecl (PNAME_NS) @prefix)",
    ) {
        // Step 1: Get all prefix strs and their lengths.
        // NOTE: Here a `HashSet` is used to avoid douplication of prefixes.
        let mut query_cursor = QueryCursor::new();
        let captures = query_cursor.captures(&query, *node, text.as_bytes());
        let prefixes: HashSet<(&str, usize)> = captures
            .map(|(query_match, capture_index)| {
                let node = query_match.captures[capture_index].node;
                (
                    node.utf8_text(text.as_bytes()).unwrap(),
                    node.end_position().column - node.start_position().column,
                )
            })
            .collect();
        // Step 2: Get the length of the longest prefix.
        let max_prefix_length = prefixes
            .iter()
            .fold(0, |old_max, (_, length)| old_max.max(*length));
        // Step 3: Insert n spaces after each prefix, where n is the length difference to
        //         the longest prefix
        for (prefix, length) in prefixes {
            formatted_string = formatted_string.replace(
                &format!(" {} ", prefix),
                &format!(" {}{}", prefix, " ".repeat(max_prefix_length - length + 1)),
            );
        }
    } else {
        error!(
            "Query string to retrieve prefixes in invalid!\nIndentation of Prefixes was aborted."
        );
    }
    return formatted_string;
}
