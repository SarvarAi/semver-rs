//! Port of `internal/constants.js`.

/// The semver.org spec version implemented, not the version of this crate.
pub const SEMVER_SPEC_VERSION: &str = "2.0.0";

pub const MAX_LENGTH: usize = 256;

/// JavaScript's `Number.MAX_SAFE_INTEGER`, i.e. 2^53 - 1.
///
/// Upstream stores major/minor/patch as f64 and range-checks against this, so
/// the port keeps the same type and the same bound (see DECISIONS.md D7).
pub const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Max safe segment length for coercion.
pub const MAX_SAFE_COMPONENT_LENGTH: usize = 16;

/// Max length for a build identifier: MAX_LENGTH minus the 6 characters of the
/// shortest version carrying build metadata, `0.0.0+BUILD`.
pub const MAX_SAFE_BUILD_LENGTH: usize = MAX_LENGTH - 6;

pub const RELEASE_TYPES: [&str; 7] = [
    "major",
    "premajor",
    "minor",
    "preminor",
    "patch",
    "prepatch",
    "prerelease",
];

pub const FLAG_INCLUDE_PRERELEASE: u8 = 0b001;
pub const FLAG_LOOSE: u8 = 0b010;
