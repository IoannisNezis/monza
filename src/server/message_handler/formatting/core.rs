use std::{collections::HashSet, usize};

use log::{error, warn};
use tree_sitter_c2rust::{Node, Query, QueryCursor, Tree, TreeCursor};

use crate::lsp::{
    textdocument::{TextDocumentItem, TextEdit},
    FormattingOptions,
};

use super::utils::KEYWORDS;

pub(super) fn format_textdoument(
    document: &TextDocumentItem,
    tree: &Tree,
    options: &FormattingOptions,
) -> Vec<TextEdit> {
    let range = document.get_full_range();
    let indent_string = match options.insert_spaces {
        true => " ".repeat(options.tab_size as usize),
        false => "\t".to_string(),
    };
    let text = format_helper(&document.text, &mut tree.walk(), 0, &indent_string, "");
    vec![TextEdit::new(range, text)]
}

pub(super) fn format_helper(
    text: &String,
    cursor: &mut TreeCursor,
    indentation: usize,
    indent_base: &str,
    extra_indent: &str,
) -> String {
    let indent_str = &indent_base.repeat(indentation);
    let indent_str_small = match indentation {
        0 => String::new(),
        i => (&indent_base).repeat(i - 1),
    };
    let line_break = "\n".to_string() + &indent_str;
    let line_break_small = "\n".to_string() + &indent_str_small;

    match cursor.node().kind() {
        "unit" => separate_children_by(text, &cursor.node(), &line_break, 0, indent_base),
        "Update" => {
            separate_children_by(text, &cursor.node(), " ", 0, indent_base).replace("; ", ";\n")
        }
        "Prologue" | "GroupOrUnionGraphPattern" | "MinusGraphPattern" => {
            let mut formatted_string =
                separate_children_by(text, &cursor.node(), &line_break, indentation, indent_base);

            // Indent the prefix iris to align.
            if let Ok(query) = Query::new(
                &tree_sitter_sparql::language(),
                "(PrefixDecl (PNAME_NS) @prefix)",
            ) {
                // Step 1: Get all prefix strs and their lengths.
                // NOTE: Here a `HashSet` is used to avoid douplication of prefixes.
                let mut query_cursor = QueryCursor::new();
                let captures = query_cursor.captures(&query, cursor.node(), text.as_bytes());
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
                error!("Query string to retrieve prefixes in invalid!\nIndentation of Prefixes was aborted.");
            }
            return formatted_string;
        }
        "Modify" => {
            separate_children_by(text, &cursor.node(), &line_break, indentation, indent_base)
                .replace("WITH\n", "WITH ")
                .replace("WHERE\n{", "WHERE {")
        }
        "BaseDecl"
        | "PrefixDecl"
        | "SubSelect"
        | "DatasetClause"
        | "DefaultGraphClause"
        | "NamedGraphClause"
        | "TriplesSameSubject"
        | "property"
        | "WhereClause"
        | "OptionalGraphPattern"
        | "GraphGraphPattern"
        | "ServiceGraphPattern"
        | "Filter"
        | "binary_expression"
        | "Bind"
        | "InlineData"
        | "ValuesClause"
        | "DataBlock"
        | "GroupClause"
        | "GroupCondition"
        | "HavingClause"
        | "HavingCondition"
        | "OrderClause"
        | "OrderCondition"
        | "LimitOffsetClauses"
        | "LimitClause"
        | "OffsetClause"
        | "ExistsFunc"
        | "NotExistsFunc"
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
        | "Path" => separate_children_by(text, &cursor.node(), " ", indentation, indent_base),
        "Aggregate" | "SelectClause" => {
            separate_children_by(text, &cursor.node(), " ", indentation, indent_base)
                .replace("( ", "(")
                .replace(" )", ")")
        }

        "ConstructQuery" => {
            separate_children_by(text, &cursor.node(), "\n", indentation, indent_base)
                .replace("\n{", " {")
        }
        "SelectQuery" | "DescribeQuery" | "AskQuery" => {
            separate_children_by(text, &cursor.node(), " ", indentation, indent_base)
                .replace("} \n", "}\n")
        }
        "ObjectList" | "ExpressionList" | "SubstringExpression" | "RegexExpression" | "ArgList" => {
            separate_children_by(text, &cursor.node(), "", 0, indent_base).replace(",", ", ")
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

                subject.utf8_text(text.as_bytes()).unwrap().to_string()
                    + " "
                    + &format_helper(
                        text,
                        &mut predicate.walk(),
                        indentation,
                        indent_base,
                        &" ".repeat(range.end_point.column - range.start_point.column + 1),
                    )
            }
            _ => separate_children_by(text, &cursor.node(), " ", indentation, indent_base),
        },
        "PropertyListPathNotEmpty" => {
            separate_children_by(text, &cursor.node(), " ", indentation, indent_base)
                .replace("; ", &(";".to_string() + &line_break + extra_indent))
        }
        "GroupGraphPattern" | "BrackettedExpression" | "ConstructTemplate" | "QuadData" => {
            separate_children_by(text, &cursor.node(), "", indentation + 1, indent_base)
        }
        "BuildInCall" | "FunctionCall" | "PathSequence" | "PathEltOrInverse" | "PathElt"
        | "PathPrimary" => separate_children_by(text, &cursor.node(), "", indentation, indent_base),
        "QuadsNotTriples" => {
            separate_children_by(text, &cursor.node(), " ", indentation + 1, indent_base)
        }
        "TriplesTemplateBlock" => {
            separate_children_by(text, &cursor.node(), &line_break, indentation, indent_base)
                .replace(&(line_break + "}"), &(line_break_small + "}"))
        }
        "GroupGraphPatternSub" | "ConstructTriples" | "Quads" => {
            line_break.clone()
                + &separate_children_by(text, &cursor.node(), &line_break, indentation, indent_base)
                    .replace(&(line_break + "."), " .")
                + &line_break_small
        }
        "SolutionModifier" => {
            line_break.clone()
                + &separate_children_by(text, &cursor.node(), &line_break, indentation, indent_base)
        }
        "TriplesBlock" | "TriplesTemplate" => {
            separate_children_by(text, &cursor.node(), " ", indentation, indent_base)
                .replace(". ", &(".".to_string() + &line_break))
        }

        keyword if (KEYWORDS.contains(&keyword)) => keyword.to_string(),
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
) -> String {
    node.children(&mut node.walk())
        .map(|node| format_helper(text, &mut node.walk(), indentation, indent_base, ""))
        .collect::<Vec<String>>()
        .join(seperator)
}
