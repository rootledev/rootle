//! Typed unified diffs. Parsing validates hunk extents; malformed or
//! incomplete patches are errors rather than plausible-looking source.

mod emphasis;
mod parse;
#[cfg(test)]
mod tests;

pub use parse::{PatchError, PatchErrorKind};
use std::num::NonZeroU32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineOrigin {
    Context,
    Addition,
    Deletion,
}

/// One-based source line. Missing sides are `None`, never line zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LineNumber(NonZeroU32);
impl LineNumber {
    pub fn new(value: u32) -> Option<Self> {
        NonZeroU32::new(value).map(Self)
    }
    pub fn get(self) -> u32 {
        self.0.get()
    }
}
impl std::fmt::Display for LineNumber {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub origin: LineOrigin,
    pub old_line: Option<LineNumber>,
    pub new_line: Option<LineNumber>,
    /// Unprefixed content, preserving CR and other bytes represented in UTF-8.
    pub text: String,
    pub has_newline: bool,
}

/// A side of a hunk. An empty range may start at zero; nonempty ranges
/// are checked to fit positive u32 source line numbers by the parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HunkRange {
    pub start: u32,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old: HunkRange,
    pub new: HunkRange,
    pub context: String,
    pub lines: Vec<DiffLine>,
}
impl Hunk {
    pub fn header(&self) -> String {
        format!(
            "@@ -{},{} +{},{} @@",
            self.old.start, self.old.count, self.new.start, self.new.count
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDiff {
    pub hunks: Vec<Hunk>,
    pub added: u64,
    pub deleted: u64,
}

/// Character-boundary-safe byte offsets into one specific diff line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedSpan(std::ops::Range<usize>);
impl ChangedSpan {
    pub fn range(&self) -> std::ops::Range<usize> {
        self.0.clone()
    }
}
