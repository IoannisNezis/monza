use futures::lock::Mutex;
use ll_sparql_parser::{
    SyntaxNode,
    ast::{AstNode, Path, Prologue, QueryUnit},
    syntax_kind::SyntaxKind,
};
use std::{collections::HashMap, rc::Rc};
use tera::Context;
use text_size::TextSize;

use crate::{
    server::{
        Server,
        common::get_timestamp_ms,
        configuration::BackendConfiguration,
        lsp::{
            Command, CompletionItemBuilder, CompletionItemKind, CompletionItemLabelDetails,
            CompletionList,
            base_types::LSPAny,
            rpc::RequestId,
            textdocument::{Range, TextEdit},
        },
        sparql_operations::{SparqlRequestError, execute_query},
    },
    sparql::results::{RDFTerm, SparqlResultsBody},
};

use super::{environment::CompletionEnvironment, error::CompletionError};

/// Returns true if the label matches the search term as a case-insensitive prefix.
/// If no search term is provided (or it's empty), returns true to show all completions.
pub(super) fn matches_search_term(label: &str, search_term: Option<&str>) -> bool {
    match search_term {
        // NOTE: byte-wise ASCII comparison, avoids allocating uppercased copies;
        // slicing the bytes instead of the str avoids char-boundary panics
        Some(term) if !term.is_empty() => {
            label.len() >= term.len()
                && label.as_bytes()[..term.len()].eq_ignore_ascii_case(term.as_bytes())
        }
        _ => true,
    }
}

pub(super) type CompletionTemplate = crate::server::configuration::CompletionTemplate;

pub(super) async fn dispatch_completion_query(
    server_rc: Rc<Mutex<Server>>,
    environment: &CompletionEnvironment,
    template_context: Context,
    completion_template: CompletionTemplate,
    trigger_on_accept: bool,
) -> Result<CompletionList, CompletionError> {
    match environment.backend.as_ref() {
        Some(backend) => {
            let query_unit = QueryUnit::cast(environment.truncated_tree.clone()).ok_or(
                CompletionError::Resolve("Could not cast root to QueryUnit".to_string()),
            )?;
            Ok(to_completion_items(
                fetch_online_completions(
                    server_rc.clone(),
                    &query_unit,
                    backend,
                    &format!("{}-{}", backend.name, completion_template),
                    template_context,
                    &environment.request_id,
                )
                .await?,
                environment.replace_range.clone(),
                trigger_on_accept.then_some("triggerNewCompletion"),
                server_rc.lock().await.settings.completion.result_size_limit,
                environment.search_term.as_deref(),
            ))
        }
        _ => {
            tracing::info!("No Backend for completion query found");
            Err(CompletionError::Resolve("No Backend defined".to_string()))
        }
    }
}

pub(super) struct InternalCompletionItem {
    label: String,
    /// The alternative label that matched the search term, `None` when the
    /// query binds none.
    alias: Option<String>,
    /// A prose description of the entity, `None` when the query binds none.
    description: Option<String>,
    /// The facts of a literal, `None` for an IRI or a blank node.
    literal: Option<LiteralFacts>,
    /// The absolute IRI, `None` for a literal or a blank node.
    uri: Option<String>,
    value: String,
    _filter_text: Option<String>,
    score: Option<usize>,
    import_edit: Option<TextEdit>,
}

pub(super) struct LiteralFacts {
    /// The lexical form, without the quotes `value` carries.
    value: String,
    language: Option<String>,
    /// The datatype, shortened to a curie where the prefix map allows it.
    datatype: Option<String>,
}

const NO_BINDINGS_MESSAGE: &str = "The SPARQL result of a completion query did not contain bindings. Likely because its not a SELECT query.";

/// Flattens a tera error and its source chain into one line.
fn tera_error_message(error: &tera::Error) -> String {
    let mut message = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(error) = source {
        message.push_str(&format!(": {}", error));
        source = error.source();
    }
    message
}

fn request_error_message(error: SparqlRequestError) -> String {
    match error {
        SparqlRequestError::Timeout => "Completion query timed out".to_string(),
        SparqlRequestError::Connection(_err) => {
            "Completion query failed, connection errored".to_string()
        }
        SparqlRequestError::Canceled(_err) => "Completion query was canceled".to_string(),
        SparqlRequestError::Http(err) => format!(
            "Completion query failed with status {} {}",
            err.status, err.status_text
        ),
        SparqlRequestError::Deserialization(msg) => msg,
        SparqlRequestError::QLeverException(exception) => exception.exception,
    }
}

/// Notifies the client about a completion query, so it can be inspected while a completion
/// template is being edited. Diagnostic only, failures to send are ignored.
fn report_completion_query(
    server: &Server,
    request_id: &RequestId,
    template: &str,
    query: &str,
    url: &str,
    started: f64,
    result_count: Option<usize>,
    error: Option<String>,
) {
    // NOTE: On native the body below is compiled out, leaving every parameter unused.
    // Discarding them here marks them as used and avoids `unused_variables` warnings.
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (
        server,
        request_id,
        template,
        query,
        url,
        started,
        result_count,
        error,
    );
    #[cfg(target_arch = "wasm32")]
    {
        use crate::server::lsp::{CompletionQueryNotification, CompletionQueryParams};
        let _ = server.send_message(CompletionQueryNotification::new(CompletionQueryParams {
            request_id: request_id.clone(),
            template: template.to_string(),
            query: query.to_string(),
            url: url.to_string(),
            duration_ms: (get_timestamp_ms() - started) as u32,
            result_count,
            error,
        }));
    }
}

pub(super) async fn fetch_online_completions(
    server_rc: Rc<Mutex<Server>>,
    query_unit: &QueryUnit,
    backend: &BackendConfiguration,
    query_template: &str,
    mut query_template_context: Context,
    request_id: &RequestId,
) -> Result<Vec<InternalCompletionItem>, CompletionError> {
    let (url, query, timeout_ms, method) = {
        let server = server_rc.lock().await;
        query_template_context.insert("limit", &server.settings.completion.result_size_limit);
        query_template_context.insert("offset", &0);
        let url = backend.url.clone();
        let query = match server
            .tools
            .tera
            .render(query_template, &query_template_context)
        {
            Ok(query) => query,
            Err(err) => {
                report_completion_query(
                    &server,
                    request_id,
                    query_template,
                    "",
                    &url,
                    get_timestamp_ms(),
                    None,
                    Some(tera_error_message(&err)),
                );
                return Err(CompletionError::Template(query_template.to_string(), err));
            }
        };

        let timeout_ms = server.settings.completion.timeout_ms;
        let method = server.state.get_backend_request_method(&backend.name);
        (url, query, timeout_ms, method)
    };

    let started = get_timestamp_ms();
    let result = execute_query(
        server_rc.clone(),
        url.clone(),
        query.clone(),
        None,
        None,
        Some(timeout_ms),
        method,
        None,
        0,
        false,
    )
    .await;

    let result = match result {
        Ok(result) => result.expect("Non-lazy request should always return a result."),
        Err(err) => {
            let message = request_error_message(err);
            report_completion_query(
                &*server_rc.lock().await,
                request_id,
                query_template,
                &query,
                &url,
                started,
                None,
                Some(message.clone()),
            );
            return Err(CompletionError::Request(message));
        }
    };

    let SparqlResultsBody::Results { bindings } = result.body else {
        tracing::error!("{}", NO_BINDINGS_MESSAGE);
        report_completion_query(
            &*server_rc.lock().await,
            request_id,
            query_template,
            &query,
            &url,
            started,
            None,
            Some(NO_BINDINGS_MESSAGE.to_string()),
        );
        return Err(CompletionError::Resolve(NO_BINDINGS_MESSAGE.to_string()));
    };

    let mut server = server_rc.lock().await;
    report_completion_query(
        &server,
        request_id,
        query_template,
        &query,
        &url,
        started,
        Some(bindings.len()),
        None,
    );
    // NOTE: one result row is one completion item, in the order the backend
    // returned them.
    let mut items: Vec<InternalCompletionItem> = Vec::new();
    for binding in bindings {
        let rdf_term = binding.get("qls_entity").ok_or_else(|| {
            CompletionError::Request(
                "Completion query result is missing the `qls_entity` binding".to_string(),
            )
        })?;
        let alias = binding
            .get("qls_alias")
            .map(|rdf_term: &RDFTerm| rdf_term.value().to_string())
            .filter(|alias| !alias.is_empty());
        let description = binding
            .get("qls_description")
            .map(|rdf_term: &RDFTerm| rdf_term.value().to_string())
            .filter(|description| !description.is_empty());
        let (value, import_edit) = render_rdf_term(&server, query_unit, rdf_term, &backend.name);
        let label = binding
            .get("qls_label")
            .map_or(String::new(), |rdf_term| rdf_term.value().to_string());
        let score = binding
            .get("qls_count")
            .and_then(|rdf_term: &RDFTerm| rdf_term.value().parse().ok());
        // NOTE: This is the text the in editor filter uses.
        // If a compressed IRI is used as search term i.e. "wdt:p" the expanded iri is used as
        // filter text. Otherwise the label and alias are used as filter text.
        // This is currently not used, but should be redone at some point
        let filter_text = query_template_context
            .get("search_term_uncompressed")
            .is_some()
            .then_some(value.to_string())
            .or((!label.is_empty()).then_some(format!(
                "{}{}",
                label,
                alias.clone().unwrap_or_default()
            )))
            .or(Some(rdf_term.to_string()));
        if !label.is_empty() {
            server
                .state
                .label_memory
                .insert(value.clone(), label.clone());
        }
        let literal = match rdf_term {
            RDFTerm::Literal {
                value,
                lang,
                datatype,
            } => Some(LiteralFacts {
                value: value.clone(),
                language: lang.clone(),
                datatype: datatype
                    .as_ref()
                    .map(|datatype| shorten_datatype(&server, datatype, &backend.name)),
            }),
            _ => None,
        };
        // NOTE: `value` is the curie the query will read; a client that wants to
        // resolve the entity needs the IRI it stands for, and reconstructing it
        // from the prefix map is this server's job rather than the client's.
        let uri = match rdf_term {
            RDFTerm::Uri { value, .. } => Some(value.clone()),
            _ => None,
        };
        items.push(InternalCompletionItem {
            label,
            alias,
            description,
            literal,
            uri,
            value,
            _filter_text: filter_text,
            score,
            import_edit,
        });
    }
    Ok(items)
}

/// Renders a datatype IRI as a curie, falling back to the IRI when the
/// backend's prefix map has no prefix for it.
fn shorten_datatype(server: &Server, datatype: &str, backend_name: &str) -> String {
    server
        .shorten_uri(datatype, Some(backend_name))
        .map_or_else(|| datatype.to_string(), |(_, _, curie)| curie)
}

fn render_rdf_term(
    server: &Server,
    query_unit: &QueryUnit,
    rdf_term: &RDFTerm,
    backend_name: &str,
) -> (String, Option<TextEdit>) {
    match rdf_term {
        RDFTerm::Uri { value, curie: _ } => match server.shorten_uri(value, Some(backend_name)) {
            Some((prefix, uri, curie)) => {
                let prefix_decl_edit = if query_unit.prologue().as_ref().is_none_or(|prologue| {
                    prologue
                        .prefix_declarations()
                        .iter()
                        .all(|prefix_declaration| {
                            prefix_declaration
                                .prefix()
                                .is_some_and(|declared_prefix| declared_prefix != prefix)
                        })
                }) {
                    Some(TextEdit::new(
                        Range::new(0, 0, 0, 0),
                        &format!("PREFIX {}: <{}>\n", prefix, uri),
                    ))
                } else {
                    None
                };
                (curie, prefix_decl_edit)
            }
            None => (rdf_term.to_string(), None),
        },
        _ => (rdf_term.to_string(), None),
    }
}

pub(super) async fn get_prefix_declarations(root: &SyntaxNode) -> Vec<(String, String)> {
    root.first_child()
        .and_then(|child| child.first_child())
        .and_then(Prologue::cast)
        .map(|prologue| {
            prologue
                .prefix_declarations()
                .iter()
                .filter_map(|prefix_declaration| {
                    match (
                        prefix_declaration.prefix(),
                        prefix_declaration.raw_uri_prefix(),
                    ) {
                        (Some(prefix), Some(uri_prefix)) => Some((prefix, uri_prefix)),
                        _ => None,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn reduce_path(
    subject: &str,
    path: Option<&Path>,
    object: &str,
    offset: TextSize,
) -> Option<String> {
    let path = if let Some(path) = path {
        path
    } else {
        return Some(format!("{} ?qls_entity {}", subject, object));
    };
    if path.syntax().text_range().start() >= offset {
        return Some(format!("{} ?qls_entity {}", subject, object));
    }
    match path.syntax().kind() {
        SyntaxKind::PathPrimary | SyntaxKind::PathElt | SyntaxKind::Path | SyntaxKind::VerbPath => {
            reduce_path(
                subject,
                path.syntax().last_child().and_then(Path::cast).as_ref(),
                object,
                offset,
            )
        }
        SyntaxKind::PathAlternative => {
            reduce_path(subject, Some(&path.sub_paths().last()?), object, offset)
        }
        SyntaxKind::PathSequence => {
            let sub_paths = path
                .sub_paths()
                .map(|sub_path| sub_path.text())
                .collect::<Vec<_>>();
            let path_seq_len = sub_paths.len();
            // NOTE: a dangling separator ("<p0>/") has no sub-path after the
            // slash; the existing sub-paths become the prefix and the cursor
            // position is the empty final element
            if path
                .syntax()
                .last_child_or_token()
                .is_some_and(|elt| elt.kind() == SyntaxKind::Slash)
            {
                return Some(format!(
                    "{} {} ?qls_inner . ?qls_inner ?qls_entity {}",
                    subject,
                    sub_paths.join("/"),
                    object
                ));
            }
            if path_seq_len > 1 {
                let path_prefix = sub_paths[..path_seq_len - 1].join("/");
                let prefix = format!("{} {} {}", subject, path_prefix, "?qls_inner");
                Some(format!(
                    "{} . {}",
                    prefix,
                    reduce_path(
                        "?qls_inner",
                        Some(&path.sub_paths().last()?),
                        object,
                        offset
                    )?
                ))
            } else {
                reduce_path(subject, Some(&path.sub_paths().last()?), object, offset)
            }
        }
        SyntaxKind::PathEltOrInverse => {
            if path.syntax().first_child_or_token()?.kind() == SyntaxKind::Zirkumflex {
                // NOTE: Swap subject and object
                reduce_path(
                    object,
                    path.syntax().last_child().and_then(Path::cast).as_ref(),
                    subject,
                    offset,
                )
            } else {
                reduce_path(
                    subject,
                    path.syntax().last_child().and_then(Path::cast).as_ref(),
                    object,
                    offset,
                )
            }
        }
        SyntaxKind::PathNegatedPropertySet => match path.syntax().last_child() {
            Some(last_child) => {
                reduce_path(subject, Path::cast(last_child).as_ref(), object, offset)
            }
            _ => Some(format!("{} ?qls_entity {}", subject, object)),
        },
        SyntaxKind::PathOneInPropertySet => {
            let first_child = path.syntax().first_child_or_token()?;
            if first_child.kind() == SyntaxKind::Zirkumflex {
                if first_child.text_range().end() == offset {
                    Some(format!("{} ?qls_entity {}", object, subject))
                } else {
                    Some(format!("{} ?qls_entity {}", subject, object))
                }
            } else {
                Some(path.text().to_string())
            }
        }
        // WARNING: an unexpected path kind means the parse tree does not look like a
        //          path at all -> no local context instead of aborting.
        _ => None,
    }
}

pub(super) fn to_completion_items(
    mut items: Vec<InternalCompletionItem>,
    range: Range,
    command: Option<&str>,
    _limit: u32,
    search_term: Option<&str>,
) -> CompletionList {
    // NOTE: Rank by score (e.g. `?qls_count`) descending, items without a score go last.
    // The sort is stable, so items with equal (or missing) score keep the order the
    // backend returned them in.
    // INFO: `Option::None` sorts before `Some(_)`, hence the explicit `is_none()` first key.
    items.sort_by_key(|item| (item.score.is_none(), std::cmp::Reverse(item.score)));
    let items: Vec<_> = items
        .into_iter()
        .enumerate()
        .map(
            |(
                idx,
                InternalCompletionItem {
                    label,
                    alias,
                    description,
                    literal,
                    uri,
                    value,
                    _filter_text,
                    score,
                    import_edit,
                },
            )| {
                // NOTE: a literal has no readable-name-plus-curie split — its
                // value is all there is — so it carries no label details and a
                // client renders it on one line.
                let (label_details, data) = match &literal {
                    Some(literal) => (None, literal_data(literal, score, description.as_deref())),
                    None => (
                        Some(CompletionItemLabelDetails {
                            detail: label.clone(),
                            description: alias.clone(),
                        }),
                        entity_data(
                            &label,
                            alias.as_deref(),
                            score,
                            uri.as_deref(),
                            description.as_deref(),
                        ),
                    ),
                };
                let mut builder = CompletionItemBuilder::new()
                    .label(&value)
                    .kind(CompletionItemKind::Value)
                    // NOTE: The first 100 ID's are reserved
                    .sort_text(&format!("{:0>5}", idx + 100))
                    .text_edit(TextEdit {
                        range: range.clone(),
                        new_text: format!("{} ", value),
                    })
                    // NOTE: `label`, `labelDetails` and `sortText` carry these
                    // values as presentation only. The structured copy under
                    // `data` is what a client reads them back from, since
                    // `detail` and `documentation` are human fields the other
                    // completion kinds use for their own purposes.
                    .data(data);
                if let Some(label_details) = label_details {
                    builder = builder.label_details_full(label_details);
                }
                if let Some(description) = &description {
                    builder = builder.detail(description);
                }
                if let Some(edit) = import_edit {
                    builder = builder.additional_text_edits(vec![edit]);
                }
                // NOTE: Use the search term as filter_text for all items.
                // This gives all items the same fuzzy match score in Monaco,
                // forcing it to fall back to sortText for ordering.
                if let Some(search_term) = search_term {
                    builder = builder.filter_text(search_term);
                }
                if let Some(command) = command {
                    builder = builder.command(Command {
                        title: command.to_string(),
                        command: command.to_string(),
                        arguments: None,
                    });
                }
                builder.build()
            },
        )
        .collect();
    CompletionList {
        is_incomplete: true,
        items,
        item_defaults: None,
    }
}

/// The structured payload of a literal completion, sent as `CompletionItem.data`.
///
/// `value` is the lexical form alone: the item's own label carries the literal
/// as SPARQL writes it, quotes and tag included.
fn literal_data(literal: &LiteralFacts, score: Option<usize>, description: Option<&str>) -> LSPAny {
    let mut data = HashMap::from([
        ("kind".to_string(), LSPAny::String("literal".to_string())),
        ("value".to_string(), LSPAny::String(literal.value.clone())),
    ]);
    if let Some(language) = &literal.language {
        data.insert("language".to_string(), LSPAny::String(language.clone()));
    }
    if let Some(datatype) = &literal.datatype {
        data.insert("datatype".to_string(), LSPAny::String(datatype.clone()));
    }
    insert_description(&mut data, description);
    insert_score(&mut data, score);
    qlue_ls_data(data)
}

/// The structured payload of an entity completion, sent as `CompletionItem.data`.
///
/// Namespaced under a `qlueLs` key: `data` is opaque to the protocol and round
/// trips through `completionItem/resolve`, so the namespace keeps room for other
/// payloads and marks the blob as server specific.
fn entity_data(
    label: &str,
    alias: Option<&str>,
    score: Option<usize>,
    uri: Option<&str>,
    description: Option<&str>,
) -> LSPAny {
    let mut data = HashMap::from([
        ("kind".to_string(), LSPAny::String("entity".to_string())),
        ("label".to_string(), LSPAny::String(label.to_string())),
    ]);
    if let Some(alias) = alias {
        data.insert("alias".to_string(), LSPAny::String(alias.to_string()));
    }
    if let Some(uri) = uri {
        data.insert("uri".to_string(), LSPAny::String(uri.to_string()));
    }
    insert_description(&mut data, description);
    insert_score(&mut data, score);
    qlue_ls_data(data)
}

fn insert_description(data: &mut HashMap<String, LSPAny>, description: Option<&str>) {
    if let Some(description) = description {
        data.insert(
            "description".to_string(),
            LSPAny::String(description.to_string()),
        );
    }
}

fn insert_score(data: &mut HashMap<String, LSPAny>, score: Option<usize>) {
    if let Some(score) = score.and_then(|score| u32::try_from(score).ok()) {
        data.insert("score".to_string(), LSPAny::Uinteger(score));
    }
}

fn qlue_ls_data(data: HashMap<String, LSPAny>) -> LSPAny {
    LSPAny::LSPObject(HashMap::from([(
        "qlueLs".to_string(),
        LSPAny::LSPObject(data),
    )]))
}

#[cfg(test)]
mod test {
    use ll_sparql_parser::{
        ast::{AstNode, QueryUnit},
        parse_query,
    };

    use super::{matches_search_term, reduce_path};

    #[test]
    fn matches_search_term_exact_match() {
        assert!(matches_search_term("FILTER", Some("FILTER")));
    }

    #[test]
    fn matches_search_term_prefix_match() {
        assert!(matches_search_term("FILTER", Some("FI")));
        assert!(matches_search_term("FILTER", Some("F")));
        assert!(matches_search_term("OPTIONAL", Some("OP")));
        assert!(matches_search_term("GROUP BY", Some("GR")));
    }

    #[test]
    fn matches_search_term_case_insensitive() {
        assert!(matches_search_term("FILTER", Some("fi")));
        assert!(matches_search_term("FILTER", Some("filter")));
        assert!(matches_search_term("FILTER", Some("Filter")));
        assert!(matches_search_term("OPTIONAL", Some("opt")));
        assert!(matches_search_term("GROUP BY", Some("group")));
    }

    #[test]
    fn matches_search_term_no_match() {
        assert!(!matches_search_term("FILTER", Some("Germany")));
        assert!(!matches_search_term("FILTER", Some("BI")));
        assert!(!matches_search_term("OPTIONAL", Some("FI")));
        assert!(!matches_search_term("BIND", Some("FILTER")));
    }

    #[test]
    fn matches_search_term_none_shows_all() {
        assert!(matches_search_term("FILTER", None));
        assert!(matches_search_term("BIND", None));
        assert!(matches_search_term("OPTIONAL", None));
    }

    #[test]
    fn matches_search_term_empty_string_shows_all() {
        assert!(matches_search_term("FILTER", Some("")));
        assert!(matches_search_term("BIND", Some("")));
        assert!(matches_search_term("OPTIONAL", Some("")));
    }

    #[test]
    fn matches_search_term_partial_word_not_prefix() {
        // "ILTER" is not a prefix of "FILTER"
        assert!(!matches_search_term("FILTER", Some("ILTER")));
        // "TER" is not a prefix of "FILTER"
        assert!(!matches_search_term("FILTER", Some("TER")));
    }

    /// Reduce the verb of the (only) triple in `query` at `offset`.
    fn reduce_verb_of_first_triple(query: &str, offset: u32) -> Option<String> {
        let (tree, _) = parse_query(query);
        let query_unit = QueryUnit::cast(tree).unwrap();
        let triples = query_unit
            .select_query()
            .unwrap()
            .where_clause()
            .unwrap()
            .group_graph_pattern()
            .unwrap()
            .triple_blocks()
            .first()
            .unwrap()
            .triples();
        let triple = triples.first().unwrap();
        reduce_path(
            &triple.subject().unwrap().text(),
            Some(
                &triple
                    .properties_list_path()
                    .unwrap()
                    .properties()
                    .last()
                    .unwrap()
                    .verb,
            ),
            "[]",
            offset.into(),
        )
    }

    #[test]
    fn reduce_unknown_path_kind_returns_none() {
        // NOTE: a variable predicate is a VerbSimple, which is castable to `Path`
        // but is not a path -> no local context. This used to panic.
        //                                      0123456789012345678
        assert_eq!(
            reduce_verb_of_first_triple("Select * { ?a ?p ?b }", 16),
            None
        );
    }

    #[test]
    fn reduce_sequence_path() {
        //       0123456789012345678901
        let s = "Select * { ?a <p0>/  }";
        let reduced = "?a <p0> ?qls_inner . ?qls_inner ?qls_entity []";
        let offset = 19;
        let (tree, _) = parse_query(s);
        let query_unit = QueryUnit::cast(tree).unwrap();
        let triples = query_unit
            .select_query()
            .unwrap()
            .where_clause()
            .unwrap()
            .group_graph_pattern()
            .unwrap()
            .triple_blocks()
            .first()
            .unwrap()
            .triples();
        let triple = triples.first().unwrap();
        let res = reduce_path(
            &triple.subject().unwrap().text(),
            Some(
                &triple
                    .properties_list_path()
                    .unwrap()
                    .properties()
                    .last()
                    .unwrap()
                    .verb,
            ),
            "[]",
            offset.into(),
        )
        .unwrap();
        assert_eq!(res, reduced);
    }

    #[test]
    fn reduce_alternating_path() {
        //       012345678901234567890123456
        let s = "Select * { ?a <p0>/<p1>|  <x>}";
        let reduced = "?a ?qls_entity []";
        let offset = 24;
        let (tree, _) = parse_query(s);
        let query_unit = QueryUnit::cast(tree).unwrap();
        let triples = query_unit
            .select_query()
            .unwrap()
            .where_clause()
            .unwrap()
            .group_graph_pattern()
            .unwrap()
            .triple_blocks()
            .first()
            .unwrap()
            .triples();
        let triple = triples.first().unwrap();
        let res = reduce_path(
            &triple.subject().unwrap().text(),
            Some(
                &triple
                    .properties_list_path()
                    .unwrap()
                    .properties()
                    .last()
                    .unwrap()
                    .verb,
            ),
            "[]",
            offset.into(),
        )
        .unwrap();
        assert_eq!(res, reduced);
    }

    #[test]
    fn reduce_inverse_path() {
        //       012345678901234567890123456
        let s = "Select * { ?a ^";
        let reduced = "[] ?qls_entity ?a";
        let offset = 15;
        let (tree, _) = parse_query(s);
        let query_unit = QueryUnit::cast(tree).unwrap();
        let triples = query_unit
            .select_query()
            .unwrap()
            .where_clause()
            .unwrap()
            .group_graph_pattern()
            .unwrap()
            .triple_blocks()
            .first()
            .unwrap()
            .triples();
        let triple = triples.first().unwrap();
        let res = reduce_path(
            &triple.subject().unwrap().text(),
            Some(
                &triple
                    .properties_list_path()
                    .unwrap()
                    .properties()
                    .last()
                    .unwrap()
                    .verb,
            ),
            "[]",
            offset.into(),
        )
        .unwrap();
        pretty_assertions::assert_eq!(res, reduced);
    }

    #[test]
    fn reduce_negated_path() {
        //       012345678901234567890123456
        let s = "Select * { ?a !(";
        let reduced = "?a ?qls_entity []";
        let offset: u32 = 16;
        let (tree, _) = parse_query(&s[..offset as usize]);
        let query_unit = QueryUnit::cast(tree).unwrap();
        let triples = query_unit
            .select_query()
            .unwrap()
            .where_clause()
            .unwrap()
            .group_graph_pattern()
            .unwrap()
            .triple_blocks()
            .first()
            .unwrap()
            .triples();
        let triple = triples.first().unwrap();
        let res = reduce_path(
            &triple.subject().unwrap().text(),
            Some(
                &triple
                    .properties_list_path()
                    .unwrap()
                    .properties()
                    .last()
                    .unwrap()
                    .verb,
            ),
            "[]",
            offset.into(),
        )
        .unwrap();
        assert_eq!(res, reduced);
    }

    #[test]
    fn reduce_complex_path1() {
        //       0123456789012345678901234567890123456
        let s = "Select * { ?a <p0>|<p1>/(<p2>)/^  <x>}";
        let reduced = "?a <p1>/(<p2>) ?qls_inner . [] ?qls_entity ?qls_inner";
        let offset = 32;
        let (tree, _) = parse_query(s);
        let query_unit = QueryUnit::cast(tree).unwrap();
        let triples = query_unit
            .select_query()
            .unwrap()
            .where_clause()
            .unwrap()
            .group_graph_pattern()
            .unwrap()
            .triple_blocks()
            .first()
            .unwrap()
            .triples();
        let triple = triples.first().unwrap();
        let res = reduce_path(
            &triple.subject().unwrap().text(),
            Some(
                &triple
                    .properties_list_path()
                    .unwrap()
                    .properties()
                    .last()
                    .unwrap()
                    .verb,
            ),
            "[]",
            offset.into(),
        )
        .unwrap();
        assert_eq!(res, reduced);
    }
    #[test]
    fn reduce_complex_path2() {
        //       0         1         2         3         4
        //       01234567890123456789012345678901234567890
        let s = "Select * { ?a <p0>|<p1>/(<p2>)/^<p2>/!(^";
        let reduced = "?a <p1>/(<p2>)/^<p2> ?qls_inner . [] ?qls_entity ?qls_inner";
        let offset = 40;
        let (tree, _) = parse_query(s);
        let query_unit = QueryUnit::cast(tree).unwrap();
        let triples = query_unit
            .select_query()
            .unwrap()
            .where_clause()
            .unwrap()
            .group_graph_pattern()
            .unwrap()
            .triple_blocks()
            .first()
            .unwrap()
            .triples();
        let triple = triples.first().unwrap();
        let res = reduce_path(
            &triple.subject().unwrap().text(),
            Some(
                &triple
                    .properties_list_path()
                    .unwrap()
                    .properties()
                    .last()
                    .unwrap()
                    .verb,
            ),
            "[]",
            offset.into(),
        )
        .unwrap();
        assert_eq!(res, reduced);
    }

    #[test]
    fn reduce_complex_path3() {
        //       0         1         2
        //       0123456789012345678901
        let s = "Select * { ?a ^(^<a>/";
        let reduced = "[] ^<a> ?qls_inner . ?qls_inner ?qls_entity ?a";
        let offset = 21;
        let (tree, _) = parse_query(s);
        let query_unit = QueryUnit::cast(tree).unwrap();
        let triples = query_unit
            .select_query()
            .unwrap()
            .where_clause()
            .unwrap()
            .group_graph_pattern()
            .unwrap()
            .triple_blocks()
            .first()
            .unwrap()
            .triples();
        let triple = triples.first().unwrap();
        let res = reduce_path(
            &triple.subject().unwrap().text(),
            Some(
                &triple
                    .properties_list_path()
                    .unwrap()
                    .properties()
                    .last()
                    .unwrap()
                    .verb,
            ),
            "[]",
            offset.into(),
        )
        .unwrap();
        assert_eq!(res, reduced);
    }

    #[test]
    fn reduce_complex_path4() {
        //       01234567890123456
        let s = "Select * { ?a !^";
        let reduced = "[] ?qls_entity ?a";
        let offset = 16;

        let (tree, _) = parse_query(s);
        let query_unit = QueryUnit::cast(tree).unwrap();
        let triples = query_unit
            .select_query()
            .unwrap()
            .where_clause()
            .unwrap()
            .group_graph_pattern()
            .unwrap()
            .triple_blocks()
            .first()
            .unwrap()
            .triples();
        let triple = triples.first().unwrap();
        let res = reduce_path(
            &triple.subject().unwrap().text(),
            Some(
                &triple
                    .properties_list_path()
                    .unwrap()
                    .properties()
                    .last()
                    .unwrap()
                    .verb,
            ),
            "[]",
            offset.into(),
        )
        .unwrap();
        assert_eq!(res, reduced);
    }
}
