//! Port of `internal/parse-options.js`.
//!
//! Upstream's `parseOptions` is a passthrough rather than a normalizer:
//!
//! ```js
//! if (!options) { return emptyOpts }                       // null/undefined/0/false/''
//! if (typeof options !== 'object') { return looseOption }  // true/123/'x'
//! return options                                           // objects unchanged
//! ```
//!
//! Every downstream consumer then reads `!!options.loose` and
//! `!!options.includePrerelease`, so those two booleans are the entire
//! observable surface. Rust models exactly that. The exotic JS values that
//! appear in upstream fixtures (`/asdf/`, `{loose: 123}`, `{loose: null}`) are
//! resolved once at corpus-extraction time by upstream's own `parseOptions`,
//! and the corpus records both spellings so the reduction stays auditable.
//!
//! See DECISIONS.md D6.

use crate::constants::{FLAG_INCLUDE_PRERELEASE, FLAG_LOOSE};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Options {
    pub loose: bool,
    pub include_prerelease: bool,
}

impl Options {
    pub const fn new() -> Self {
        Self { loose: false, include_prerelease: false }
    }

    pub const fn loose() -> Self {
        Self { loose: true, include_prerelease: false }
    }

    pub const fn include_prerelease() -> Self {
        Self { loose: false, include_prerelease: true }
    }

    pub const fn with_loose(mut self, v: bool) -> Self {
        self.loose = v;
        self
    }

    pub const fn with_include_prerelease(mut self, v: bool) -> Self {
        self.include_prerelease = v;
        self
    }

    /// Mirrors the memo key built in `Range.parseRange`.
    pub const fn flags(&self) -> u8 {
        (if self.include_prerelease { FLAG_INCLUDE_PRERELEASE } else { 0 })
            | (if self.loose { FLAG_LOOSE } else { 0 })
    }
}

/// Convenience for the many upstream call sites that pass a bare boolean where
/// an options object is expected — `new SemVer(x, true)` means loose.
impl From<bool> for Options {
    fn from(loose: bool) -> Self {
        Self { loose, include_prerelease: false }
    }
}

impl From<Option<Options>> for Options {
    fn from(o: Option<Options>) -> Self {
        o.unwrap_or_default()
    }
}
