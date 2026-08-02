//! Port of `internal/identifiers.js`.
//!
//! Upstream compares prerelease identifiers after coercing numeric-looking ones
//! with JavaScript's `+a`, i.e. IEEE-754 double. Above 2^53 that is lossy, and
//! upstream consequently reports `1.0.0-9007199254740993` and
//! `1.0.0-9007199254740992` as **equal**.
//!
//! This port reproduces that deliberately: the contract being ported is "behave
//! like node-semver", not "behave like an idealised semver". Using `u64` here
//! would be a parity bug. See DECISIONS.md D7.

use std::cmp::Ordering;
use std::sync::LazyLock;

use regex::Regex;

static NUMERIC: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[0-9]+$").unwrap());

/// A prerelease or build identifier.
///
/// Upstream's `SemVer` stores prerelease parts as JS numbers when they are
/// numeric and in safe range, and as strings otherwise, and `compareIdentifiers`
/// branches on that distinction. The port keeps the distinction explicit.
#[derive(Debug, Clone, PartialEq)]
pub enum Identifier {
    Num(f64),
    Str(String),
}

impl Identifier {
    /// How JavaScript would stringify this value (`String(x)`), which is what
    /// `numeric.test(a)` implicitly does to a number argument.
    pub fn as_js_string(&self) -> String {
        match self {
            Identifier::Num(n) => crate::util::js_number_to_string(*n),
            Identifier::Str(s) => s.clone(),
        }
    }

    pub fn is_num(&self) -> bool {
        matches!(self, Identifier::Num(_))
    }
}

impl std::fmt::Display for Identifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_js_string())
    }
}

/// JavaScript compares strings by UTF-16 code unit, Rust by UTF-8 byte. Those
/// orders agree for everything the semver grammar can produce (`[a-zA-Z0-9-]`),
/// but `compareIdentifiers` is a public export that accepts arbitrary strings,
/// so match JS exactly rather than relying on the inputs staying well-behaved.
fn js_string_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// Port of `compareIdentifiers`.
pub fn compare_identifiers(a: &Identifier, b: &Identifier) -> i32 {
    // Fast path: upstream's `typeof a === 'number' && typeof b === 'number'`.
    if let (Identifier::Num(x), Identifier::Num(y)) = (a, b) {
        return if x == y {
            0
        } else if x < y {
            -1
        } else {
            1
        };
    }

    let as_ = a.as_js_string();
    let bs = b.as_js_string();
    let anum = NUMERIC.is_match(&as_);
    let bnum = NUMERIC.is_match(&bs);

    if anum && bnum {
        // `a = +a; b = +b` — the lossy step, reproduced on purpose.
        let x = as_.parse::<f64>().unwrap_or(f64::NAN);
        let y = bs.parse::<f64>().unwrap_or(f64::NAN);
        return if x == y {
            0
        } else if x < y {
            -1
        } else {
            1
        };
    }

    if as_ == bs {
        0
    } else if anum && !bnum {
        -1
    } else if bnum && !anum {
        1
    } else if js_string_cmp(&as_, &bs) == Ordering::Less {
        -1
    } else {
        1
    }
}

/// Port of `rcompareIdentifiers`.
pub fn rcompare_identifiers(a: &Identifier, b: &Identifier) -> i32 {
    compare_identifiers(b, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> Identifier {
        Identifier::Str(x.into())
    }
    fn n(x: f64) -> Identifier {
        Identifier::Num(x)
    }

    #[test]
    fn numeric_beats_alpha() {
        assert_eq!(compare_identifiers(&s("1"), &s("alpha")), -1);
        assert_eq!(compare_identifiers(&s("alpha"), &s("1")), 1);
    }

    #[test]
    fn numbers_compare_numerically_not_lexically() {
        assert_eq!(compare_identifiers(&s("2"), &s("10")), -1);
        assert_eq!(compare_identifiers(&n(2.0), &n(10.0)), -1);
    }

    #[test]
    fn reproduces_javascript_f64_precision_loss() {
        // 2^53 and 2^53+1 are the same double. Upstream says these are equal,
        // so this port must too. See DECISIONS.md D7.
        assert_eq!(compare_identifiers(&s("9007199254740992"), &s("9007199254740993")), 0);
        // Below the boundary the distinction survives.
        assert_eq!(compare_identifiers(&s("9007199254740991"), &s("9007199254740992")), -1);
    }
}
