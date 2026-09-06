//! Typed identity for the provider seam (plans/0025): the three
//! nouns that cross every boundary — which repo, which content, which
//! revision — as opaque newtypes instead of naked `String`s. The bug
//! classes this kills: a sha where a ref was meant, a ref where a
//! repo id was meant, a repo id formatted differently at two sites.
//!
//! Wire discipline: the protocol is stringly by design (reader
//! tolerance, opaque ids); these types are the *Rust* vocabulary.
//! They are `#[serde(transparent)]` so wire models can adopt them
//! without format changes. Millers-pane structure (separate
//! owner/name) is a UI concern and stays untyped — the joined id is
//! minted where the seam begins.

use serde::{Deserialize, Serialize};

macro_rules! opaque_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// The wire/UI form, verbatim.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                $name(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                $name(value.to_string())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

opaque_id!(
    RepoId,
    "Opaque `\"group/project\"` repo identity (protocol: the UI never\n\
     parses it; each backend owns its grammar)."
);

opaque_id!(
    Sha,
    "Opaque content id: MUST change when content changes (the cache\n\
     is content-keyed and immutable). A commit sha on backends that\n\
     have them; whatever the adapter guarantees otherwise."
);

opaque_id!(
    GitRef,
    "A resolvable revision name — branch, tag, or commit sha — as the\n\
     backend interprets it (`repo/tree`'s `ref` param). `None` at a\n\
     call site means the default branch; this type never encodes it."
);

impl std::fmt::Display for Generation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A staleness clock for async replies (plans/0025): workers capture
/// the generation they were spawned under; the landing site drops
/// results whose generation is no longer current. Replaces raw `u64`
/// counters compared by `!=` — the type makes the guard read as a
/// guard and stops cross-counter comparisons from compiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Generation(u64);

impl Generation {
    /// Tick the clock; the returned value is what a spawn captures.
    pub fn tick(&mut self) -> Generation {
        self.0 += 1;
        *self
    }

    /// Is `other` the generation this clock is currently at? A spawn
    /// holds the value it captured; the landing site asks the clock.
    pub fn is_current(&self, other: Generation) -> bool {
        *self == other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_guards_drop_stale_captures() {
        let mut clock = Generation::default();
        let spawn = clock.tick();
        assert!(clock.is_current(spawn));
        let _newer = clock.tick();
        assert!(!clock.is_current(spawn));
    }
}
