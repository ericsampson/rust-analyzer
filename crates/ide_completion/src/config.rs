//! Settings for tweaking completion.
//!
//! The fun thing here is `SnippetCap` -- this type can only be created in this
//! module, and we use to statically check that we only produce snippet
//! completions if we are allowed to.

use ide_db::helpers::{insert_use::InsertUseConfig, SnippetCap};

use crate::snippet::Snippet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionConfig {
    pub enable_postfix_completions: bool,
    pub enable_imports_on_the_fly: bool,
    pub enable_self_on_the_fly: bool,
    pub add_call_parenthesis: bool,
    pub add_call_argument_snippets: bool,
    pub snippet_cap: Option<SnippetCap>,
    pub insert_use: InsertUseConfig,
    pub snippets: Vec<Snippet>,
}

impl CompletionConfig {
    pub fn postfix_snippets(&self) -> impl Iterator<Item = (&str, &Snippet)> {
        self.snippets.iter().flat_map(|snip| {
            snip.postfix_triggers.iter().map(move |trigger| (trigger.as_str(), snip))
        })
    }
    pub fn prefix_snippets(&self) -> impl Iterator<Item = (&str, &Snippet)> {
        self.snippets.iter().flat_map(|snip| {
            snip.prefix_triggers.iter().map(move |trigger| (trigger.as_str(), snip))
        })
    }
}
