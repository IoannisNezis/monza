use crate::SyntaxKind::{self, *};

pub use rowan::Direction;

#[allow(dead_code)]
pub type SyntaxNode = rowan::SyntaxNode<Sparql>;
#[allow(dead_code)]
pub type SyntaxToken = rowan::SyntaxToken<Sparql>;
#[allow(dead_code)]
pub type SyntaxElement = rowan::SyntaxElement<Sparql>;
#[allow(dead_code)]
pub type SyntaxNodeChildren = rowan::SyntaxNodeChildren<Sparql>;
#[allow(dead_code)]
pub type SyntaxElementChildren = rowan::SyntaxElementChildren<Sparql>;
#[allow(dead_code)]
pub type PreorderWithTokens = rowan::api::PreorderWithTokens<Sparql>;
#[allow(dead_code)]
pub type TokenAtOffset = rowan::TokenAtOffset<SyntaxToken>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Sparql {}
impl rowan::Language for Sparql {
    type Kind = SyntaxKind;
    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        assert!(raw.0 <= PrefixedName as u16);
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }
    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

pub fn print_full_tree(syntax_node: &SyntaxNode, indent: usize) -> std::string::String {
    let mut s = std::string::String::new();
    s += &format!("{}{:?}\n", "    ".repeat(indent), syntax_node,);
    syntax_node
        .children_with_tokens()
        .for_each(|child| match child {
            rowan::NodeOrToken::Node(node) => s += &print_full_tree(&node, indent + 1),
            rowan::NodeOrToken::Token(token) => {
                s += &format!("{}{:?}\n", "    ".repeat(indent + 1), token);
            }
        });
    return s;
}
