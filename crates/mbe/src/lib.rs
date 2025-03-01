//! `mbe` (short for Macro By Example) crate contains code for handling
//! `macro_rules` macros. It uses `TokenTree` (from `tt` package) as the
//! interface, although it contains some code to bridge `SyntaxNode`s and
//! `TokenTree`s as well!

mod parser;
mod expander;
mod syntax_bridge;
mod tt_iter;
mod subtree_source;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod benchmark;
mod token_map;

use std::fmt;

use crate::{
    parser::{MetaTemplate, Op},
    tt_iter::TtIter,
};

// FIXME: we probably should re-think  `token_tree_to_syntax_node` interfaces
pub use ::parser::ParserEntryPoint;
pub use tt::{Delimiter, DelimiterKind, Punct};

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    UnexpectedToken(String),
    Expected(String),
    InvalidRepeat,
    RepetitionEmptyTokenTree,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ExpandError {
    NoMatchingRule,
    UnexpectedToken,
    BindingError(String),
    ConversionError,
    // FIXME: no way mbe should know about proc macros.
    UnresolvedProcMacro,
    Other(String),
}

impl fmt::Display for ExpandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExpandError::NoMatchingRule => f.write_str("no rule matches input tokens"),
            ExpandError::UnexpectedToken => f.write_str("unexpected token in input"),
            ExpandError::BindingError(e) => f.write_str(e),
            ExpandError::ConversionError => f.write_str("could not convert tokens"),
            ExpandError::UnresolvedProcMacro => f.write_str("unresolved proc macro"),
            ExpandError::Other(e) => f.write_str(e),
        }
    }
}

pub use crate::{
    syntax_bridge::{
        parse_exprs_with_sep, parse_to_token_tree, syntax_node_to_token_tree,
        syntax_node_to_token_tree_censored, token_tree_to_syntax_node,
    },
    token_map::TokenMap,
};

/// This struct contains AST for a single `macro_rules` definition. What might
/// be very confusing is that AST has almost exactly the same shape as
/// `tt::TokenTree`, but there's a crucial difference: in macro rules, `$ident`
/// and `$()*` have special meaning (see `Var` and `Repeat` data structures)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroRules {
    rules: Vec<Rule>,
    /// Highest id of the token we have in TokenMap
    shift: Shift,
}

/// For Macro 2.0
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroDef {
    rules: Vec<Rule>,
    /// Highest id of the token we have in TokenMap
    shift: Shift,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Rule {
    lhs: MetaTemplate,
    rhs: MetaTemplate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shift(u32);

impl Shift {
    pub fn new(tt: &tt::Subtree) -> Shift {
        // Note that TokenId is started from zero,
        // We have to add 1 to prevent duplication.
        let value = max_id(tt).map_or(0, |it| it + 1);
        return Shift(value);

        // Find the max token id inside a subtree
        fn max_id(subtree: &tt::Subtree) -> Option<u32> {
            subtree
                .token_trees
                .iter()
                .filter_map(|tt| match tt {
                    tt::TokenTree::Subtree(subtree) => {
                        let tree_id = max_id(subtree);
                        match subtree.delimiter {
                            Some(it) if it.id != tt::TokenId::unspecified() => {
                                Some(tree_id.map_or(it.id.0, |t| t.max(it.id.0)))
                            }
                            _ => tree_id,
                        }
                    }
                    tt::TokenTree::Leaf(tt::Leaf::Ident(ident))
                        if ident.id != tt::TokenId::unspecified() =>
                    {
                        Some(ident.id.0)
                    }
                    _ => None,
                })
                .max()
        }
    }

    /// Shift given TokenTree token id
    pub fn shift_all(self, tt: &mut tt::Subtree) {
        for t in &mut tt.token_trees {
            match t {
                tt::TokenTree::Leaf(leaf) => match leaf {
                    tt::Leaf::Ident(ident) => ident.id = self.shift(ident.id),
                    tt::Leaf::Punct(punct) => punct.id = self.shift(punct.id),
                    tt::Leaf::Literal(lit) => lit.id = self.shift(lit.id),
                },
                tt::TokenTree::Subtree(tt) => {
                    if let Some(it) = tt.delimiter.as_mut() {
                        it.id = self.shift(it.id);
                    };
                    self.shift_all(tt)
                }
            }
        }
    }

    pub fn shift(self, id: tt::TokenId) -> tt::TokenId {
        if id == tt::TokenId::unspecified() {
            return id;
        }
        tt::TokenId(id.0 + self.0)
    }

    pub fn unshift(self, id: tt::TokenId) -> Option<tt::TokenId> {
        id.0.checked_sub(self.0).map(tt::TokenId)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum Origin {
    Def,
    Call,
}

impl MacroRules {
    pub fn parse(tt: &tt::Subtree) -> Result<MacroRules, ParseError> {
        // Note: this parsing can be implemented using mbe machinery itself, by
        // matching against `$($lhs:tt => $rhs:tt);*` pattern, but implementing
        // manually seems easier.
        let mut src = TtIter::new(tt);
        let mut rules = Vec::new();
        while src.len() > 0 {
            let rule = Rule::parse(&mut src, true)?;
            rules.push(rule);
            if let Err(()) = src.expect_char(';') {
                if src.len() > 0 {
                    return Err(ParseError::Expected("expected `;`".to_string()));
                }
                break;
            }
        }

        for rule in &rules {
            validate(&rule.lhs)?;
        }

        Ok(MacroRules { rules, shift: Shift::new(tt) })
    }

    pub fn expand(&self, tt: &tt::Subtree) -> ExpandResult<tt::Subtree> {
        // apply shift
        let mut tt = tt.clone();
        self.shift.shift_all(&mut tt);
        expander::expand_rules(&self.rules, &tt)
    }

    pub fn map_id_down(&self, id: tt::TokenId) -> tt::TokenId {
        self.shift.shift(id)
    }

    pub fn map_id_up(&self, id: tt::TokenId) -> (tt::TokenId, Origin) {
        match self.shift.unshift(id) {
            Some(id) => (id, Origin::Call),
            None => (id, Origin::Def),
        }
    }
}

impl MacroDef {
    pub fn parse(tt: &tt::Subtree) -> Result<MacroDef, ParseError> {
        let mut src = TtIter::new(tt);
        let mut rules = Vec::new();

        if Some(tt::DelimiterKind::Brace) == tt.delimiter_kind() {
            cov_mark::hit!(parse_macro_def_rules);
            while src.len() > 0 {
                let rule = Rule::parse(&mut src, true)?;
                rules.push(rule);
                if let Err(()) = src.expect_any_char(&[';', ',']) {
                    if src.len() > 0 {
                        return Err(ParseError::Expected(
                            "expected `;` or `,` to delimit rules".to_string(),
                        ));
                    }
                    break;
                }
            }
        } else {
            cov_mark::hit!(parse_macro_def_simple);
            let rule = Rule::parse(&mut src, false)?;
            if src.len() != 0 {
                return Err(ParseError::Expected("remain tokens in macro def".to_string()));
            }
            rules.push(rule);
        }
        for rule in &rules {
            validate(&rule.lhs)?;
        }

        Ok(MacroDef { rules, shift: Shift::new(tt) })
    }

    pub fn expand(&self, tt: &tt::Subtree) -> ExpandResult<tt::Subtree> {
        // apply shift
        let mut tt = tt.clone();
        self.shift.shift_all(&mut tt);
        expander::expand_rules(&self.rules, &tt)
    }

    pub fn map_id_down(&self, id: tt::TokenId) -> tt::TokenId {
        self.shift.shift(id)
    }

    pub fn map_id_up(&self, id: tt::TokenId) -> (tt::TokenId, Origin) {
        match self.shift.unshift(id) {
            Some(id) => (id, Origin::Call),
            None => (id, Origin::Def),
        }
    }
}

impl Rule {
    fn parse(src: &mut TtIter, expect_arrow: bool) -> Result<Self, ParseError> {
        let lhs = src
            .expect_subtree()
            .map_err(|()| ParseError::Expected("expected subtree".to_string()))?;
        if expect_arrow {
            src.expect_char('=').map_err(|()| ParseError::Expected("expected `=`".to_string()))?;
            src.expect_char('>').map_err(|()| ParseError::Expected("expected `>`".to_string()))?;
        }
        let rhs = src
            .expect_subtree()
            .map_err(|()| ParseError::Expected("expected subtree".to_string()))?;

        let lhs = MetaTemplate::parse_pattern(lhs)?;
        let rhs = MetaTemplate::parse_template(rhs)?;

        Ok(crate::Rule { lhs, rhs })
    }
}

fn validate(pattern: &MetaTemplate) -> Result<(), ParseError> {
    for op in pattern.iter() {
        match op {
            Op::Subtree { tokens, .. } => validate(tokens)?,
            Op::Repeat { tokens: subtree, separator, .. } => {
                // Checks that no repetition which could match an empty token
                // https://github.com/rust-lang/rust/blob/a58b1ed44f5e06976de2bdc4d7dc81c36a96934f/src/librustc_expand/mbe/macro_rules.rs#L558

                if separator.is_none()
                    && subtree.iter().all(|child_op| {
                        match child_op {
                            Op::Var { kind, .. } => {
                                // vis is optional
                                if kind.as_ref().map_or(false, |it| it == "vis") {
                                    return true;
                                }
                            }
                            Op::Repeat { kind, .. } => {
                                return matches!(
                                    kind,
                                    parser::RepeatKind::ZeroOrMore | parser::RepeatKind::ZeroOrOne
                                )
                            }
                            Op::Leaf(_) => {}
                            Op::Subtree { .. } => {}
                        }
                        false
                    })
                {
                    return Err(ParseError::RepetitionEmptyTokenTree);
                }
                validate(subtree)?
            }
            _ => (),
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ExpandResult<T> {
    pub value: T,
    pub err: Option<ExpandError>,
}

impl<T> ExpandResult<T> {
    pub fn ok(value: T) -> Self {
        Self { value, err: None }
    }

    pub fn only_err(err: ExpandError) -> Self
    where
        T: Default,
    {
        Self { value: Default::default(), err: Some(err) }
    }

    pub fn str_err(err: String) -> Self
    where
        T: Default,
    {
        Self::only_err(ExpandError::Other(err))
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> ExpandResult<U> {
        ExpandResult { value: f(self.value), err: self.err }
    }

    pub fn result(self) -> Result<T, ExpandError> {
        self.err.map_or(Ok(self.value), Err)
    }
}

impl<T: Default> From<Result<T, ExpandError>> for ExpandResult<T> {
    fn from(result: Result<T, ExpandError>) -> Self {
        result.map_or_else(Self::only_err, Self::ok)
    }
}
