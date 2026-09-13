use super::{
    super::environment::{CompletionEnvironment, CompletionLocation},
    super::error::CompletionError,
    super::utils::{CompletionTemplate, dispatch_completion_query},
};
use crate::server::{Server, lsp::CompletionList};
use futures::lock::Mutex;
use ll_sparql_parser::ast::AstNode;
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
    let property_list = blank_node_props.property_list()?;
    let anchor_range = environment.anchor_token.as_ref()?.text_range();
    // NOTE: the anchor token has to be inside the property list, otherwise there is no
    //       meaningful local context to build.
    if !property_list
        .syntax()
        .text_range()
        .contains_range(anchor_range)
    {
        return None;
    }
    // INFO: clip the property list at the anchor token, everything after the cursor is not
    //       part of the context.
    let clipped_text = property_list.text_until(anchor_range.end());
    Some(format!("[] {} ?qls_entity", clipped_text))
}
