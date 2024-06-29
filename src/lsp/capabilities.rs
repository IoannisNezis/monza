use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    text_document_sync: TextDocumentSyncKind,
    hover_provider: bool,
    completion_provider: CompletionOptions,
}

impl ServerCapabilities {
    pub fn new() -> Self {
        Self {
            text_document_sync: TextDocumentSyncKind::Full,
            hover_provider: true,
            completion_provider: CompletionOptions {
                trigger_characters: vec!["?".to_string()],
            },
        }
    }
}

#[derive(Debug, Serialize_repr, Deserialize_repr, PartialEq)]
#[repr(u8)]
pub enum TextDocumentSyncKind {
    None = 0,
    Full = 1,
    Incremental = 2,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct CompletionOptions {
    // WARNING: This is not to spec, there are multiple optional options:
    // https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#completionOptions
    trigger_characters: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::ServerCapabilities;

    #[test]
    fn test_serialization() {
        let server_capabilities = ServerCapabilities::new();

        let serialized = serde_json::to_string(&server_capabilities).unwrap();

        assert_eq!(
            serialized,
            "{\"textDocumentSync\":1,\"hoverProvider\":true,\"completionProvider\":{}}"
        );
    }
}
