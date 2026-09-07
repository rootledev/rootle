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

opaque_id!(
    RepoPath,
    "An opaque repository-relative path, not a host filesystem path."
);
opaque_id!(
    OrgId,
    "An opaque organization or group identity, independent of its displayed caption."
);

impl Sha {
    pub fn short(&self) -> String {
        self.as_str().chars().take(7).collect()
    }
}

/// Domain-tagged request generation. Different pipelines cannot compare
/// clocks, even though both happen to be represented by an integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Generation<Domain> {
    value: u64,
    domain: std::marker::PhantomData<Domain>,
}

impl<Domain> Default for Generation<Domain> {
    fn default() -> Self {
        Self {
            value: 0,
            domain: std::marker::PhantomData,
        }
    }
}

impl<Domain: Copy + PartialEq> Generation<Domain> {
    pub fn tick(&mut self) -> Self {
        self.value = self
            .value
            .checked_add(1)
            .expect("request generation exhausted");
        *self
    }

    pub fn is_current(&self, other: Self) -> bool {
        *self == other
    }
}

impl<Domain> std::fmt::Display for Generation<Domain> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.value.fmt(formatter)
    }
}

/// Diagnostic/wire serialization exposes the count without weakening the
/// domain-typed comparison API or round-tripping through Display.
impl<Domain> serde::Serialize for Generation<Domain> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_guards_drop_stale_captures() {
        let mut clock = Generation::<()>::default();
        let spawn = clock.tick();
        assert!(clock.is_current(spawn));
        let _newer = clock.tick();
        assert!(!clock.is_current(spawn));
    }
}
