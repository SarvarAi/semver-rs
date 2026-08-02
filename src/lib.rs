//! A Rust port of [`npm/node-semver`](https://github.com/npm/node-semver),
//! pinned at commit `6e05b7637396ac66522cff8731f07cfe0ef49a29` (tag `v7.8.5`, ISC).
//!
//! This implements **npm's** semantic-version dialect, which is a superset of
//! plain SemVer 2.0.0: X-ranges (`1.2.x`), npm tilde/caret bump rules, hyphen
//! ranges, `||` combinators, `includePrerelease`, `loose` parsing and
//! `coerce()`. Rust's existing `semver` crate implements SemVer 2.0.0 for
//! Cargo's own needs and none of that range grammar.
//!
//! The port contains no `unsafe`, and neither links nor invokes Node. The
//! differential test harness that compares against the original *does* run
//! Node, and is gated behind the non-default `harness` feature so it can never
//! end up in a release build.

#![forbid(unsafe_code)]

pub mod comparator;
pub mod constants;
pub mod functions;
pub mod lrucache;
pub mod identifiers;
pub mod options;
pub mod range;
pub mod ranges;
pub mod re;
pub mod semver;
pub mod subset;
pub mod util;

pub use constants::{RELEASE_TYPES, SEMVER_SPEC_VERSION};
pub use functions::{
    clean, cmp, compare, compare_build, compare_loose, coerce, diff, eq, gt, gte, lt, lte, major,
    inc, minor, neq, parse, patch, prerelease, rcompare, rsort, sort, truncate, valid, CoerceOptions,
};
pub use identifiers::{compare_identifiers, rcompare_identifiers, Identifier};
pub use options::Options;
pub use comparator::Comparator;
pub use range::{satisfies, to_comparators, valid_range, Range};
pub use ranges::{
    gtr, intersects, ltr, max_satisfying, min_satisfying, min_version, outside, simplify_range,
};
pub use semver::{IdentifierBase, SemVer};
pub use subset::subset;

/// The error type for operations that upstream signals by throwing.
///
/// Upstream has two failure conventions and the port keeps them distinct:
/// functions like `parse()` and `valid()` swallow errors and return `null`
/// (modelled as `Option`), while constructors like `new SemVer()` throw
/// (modelled as `Result<_, Error>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub message: String,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
