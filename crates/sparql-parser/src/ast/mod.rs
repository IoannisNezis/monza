mod utils;
use crate::{syntax_kind::SyntaxKind, Sparql, SyntaxNode};
use rowan::{cursor::SyntaxToken, SyntaxNodeChildren, TextRange, TextSize};
use std::usize;
use utils::nth_ancestor;

#[derive(Debug, PartialEq)]
pub struct QueryUnit {
    syntax: SyntaxNode,
}

impl QueryUnit {
    pub fn select_query(&self) -> Option<SelectQuery> {
        SelectQuery::cast(self.syntax.first_child()?.first_child()?)
    }

    pub fn prologue(&self) -> Option<Prologue> {
        Prologue::cast(self.syntax.first_child()?.first_child()?)
    }

    /// The first child of the unit that is not the `Prologue`.
    ///
    /// NOTE: The `Prologue` is a child of the inner `Query`/`Update` node, not
    ///       of the unit itself, hence the `first_child()` hop.
    pub fn strip_prologue(&self) -> Option<SyntaxNode> {
        self.syntax
            .first_child()?
            .children()
            .find(|node| node.kind() != SyntaxKind::Prologue)
    }
}

#[derive(Debug, PartialEq)]
pub struct UpdateUnit {
    syntax: SyntaxNode,
}

impl UpdateUnit {
    /// The `Prologue` of the first `Update` in the unit.
    ///
    /// NOTE: Chained updates (`... ; ...`) nest an `Update` inside an `Update`,
    ///       each carrying its own `Prologue`. This returns the outermost one.
    pub fn prologue(&self) -> Option<Prologue> {
        Prologue::cast(self.syntax.first_child()?.first_child()?)
    }

    /// The first child of the unit that is not the `Prologue`.
    ///
    /// NOTE: The `Prologue` is a child of the inner `Query`/`Update` node, not
    ///       of the unit itself, hence the `first_child()` hop.
    pub fn strip_prologue(&self) -> Option<SyntaxNode> {
        self.syntax
            .first_child()?
            .children()
            .find(|node| node.kind() != SyntaxKind::Prologue)
    }

    /// All `UpdateOne` nodes of the unit, in order.
    pub fn update_ones(&self) -> Vec<SyntaxNode> {
        self.preorder_find_kind(SyntaxKind::UpdateOne)
    }
}

/// A parsed top level unit: either a query or an update.
#[derive(Debug, PartialEq)]
pub enum Unit {
    Query(QueryUnit),
    Update(UpdateUnit),
}

impl Unit {
    pub fn prologue(&self) -> Option<Prologue> {
        match self {
            Unit::Query(query_unit) => query_unit.prologue(),
            Unit::Update(update_unit) => update_unit.prologue(),
        }
    }

    pub fn as_query(&self) -> Option<&QueryUnit> {
        match self {
            Unit::Query(query_unit) => Some(query_unit),
            Unit::Update(_) => None,
        }
    }

    pub fn as_update(&self) -> Option<&UpdateUnit> {
        match self {
            Unit::Update(update_unit) => Some(update_unit),
            Unit::Query(_) => None,
        }
    }

    pub fn strip_prologue(&self) -> Option<SyntaxNode> {
        match self {
            Unit::Query(query_unit) => query_unit.strip_prologue(),
            Unit::Update(update_unit) => update_unit.strip_prologue(),
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Prologue {
    syntax: SyntaxNode,
}

impl Prologue {
    pub fn prefix_declarations(&self) -> Vec<PrefixDeclaration> {
        self.syntax
            .children()
            .filter_map(&PrefixDeclaration::cast)
            .collect()
    }
}

#[derive(Debug, PartialEq)]
pub struct SolutionModifier {
    syntax: SyntaxNode,
}

impl SolutionModifier {
    pub fn group_clause(&self) -> Option<GroupClause> {
        GroupClause::cast(self.syntax.first_child()?)
    }

    pub fn select_query(&self) -> Option<SelectQuery> {
        self.syntax.parent().and_then(SelectQuery::cast)
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct GroupClause {
    syntax: SyntaxNode,
}

impl GroupClause {
    pub fn select_query(&self) -> Option<SelectQuery> {
        self.syntax
            .parent()
            .and_then(|p| p.parent())
            .and_then(SelectQuery::cast)
    }
}

#[derive(Debug, PartialEq)]
pub struct PrefixDeclaration {
    syntax: SyntaxNode,
}

impl PrefixDeclaration {
    pub fn prefix(&self) -> Option<String> {
        Some(
            self.syntax
                .children_with_tokens()
                .find(|element| element.kind() == SyntaxKind::PNAME_NS)?
                .to_string()
                .split_once(":")
                .expect("Every PNAME_NS should contain ':' at the end")
                .0
                .to_string(),
        )
    }

    pub fn raw_uri_prefix(&self) -> Option<String> {
        let s = self
            .syntax
            .children_with_tokens()
            .find(|element| element.kind() == SyntaxKind::IRIREF)?
            .to_string();
        (s.len() >= 2).then_some(s[1..(s.len() - 1)].to_string())
    }

    pub fn uri_prefix(&self) -> Option<String> {
        Some(
            self.syntax
                .children_with_tokens()
                .find(|element| element.kind() == SyntaxKind::IRIREF)?
                .to_string(),
        )
    }
}

#[derive(Debug, PartialEq)]
pub struct SelectQuery {
    syntax: SyntaxNode,
}

impl SelectQuery {
    pub fn where_clause(&self) -> Option<WhereClause> {
        WhereClause::cast(
            self.syntax
                .children()
                .find(|child| WhereClause::can_cast(child.kind()))?,
        )
    }
    pub fn select_clause(&self) -> Option<SelectClause> {
        SelectClause::cast(
            self.syntax
                .children()
                .find(|child| SelectClause::can_cast(child.kind()))?,
        )
    }
    pub fn variables(&self) -> Vec<Var> {
        if let Some(where_clause) = self.where_clause() {
            if let Some(ggp) = where_clause.group_graph_pattern() {
                return ggp
                    .triple_blocks()
                    .iter()
                    .flat_map(|triple_block| {
                        triple_block
                            .triples()
                            .iter()
                            .flat_map(|triple| triple.visible_variables())
                            .collect::<Vec<Var>>()
                    })
                    .collect();
            }
        }
        vec![]
    }

    pub fn soulution_modifier(&self) -> Option<SolutionModifier> {
        SolutionModifier::cast(self.syntax.last_child()?)
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct SelectClause {
    syntax: SyntaxNode,
}

impl SelectClause {
    /// All selected variables.
    /// This excludes variables used in assignments
    ///
    /// **Example**
    /// ```sparql
    /// Select ?a (?b as ?c) {
    ///     ...
    /// }
    /// ```
    ///
    /// here `variables` would return just **?a** and not **?b** or **?c**.
    pub fn variables(&self) -> Vec<Var> {
        self.syntax
            .children()
            .into_iter()
            .filter_map(Var::cast)
            .filter(|var| {
                var.syntax()
                    .prev_sibling()
                    .is_none_or(|var| var.kind() != SyntaxKind::Expression)
            })
            .collect()
    }

    /// All selected variables plus all assigned variables.
    /// This excludes variables used in assignments
    ///
    /// **Example**
    /// ```sparql
    /// Select ?a (?b as ?c) {
    ///     ...
    /// }
    /// ```
    /// result: [**?a**, **?c**]
    pub fn projected_variables(&self) -> Vec<Var> {
        self.syntax.children().filter_map(Var::cast).collect()
    }

    pub fn is_star_selection(&self) -> bool {
        self.syntax()
            .last_token()
            .is_some_and(|last| last.kind() == SyntaxKind::Star)
    }

    /// All assignments in the select clause.
    pub fn assignments(&self) -> Vec<Assignment> {
        self.syntax
            .children()
            .filter_map(Var::cast)
            .filter_map(|variable| {
                variable
                    .syntax()
                    .prev_sibling()
                    .and_then(Expression::cast)
                    .map(|expression| Assignment {
                        expression,
                        variable,
                    })
            })
            .collect()
    }

    pub fn select_query(&self) -> Option<SelectQuery> {
        SelectQuery::cast(self.syntax.parent()?)
    }
}

#[derive(Debug)]
pub struct Assignment {
    pub expression: Expression,
    pub variable: Var,
}

#[derive(Debug)]
pub struct Expression {
    syntax: SyntaxNode,
}

impl Expression {
    /// Returns the variable, if this expression consists of just a single variable.
    pub fn as_var(&self) -> Option<Var> {
        // NOTE: A plain variable is wrapped in a chain of single-child expression nodes.
        let mut node = self.syntax.clone();
        while !Var::can_cast(node.kind()) {
            let mut children = node.children_with_tokens().filter(|child| {
                !matches!(child.kind(), SyntaxKind::WHITESPACE | SyntaxKind::Comment)
            });
            let only_child = children.next()?;
            if children.next().is_some() {
                return None;
            }
            node = only_child.into_node()?;
        }
        Var::cast(node)
    }

    pub fn unaggregated_variables(&self) -> Vec<Var> {
        let mut res = vec![];
        let mut stack = vec![self.syntax.clone()];
        while let Some(node) = stack.pop() {
            if node.kind() != SyntaxKind::Aggregate {
                stack.extend(node.children());
            }
            if let Some(var) = Var::cast(node) {
                res.push(var);
            }
        }
        res
    }
}

#[derive(Debug)]
pub enum GraphPatternNotTriples {
    GroupOrUnionGraphPattern(GroupOrUnionGraphPattern),
    OptionalGraphPattern(OptionalGraphPattern),
    MinusGraphPattern(MinusGraphPattern),
    GraphGraphPattern(GraphGraphPattern),
    ServiceGraphPattern(ServiceGraphPattern),
    Filter(Filter),
    Bind(Bind),
    InlineData(InlineData),
}

impl GraphPatternNotTriples {
    pub fn group_graph_pattern(&self) -> Option<GraphGraphPattern> {
        match self {
            GraphPatternNotTriples::GroupOrUnionGraphPattern(_group_or_union_graph_pattern) => {
                todo!()
            }
            GraphPatternNotTriples::OptionalGraphPattern(_optional_graph_pattern) => todo!(),
            GraphPatternNotTriples::MinusGraphPattern(_minus_graph_pattern) => todo!(),
            GraphPatternNotTriples::GraphGraphPattern(_graph_graph_pattern) => todo!(),
            GraphPatternNotTriples::ServiceGraphPattern(_service_graph_pattern) => todo!(),
            GraphPatternNotTriples::Filter(_filter) => None,
            GraphPatternNotTriples::Bind(_bind) => None,
            GraphPatternNotTriples::InlineData(_inline_data) => None,
        }
    }
}

#[derive(Debug)]
pub struct GroupOrUnionGraphPattern {
    syntax: SyntaxNode,
}
impl GroupOrUnionGraphPattern {
    fn group_graph_patterns(&self) -> Vec<GroupGraphPattern> {
        self.syntax
            .children()
            .filter_map(GroupGraphPattern::cast)
            .collect()
    }
}

#[derive(Debug)]
pub struct OptionalGraphPattern {
    syntax: SyntaxNode,
}
impl OptionalGraphPattern {
    fn group_graph_pattern(&self) -> Option<GroupGraphPattern> {
        self.syntax.last_child().and_then(GroupGraphPattern::cast)
    }
}

#[derive(Debug)]
pub struct MinusGraphPattern {
    syntax: SyntaxNode,
}
impl MinusGraphPattern {
    fn group_graph_pattern(&self) -> Option<GroupGraphPattern> {
        self.syntax.last_child().and_then(GroupGraphPattern::cast)
    }
}

#[derive(Debug)]
pub struct GraphGraphPattern {
    syntax: SyntaxNode,
}
impl GraphGraphPattern {
    fn group_graph_pattern(&self) -> Option<GroupGraphPattern> {
        self.syntax.last_child().and_then(GroupGraphPattern::cast)
    }
}

#[derive(Debug)]
pub struct Filter {
    syntax: SyntaxNode,
}

#[derive(Debug)]
pub struct Bind {
    syntax: SyntaxNode,
}

#[derive(Debug, PartialEq, Clone)]
pub struct InlineData {
    syntax: SyntaxNode,
}

#[derive(Debug)]
pub struct WhereClause {
    syntax: SyntaxNode,
}

#[derive(Debug)]
pub struct ServiceGraphPattern {
    syntax: SyntaxNode,
}

impl ServiceGraphPattern {
    pub fn iri(&self) -> Option<Iri> {
        self.syntax
            .children()
            .find(|child| child.kind() == SyntaxKind::VarOrIri)
            .and_then(|child| child.first_child().and_then(Iri::cast))
    }

    pub fn group_graph_pattern(&self) -> Option<GroupGraphPattern> {
        self.syntax
            .children()
            .last()
            .and_then(GroupGraphPattern::cast)
    }
}

impl WhereClause {
    pub fn group_graph_pattern(&self) -> Option<GroupGraphPattern> {
        GroupGraphPattern::cast(self.syntax.first_child()?)
    }

    pub fn where_token(&self) -> Option<SyntaxToken> {
        match self.syntax.first_child_or_token() {
            Some(rowan::NodeOrToken::Token(token)) if token.kind() == SyntaxKind::WHERE => {
                Some(token.into())
            }
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct GroupGraphPattern {
    syntax: SyntaxNode,
}

impl GroupGraphPattern {
    pub fn triple_blocks(&self) -> Vec<TriplesBlock> {
        self.syntax()
            .children()
            .find(|child| child.kind() == SyntaxKind::GroupGraphPatternSub)
            .map(|ggp| ggp.children().filter_map(TriplesBlock::cast).collect())
            .unwrap_or_default()
    }

    pub fn group_pattern_not_triples(&self) -> Vec<GraphPatternNotTriples> {
        self.syntax()
            .children()
            .find(|child| child.kind() == SyntaxKind::GroupGraphPatternSub)
            .map(|ggp| {
                ggp.children()
                    .filter_map(GraphPatternNotTriples::cast)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn r_paren_token(&self) -> Option<SyntaxToken> {
        match self.syntax.last_child_or_token() {
            Some(rowan::NodeOrToken::Token(token)) if token.kind() == SyntaxKind::RCurly => {
                Some(token.into())
            }
            _ => None,
        }
    }

    pub fn l_paren_token(&self) -> Option<SyntaxToken> {
        match self.syntax.first_child_or_token() {
            Some(rowan::NodeOrToken::Token(token)) if token.kind() == SyntaxKind::LCurly => {
                Some(token.into())
            }
            _ => None,
        }
    }

    pub fn sub_select(&self) -> Option<SelectQuery> {
        self.syntax().first_child().and_then(SelectQuery::cast)
    }
}

#[derive(Debug)]
pub struct TriplesBlock {
    syntax: SyntaxNode,
}

impl TriplesBlock {
    /// Get the `Triple`'s contained in this `TriplesBlock`.
    pub fn triples(&self) -> Vec<Triple> {
        self.syntax
            .children()
            .filter_map(|child| match child.kind() {
                SyntaxKind::TriplesSameSubjectPath => Some(vec![Triple::cast(child).unwrap()]),
                SyntaxKind::TriplesBlock => Some(TriplesBlock::cast(child).unwrap().triples()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    pub fn group_graph_pattern(&self) -> Option<GroupGraphPattern> {
        GroupGraphPattern::cast(nth_ancestor(self.syntax.clone(), 2)?)
    }

    /// Get the Dot token that terminates the first triple in this block.
    /// Grammar: TriplesBlock = TriplesSameSubjectPath ( '.' TriplesBlock? )?
    pub fn trailing_dot(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .find_map(|child| match child {
                rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::Dot => {
                    Some(token.into())
                }
                _ => None,
            })
    }
}

#[derive(Debug, PartialEq)]
pub struct Subject {
    syntax: SyntaxNode,
}

#[derive(Debug, PartialEq, Clone)]
pub struct Triple {
    syntax: SyntaxNode,
}

impl Triple {
    pub fn subject(&self) -> Option<Subject> {
        self.syntax.first_child().and_then(Subject::cast)
    }

    pub fn properties_list_path(&self) -> Option<PropertyListPath> {
        PropertyListPath::cast(self.syntax.children().nth(1)?)
    }

    /// Get the outer `TriplesBlock` this Triple is part of.
    /// NOTE: TriplesBlock is defined recursivly
    pub fn triples_block(&self) -> Option<TriplesBlock> {
        let mut parent = self.syntax.parent()?;
        if parent.kind() != SyntaxKind::TriplesBlock {
            return None;
        }
        while let Some(node) = parent.parent() {
            if node.kind() == SyntaxKind::TriplesBlock {
                parent = node;
            } else {
                break;
            }
        }
        Some(TriplesBlock::cast(parent).expect("parent should be a TriplesBlock"))
    }
}

#[derive(Debug)]
pub struct PropertyPath {
    pub verb: Path,
    // NOTE: `None` when the input is truncated after the verb (e.g. while typing)
    pub object: Option<ObjectList>,
}

impl PropertyPath {
    pub fn text(&self) -> String {
        match &self.object {
            Some(object) => format!("{} {}", self.verb.text(), object.text()),
            None => self.verb.text(),
        }
    }

    pub fn text_range(&self) -> TextRange {
        TextRange::new(
            self.verb.syntax().text_range().start(),
            self.object
                .as_ref()
                .map(|object| object.syntax.text_range().end())
                .unwrap_or_else(|| self.verb.syntax().text_range().end()),
        )
    }
}

#[derive(Debug)]
pub struct Path {
    syntax: SyntaxNode,
}

impl Path {
    pub fn sub_paths(&self) -> SubPaths {
        SubPaths {
            children: self.syntax.children(),
        }
    }
}

pub struct SubPaths {
    children: SyntaxNodeChildren<Sparql>,
}

impl Iterator for SubPaths {
    type Item = Path;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(next_child) = self.children.next() {
            if let Some(path) = Path::cast(next_child.into()) {
                return Some(path);
            }
        }
        None
    }
}

#[derive(Debug)]
pub struct ObjectList {
    syntax: SyntaxNode,
}

#[derive(Debug, PartialEq, Clone)]
pub struct BlankPropertyList {
    syntax: SyntaxNode,
}

impl BlankPropertyList {
    pub fn triple(&self) -> Option<Triple> {
        match self.syntax.kind() {
            SyntaxKind::BlankNodePropertyListPath => {
                todo!()
            }
            SyntaxKind::BlankNodePropertyList => {
                todo!()
            }
            SyntaxKind::BlankNode => self.syntax.ancestors().nth(7).and_then(Triple::cast),
            _ => None,
        }
    }

    pub fn is_object(&self) -> bool {
        todo!()
    }

    pub fn is_subject(&self) -> bool {
        todo!()
    }

    pub fn property_list(&self) -> Option<PropertyListPath> {
        match self.syntax.kind() {
            SyntaxKind::BlankNodePropertyListPath | SyntaxKind::BlankNodePropertyList => {
                PropertyListPath::cast(self.syntax.first_child()?)
            }

            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct PropertyListPath {
    syntax: SyntaxNode,
}

impl PropertyListPath {
    pub fn properties(&self) -> Vec<PropertyPath> {
        self.syntax
            .children()
            .step_by(2)
            .filter_map(|child| {
                Path::cast(child.clone()).map(|path| PropertyPath {
                    verb: path,
                    object: child.next_sibling().and_then(ObjectList::cast),
                })
            })
            .collect()
    }
    pub fn variables(&self) -> Vec<Var> {
        self.syntax
            .children()
            .filter_map(|child| match child.kind() {
                SyntaxKind::VerbSimple => child.first_child().and_then(Var::cast),
                _ => None,
            })
            .collect()
    }
}

#[derive(Debug)]
pub struct Iri {
    syntax: SyntaxNode,
}

impl Iri {
    pub fn prefixed_name(&self) -> Option<PrefixedName> {
        self.syntax.first_child().and_then(PrefixedName::cast)
    }

    /// Converts a IRIREF "<abc>" into the raw string "abc"
    /// Returns None this iri is a PrefixedName
    pub fn raw_iri(&self) -> Option<String> {
        (self.syntax.first_child_or_token()?.kind() == SyntaxKind::IRIREF
            && self.syntax.text_range().len() >= 2.into())
        .then(|| self.text()[1..usize::from(self.syntax.text_range().len()) - 1].to_string())
    }

    pub fn is_uncompressed(&self) -> bool {
        self.syntax
            .first_child_or_token()
            .is_some_and(|child| child.kind() == SyntaxKind::IRIREF)
    }
}

#[derive(Debug)]
pub struct PrefixedName {
    syntax: SyntaxNode,
}

impl PrefixedName {
    pub fn prefix(&self) -> String {
        self.syntax
            .to_string()
            .split_once(":")
            .expect("Every PrefixedName should contain a ':'")
            .0
            .to_string()
    }

    pub fn name(&self) -> String {
        self.syntax
            .to_string()
            .split_once(":")
            .expect("Every PrefixedName should contain a ':'")
            .1
            .to_string()
    }
}

#[derive(Debug)]
pub struct VarOrTerm {
    syntax: SyntaxNode,
}

impl VarOrTerm {
    pub fn var(&self) -> Option<Var> {
        Var::cast(self.syntax.first_child()?)
    }

    pub fn is_var(&self) -> bool {
        self.syntax
            .first_child()
            .map_or(false, |child| child.kind() == SyntaxKind::Var)
    }

    pub fn is_term(&self) -> bool {
        !self.is_var()
    }
}

#[derive(Debug)]
pub struct Var {
    syntax: SyntaxNode,
}

impl Var {
    pub fn triple(&self) -> Option<Triple> {
        self.syntax.ancestors().find_map(Triple::cast)
    }

    /// Variable name without `?`
    ///
    /// ---
    ///
    /// `?subject` -> `subject`
    pub fn var_name(&self) -> String {
        let text = self.syntax.text().to_string();
        if !text.is_empty() && text.starts_with('?') {
            return text[1..].to_string();
        }
        return text;
    }
}

impl AstNode for Var {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::Var
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }
    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        vec![Var::cast(self.syntax().clone()).unwrap()]
    }
}

impl AstNode for VarOrTerm {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::VarOrTerm
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }
    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.syntax
            .first_child()
            .and_then(Var::cast)
            .map(|var| vec![var])
            .unwrap_or_default()
    }
}

impl AstNode for Iri {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::iri
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }
    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        vec![]
    }
}

impl AstNode for PrefixedName {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::PrefixedName
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }
    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        vec![]
    }
}

impl AstNode for Path {
    fn kind() -> SyntaxKind {
        SyntaxKind::VerbPath
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::Path
                | SyntaxKind::VerbPath
                | SyntaxKind::VerbSimple
                | SyntaxKind::PathAlternative
                | SyntaxKind::PathSequence
                | SyntaxKind::PathElt
                | SyntaxKind::PathEltOrInverse
                | SyntaxKind::PathPrimary
                | SyntaxKind::PathNegatedPropertySet
                | SyntaxKind::PathOneInPropertySet
        )
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        vec![]
    }
}

impl AstNode for ObjectList {
    fn kind() -> SyntaxKind {
        SyntaxKind::ObjectListPath
    }
    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(kind, SyntaxKind::ObjectListPath | SyntaxKind::ObjectList)
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.syntax.descendants().filter_map(Var::cast).collect()
    }
}

impl AstNode for BlankPropertyList {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::BlankNodePropertyListPath
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::BlankNodePropertyListPath
                | SyntaxKind::BlankNodePropertyList
                | SyntaxKind::BlankNode
        )
    }

    #[inline]
    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.syntax.descendants().filter_map(Var::cast).collect()
    }
}

impl AstNode for PropertyListPath {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::PropertyListPathNotEmpty
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::PropertyListPath | SyntaxKind::PropertyListPathNotEmpty
        )
    }

    #[inline]
    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.syntax.descendants().filter_map(Var::cast).collect()
    }
}

impl AstNode for Subject {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::VarOrTerm
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(kind, SyntaxKind::VarOrTerm | SyntaxKind::TriplesNodePath)
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }
    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.syntax
            .first_child()
            .and_then(Var::cast)
            .map(|var| vec![var])
            .unwrap_or_default()
    }
}

impl AstNode for Triple {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::TriplesSameSubjectPath
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }
    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.syntax.descendants().filter_map(Var::cast).collect()
    }
}

impl AstNode for TriplesBlock {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::TriplesBlock
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.syntax.descendants().filter_map(Var::cast).collect()
    }
}

impl AstNode for Expression {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::Expression
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.syntax.descendants().filter_map(Var::cast).collect()
    }
}

impl AstNode for GroupGraphPattern {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::GroupGraphPattern
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.sub_select()
            .map(|select| select.visible_variables())
            .or(self.syntax.first_child().map(|child| {
                child
                    .children()
                    .into_iter()
                    .filter_map(|child| match child.kind() {
                        SyntaxKind::TriplesBlock => {
                            TriplesBlock::cast(child).map(|tp| tp.visible_variables())
                        }
                        SyntaxKind::GraphPatternNotTriples => GraphPatternNotTriples::cast(child)
                            .map(|pattern| pattern.visible_variables()),
                        _ => None,
                    })
                    .flatten()
                    .collect()
            }))
            .unwrap_or_default()
    }
}

impl AstNode for WhereClause {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::WhereClause
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.group_graph_pattern()
            .map(|ggp| ggp.visible_variables())
            .unwrap_or_default()
    }
}
impl AstNode for OptionalGraphPattern {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::OptionalGraphPattern
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.group_graph_pattern()
            .map(|ggp| ggp.visible_variables())
            .unwrap_or_default()
    }
}

impl AstNode for GroupOrUnionGraphPattern {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::GroupOrUnionGraphPattern
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.group_graph_patterns()
            .into_iter()
            .flat_map(|ggp| ggp.visible_variables())
            .collect()
    }
}

impl AstNode for MinusGraphPattern {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::MinusGraphPattern
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.group_graph_pattern()
            .map(|ggp| ggp.visible_variables())
            .unwrap_or_default()
    }
}

impl AstNode for GraphGraphPattern {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::GraphGraphPattern
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.group_graph_pattern()
            .map(|ggp| ggp.visible_variables())
            .unwrap_or_default()
    }
}

impl AstNode for ServiceGraphPattern {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::ServiceGraphPattern
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.group_graph_pattern()
            .map(|ggp| ggp.visible_variables())
            .unwrap_or_default()
    }
}

impl AstNode for Filter {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::Filter
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.syntax().descendants().filter_map(Var::cast).collect()
    }
}

impl AstNode for Bind {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::Bind
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.syntax()
            .children()
            .last()
            .and_then(Var::cast)
            .map(|var| vec![var])
            .unwrap_or_default()
    }
}

impl AstNode for InlineData {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::InlineData
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.syntax()
            .last_child()
            .and_then(|data_block| data_block.first_child())
            .and_then(|inline_data| match inline_data.kind() {
                SyntaxKind::InlineDataOneVar => inline_data
                    .first_child()
                    .and_then(Var::cast)
                    .map(|var| vec![var]),
                SyntaxKind::InlineDataFull => Some(
                    inline_data
                        .children_with_tokens()
                        .take_while(|child| child.kind() != SyntaxKind::RParen)
                        .filter_map(|child| child.into_node().and_then(Var::cast))
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_default()
    }
}

impl AstNode for SelectClause {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::SelectClause
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.projected_variables()
    }
}

impl AstNode for Prologue {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::Prologue
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        vec![]
    }
}

impl AstNode for SolutionModifier {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::SolutionModifier
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        vec![]
    }
}

impl AstNode for GroupClause {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::GroupClause
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        // NOTE: A GroupCondition binds a variable if it is a plain variable ("?x"),
        //       an alias ("(... AS ?x)"), or a bracketted variable ("(?x)").
        self.syntax
            .children()
            .into_iter()
            .filter_map(|group_condition| group_condition.last_child())
            .filter_map(|node| Var::cast(node.clone()).or_else(|| Expression::cast(node)?.as_var()))
            .collect()
    }
}

impl AstNode for PrefixDeclaration {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::PrefixDecl
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        vec![]
    }
}

impl AstNode for QueryUnit {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::QueryUnit
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        self.select_query()
            .map(|select_query| select_query.visible_variables())
            .unwrap_or_default()
    }
}

impl AstNode for UpdateUnit {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::UpdateUnit
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        vec![]
    }
}

impl AstNode for Unit {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::QueryUnit
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(kind, SyntaxKind::QueryUnit | SyntaxKind::UpdateUnit)
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        match syntax.kind() {
            SyntaxKind::QueryUnit => Some(Unit::Query(QueryUnit::cast(syntax)?)),
            SyntaxKind::UpdateUnit => Some(Unit::Update(UpdateUnit::cast(syntax)?)),
            _ => None,
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        match self {
            Unit::Query(query_unit) => query_unit.syntax(),
            Unit::Update(update_unit) => update_unit.syntax(),
        }
    }

    fn visible_variables(&self) -> Vec<Var> {
        match self {
            Unit::Query(query_unit) => query_unit.visible_variables(),
            Unit::Update(update_unit) => update_unit.visible_variables(),
        }
    }
}

impl AstNode for SelectQuery {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::SelectQuery
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(kind, SyntaxKind::SelectQuery | SyntaxKind::SubSelect)
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self { syntax })
        } else {
            None
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }

    fn visible_variables(&self) -> Vec<Var> {
        if let Some(select_clause) = self.select_clause() {
            if let Some(SyntaxKind::Star) = select_clause
                .syntax
                .last_child_or_token()
                .map(|last| last.kind())
            {
                self.where_clause()
                    .map(|where_clause| where_clause.visible_variables())
                    .unwrap_or_default()
            } else {
                select_clause.visible_variables()
            }
        } else {
            Vec::new()
        }
    }
}

impl AstNode for GraphPatternNotTriples {
    #[inline]
    fn kind() -> SyntaxKind {
        SyntaxKind::GraphPatternNotTriples
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::GroupOrUnionGraphPattern
                | SyntaxKind::OptionalGraphPattern
                | SyntaxKind::MinusGraphPattern
                | SyntaxKind::GraphGraphPattern
                | SyntaxKind::ServiceGraphPattern
                | SyntaxKind::Filter
                | SyntaxKind::Bind
                | SyntaxKind::InlineData
        )
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        let child = syntax.first_child()?;
        match child.kind() {
            SyntaxKind::GroupOrUnionGraphPattern => {
                Some(GraphPatternNotTriples::GroupOrUnionGraphPattern(
                    GroupOrUnionGraphPattern::cast(child)?,
                ))
            }
            SyntaxKind::OptionalGraphPattern => Some(GraphPatternNotTriples::OptionalGraphPattern(
                OptionalGraphPattern::cast(child)?,
            )),
            SyntaxKind::MinusGraphPattern => Some(GraphPatternNotTriples::MinusGraphPattern(
                MinusGraphPattern::cast(child)?,
            )),
            SyntaxKind::GraphGraphPattern => Some(GraphPatternNotTriples::GraphGraphPattern(
                GraphGraphPattern::cast(child)?,
            )),
            SyntaxKind::ServiceGraphPattern => Some(GraphPatternNotTriples::ServiceGraphPattern(
                ServiceGraphPattern::cast(child)?,
            )),
            SyntaxKind::Filter => Some(GraphPatternNotTriples::Filter(Filter::cast(child)?)),
            SyntaxKind::Bind => Some(GraphPatternNotTriples::Bind(Bind::cast(child)?)),
            SyntaxKind::InlineData => {
                Some(GraphPatternNotTriples::InlineData(InlineData::cast(child)?))
            }
            _ => None,
        }
    }

    #[inline]
    fn syntax(&self) -> &SyntaxNode {
        match self {
            GraphPatternNotTriples::GroupOrUnionGraphPattern(x) => x.syntax(),
            GraphPatternNotTriples::OptionalGraphPattern(x) => x.syntax(),
            GraphPatternNotTriples::MinusGraphPattern(x) => x.syntax(),
            GraphPatternNotTriples::GraphGraphPattern(x) => x.syntax(),
            GraphPatternNotTriples::ServiceGraphPattern(x) => x.syntax(),
            GraphPatternNotTriples::Filter(x) => x.syntax(),
            GraphPatternNotTriples::Bind(x) => x.syntax(),
            GraphPatternNotTriples::InlineData(x) => x.syntax(),
        }
    }

    fn visible_variables(&self) -> Vec<Var> {
        match self {
            GraphPatternNotTriples::GroupOrUnionGraphPattern(x) => x.visible_variables(),
            GraphPatternNotTriples::OptionalGraphPattern(x) => x.visible_variables(),
            GraphPatternNotTriples::MinusGraphPattern(x) => x.visible_variables(),
            GraphPatternNotTriples::GraphGraphPattern(x) => x.visible_variables(),
            GraphPatternNotTriples::ServiceGraphPattern(x) => x.visible_variables(),
            GraphPatternNotTriples::Filter(x) => x.visible_variables(),
            GraphPatternNotTriples::Bind(x) => x.visible_variables(),
            GraphPatternNotTriples::InlineData(x) => x.visible_variables(),
        }
    }
}

pub trait AstNode {
    fn kind() -> SyntaxKind;

    #[inline]
    fn can_cast(kind: SyntaxKind) -> bool {
        Self::kind() == kind
    }

    fn cast(syntax: SyntaxNode) -> Option<Self>
    where
        Self: Sized;

    fn syntax(&self) -> &SyntaxNode;

    /// Returns all variables "visible" from outside the node.
    fn visible_variables(&self) -> Vec<Var>;

    fn has_error(&self) -> bool {
        self.syntax()
            .preorder()
            .find(|walk_event| match walk_event {
                rowan::WalkEvent::Enter(node) if node.kind() == SyntaxKind::Error => true,
                _ => false,
            })
            .is_some()
    }

    fn collect_decendants(&self, matcher: &impl Fn(SyntaxKind) -> bool) -> Vec<SyntaxNode> {
        self.syntax()
            .preorder()
            .filter_map(|walk_event| match walk_event {
                rowan::WalkEvent::Enter(node) if matcher(node.kind()) => Some(node),
                _ => None,
            })
            .collect()
    }

    fn preorder_find_kind(&self, kind: SyntaxKind) -> Vec<SyntaxNode> {
        self.syntax()
            .preorder()
            .filter_map(|walk_event| match walk_event {
                rowan::WalkEvent::Enter(node) if node.kind() == kind => Some(node),
                _ => None,
            })
            .collect()
    }

    fn used_prefixes(&self) -> Vec<String> {
        self.syntax()
            .descendants()
            .filter_map(PrefixedName::cast)
            .map(|prefixed_name| prefixed_name.prefix())
            .collect()
    }

    fn text(&self) -> String {
        self.syntax().text().to_string()
    }

    fn text_until(&self, offset: TextSize) -> String {
        let syntax = self.syntax();
        assert!(syntax.text_range().start() <= offset);
        syntax
            .text()
            .slice(TextSize::new(0)..(offset - syntax.text_range().start()))
            .to_string()
    }
}

#[cfg(test)]
mod tests;
