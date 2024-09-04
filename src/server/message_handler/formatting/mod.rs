mod core;
mod utils;
use core::*;
use log::{error, info};

use tree_sitter_c2rust::Parser;
use wasm_bindgen::prelude::wasm_bindgen;

use crate::{
    lsp::{FormattingRequest, FormattingResponse},
    server::ServerState,
};

pub fn handle_format_request(
    request: FormattingRequest,
    state: &mut ServerState,
) -> FormattingResponse {
    let uri = request.get_document_uri();
    info!("Received formatting request for: {}", uri);
    match state.analysis_state.get_state(uri) {
        Some((document, Some(tree))) => {
            let options = request.get_options();
            let text_edits = format_textdoument(document, tree, options);
            FormattingResponse::new(request.get_id(), text_edits)
        }
        _ => {
            error!("Requested formatting for unknown document: {}", uri);
            todo!()
        }
    }
}

#[wasm_bindgen]
pub fn format_raw(text: String) -> String {
    let mut parser = Parser::new();
    match parser.set_language(&tree_sitter_sparql::language()) {
        Ok(()) => {
            let tree = parser.parse(text.clone(), None).expect("could not parse");
            let formatted_text = format_helper(&text, &mut tree.walk(), 0, "  ", "");
            return formatted_text;
        }
        Err(_) => panic!("Could not setup parser"),
    }
}

#[cfg(test)]
mod tests;
