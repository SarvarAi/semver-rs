//! Port of `internal/re.js`.
//!
//! Upstream builds 43 regex tokens by composing earlier tokens' *source
//! strings*, then derives a second, ReDoS-hardened table by rewriting unbounded
//! quantifiers into bounded ones. The library matches with the hardened
//! (`safeSrc`) table; the raw table is exported for userland and used in exactly
//! one internal place (`BUILDSTRIPRE` in `classes/range.js`).
//!
//! This module rebuilds both tables the same way, in the same order, from the
//! same literals. `tests/port/re_table.rs` asserts the result is byte-identical
//! to `tests/original/re-table.json`, which is dumped straight out of the
//! running upstream module — so "the regexes were ported faithfully" is a
//! mechanical check rather than a claim. See DECISIONS.md D5 and D8.
//!
//! The one thing that cannot be byte-identical is what gets *compiled*:
//! JavaScript's `\d` and `\s` do not mean what Rust's `regex` crate means by
//! them. See [`js_to_rust_dialect`] and DECISIONS.md D9.

use std::sync::LazyLock;

use regex::Regex;

use crate::constants::{MAX_LENGTH, MAX_SAFE_BUILD_LENGTH, MAX_SAFE_COMPONENT_LENGTH};

const LETTERDASHNUMBER: &str = "[a-zA-Z0-9-]";

// Token indices, in upstream declaration order.
pub const NUMERICIDENTIFIER: usize = 0;
pub const NUMERICIDENTIFIERLOOSE: usize = 1;
pub const NONNUMERICIDENTIFIER: usize = 2;
pub const MAINVERSION: usize = 3;
pub const MAINVERSIONLOOSE: usize = 4;
pub const PRERELEASEIDENTIFIER: usize = 5;
pub const PRERELEASEIDENTIFIERLOOSE: usize = 6;
pub const PRERELEASE: usize = 7;
pub const PRERELEASELOOSE: usize = 8;
pub const BUILDIDENTIFIER: usize = 9;
pub const BUILD: usize = 10;
pub const FULLPLAIN: usize = 11;
pub const FULL: usize = 12;
pub const LOOSEPLAIN: usize = 13;
pub const LOOSE: usize = 14;
pub const GTLT: usize = 15;
pub const XRANGEIDENTIFIERLOOSE: usize = 16;
pub const XRANGEIDENTIFIER: usize = 17;
pub const XRANGEPLAIN: usize = 18;
pub const XRANGEPLAINLOOSE: usize = 19;
pub const XRANGE: usize = 20;
pub const XRANGELOOSE: usize = 21;
pub const COERCEPLAIN: usize = 22;
pub const COERCE: usize = 23;
pub const COERCEFULL: usize = 24;
pub const COERCERTL: usize = 25;
pub const COERCERTLFULL: usize = 26;
pub const LONETILDE: usize = 27;
pub const TILDETRIM: usize = 28;
pub const TILDE: usize = 29;
pub const TILDELOOSE: usize = 30;
pub const LONECARET: usize = 31;
pub const CARETTRIM: usize = 32;
pub const CARET: usize = 33;
pub const CARETLOOSE: usize = 34;
pub const COMPARATORLOOSE: usize = 35;
pub const COMPARATOR: usize = 36;
pub const COMPARATORTRIM: usize = 37;
pub const HYPHENRANGE: usize = 38;
pub const HYPHENRANGELOOSE: usize = 39;
pub const STAR: usize = 40;
pub const GTE0: usize = 41;
pub const GTE0PRE: usize = 42;

pub const TOKEN_COUNT: usize = 43;

pub const TILDE_TRIM_REPLACE: &str = "${1}~";
pub const CARET_TRIM_REPLACE: &str = "${1}^";
pub const COMPARATOR_TRIM_REPLACE: &str = "${1}${2}${3}";

/// ECMAScript `\s`: `WhiteSpace` plus `LineTerminator`, as a character-class
/// body. Deliberately spelled out rather than delegated to Rust's `\s`, which
/// is the Unicode `White_Space` property — a set that both misses U+FEFF and
/// includes code points JS excludes.
const JS_WHITESPACE_CLASS_BODY: &str =
    r"\t\n\x0B\f\r \x{00a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}";

/// ECMAScript `\d` is exactly `[0-9]`; Rust's `\d` is Unicode `Nd`.
const JS_DIGIT_CLASS_BODY: &str = "0-9";

/// Translate a JavaScript regex source into an equivalent Rust `regex` source.
///
/// Only `\d` and `\s` differ in meaning between the two dialects for the
/// patterns used here (verified: upstream uses no lookaround, no
/// backreferences and no named groups, so nothing else needs rewriting).
///
/// The rewrite has to be character-class aware. `\s` appears both bare
/// (`^\s*>=`) and *inside* a class (`[v=\s]*`); expanding it to `[...]` in the
/// latter position would produce a nested-class syntax error, so inside a class
/// we splice in the body without brackets.
fn js_to_rust_dialect(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len() * 2);
    let mut chars = pattern.chars().peekable();
    let mut in_class = false;

    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('d') => {
                    if in_class {
                        out.push_str(JS_DIGIT_CLASS_BODY);
                    } else {
                        out.push('[');
                        out.push_str(JS_DIGIT_CLASS_BODY);
                        out.push(']');
                    }
                }
                Some('s') => {
                    if in_class {
                        out.push_str(JS_WHITESPACE_CLASS_BODY);
                    } else {
                        out.push('[');
                        out.push_str(JS_WHITESPACE_CLASS_BODY);
                        out.push(']');
                    }
                }
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            },
            '[' if !in_class => {
                in_class = true;
                out.push('[');
            }
            ']' if in_class => {
                in_class = false;
                out.push(']');
            }
            _ => out.push(c),
        }
    }
    out
}

/// Port of upstream's `makeSafeRegex`: rewrite unbounded quantifiers on the
/// three "greedy" tokens into bounded ones. Applied to the fully composed
/// source, exactly as upstream does, and always composing from the *raw* table.
fn make_safe_regex(value: &str) -> String {
    let replacements: [(&str, usize); 3] = [
        (r"\s", 1),
        (r"\d", MAX_LENGTH),
        (LETTERDASHNUMBER, MAX_SAFE_BUILD_LENGTH),
    ];
    let mut v = value.to_string();
    for (token, max) in replacements {
        v = v.replace(&format!("{token}*"), &format!("{token}{{0,{max}}}"));
        v = v.replace(&format!("{token}+"), &format!("{token}{{1,{max}}}"));
    }
    v
}

pub struct Table {
    pub names: Vec<&'static str>,
    /// Source strings exactly as upstream writes them (`exports.src`).
    pub src: Vec<String>,
    /// Source strings after `makeSafeRegex` (`exports.safeSrc`).
    pub safe_src: Vec<String>,
    pub is_global: Vec<bool>,
    /// Compiled from `safe_src`. This is what the library matches with.
    pub safe: Vec<Regex>,
    /// Compiled from `src`. Only `BUILDSTRIPRE` uses this internally.
    pub raw: Vec<Regex>,
}

impl Table {
    #[inline]
    pub fn safe(&self, token: usize) -> &Regex {
        &self.safe[token]
    }

    #[inline]
    pub fn raw(&self, token: usize) -> &Regex {
        &self.raw[token]
    }
}

struct Builder {
    names: Vec<&'static str>,
    src: Vec<String>,
    safe_src: Vec<String>,
    is_global: Vec<bool>,
}

impl Builder {
    fn new() -> Self {
        Self { names: vec![], src: vec![], safe_src: vec![], is_global: vec![] }
    }

    /// Mirrors upstream `createToken`.
    fn token(&mut self, name: &'static str, value: String, is_global: bool) -> usize {
        let safe = make_safe_regex(&value);
        let index = self.src.len();
        self.names.push(name);
        self.src.push(value);
        self.safe_src.push(safe);
        self.is_global.push(is_global);
        index
    }

    fn s(&self, i: usize) -> &str {
        &self.src[i]
    }
}

fn compile(pattern: &str) -> Regex {
    // Bounded repeats like `[0-9]{0,256}` nested several deep in XRANGEPLAIN
    // expand to a large program, well past the crate's 10 MB default.
    regex::RegexBuilder::new(&js_to_rust_dialect(pattern))
        .size_limit(64 * 1024 * 1024)
        .dfa_size_limit(32 * 1024 * 1024)
        .build()
        .unwrap_or_else(|e| panic!("failed to compile ported regex `{pattern}`: {e}"))
}

fn build() -> Table {
    let mut b = Builder::new();

    // ## Numeric Identifier
    b.token("NUMERICIDENTIFIER", r"0|[1-9]\d*".into(), false);
    b.token("NUMERICIDENTIFIERLOOSE", r"\d+".into(), false);

    // ## Non-numeric Identifier
    b.token("NONNUMERICIDENTIFIER", format!(r"\d*[a-zA-Z-]{LETTERDASHNUMBER}*"), false);

    // ## Main Version
    let mv = format!(
        r"({0})\.({0})\.({0})",
        b.s(NUMERICIDENTIFIER)
    );
    b.token("MAINVERSION", mv, false);
    let mvl = format!(
        r"({0})\.({0})\.({0})",
        b.s(NUMERICIDENTIFIERLOOSE)
    );
    b.token("MAINVERSIONLOOSE", mvl, false);

    // ## Pre-release Version Identifier
    let pri = format!("(?:{}|{})", b.s(NONNUMERICIDENTIFIER), b.s(NUMERICIDENTIFIER));
    b.token("PRERELEASEIDENTIFIER", pri, false);
    let pril = format!("(?:{}|{})", b.s(NONNUMERICIDENTIFIER), b.s(NUMERICIDENTIFIERLOOSE));
    b.token("PRERELEASEIDENTIFIERLOOSE", pril, false);

    // ## Pre-release Version
    let pr = format!(r"(?:-({0}(?:\.{0})*))", b.s(PRERELEASEIDENTIFIER));
    b.token("PRERELEASE", pr, false);
    let prl = format!(r"(?:-?({0}(?:\.{0})*))", b.s(PRERELEASEIDENTIFIERLOOSE));
    b.token("PRERELEASELOOSE", prl, false);

    // ## Build Metadata
    b.token("BUILDIDENTIFIER", format!("{LETTERDASHNUMBER}+"), false);
    let bld = format!(r"(?:\+({0}(?:\.{0})*))", b.s(BUILDIDENTIFIER));
    b.token("BUILD", bld, false);

    // ## Full Version String
    let fullplain = format!(
        "v?{}{}?{}?",
        b.s(MAINVERSION),
        b.s(PRERELEASE),
        b.s(BUILD)
    );
    b.token("FULLPLAIN", fullplain, false);
    let full = format!("^{}$", b.s(FULLPLAIN));
    b.token("FULL", full, false);

    // Like full, but allows `v1.2.3`, `=1.2.3` and `1.0.0alpha1`.
    let looseplain = format!(
        r"[v=\s]*{}{}?{}?",
        b.s(MAINVERSIONLOOSE),
        b.s(PRERELEASELOOSE),
        b.s(BUILD)
    );
    b.token("LOOSEPLAIN", looseplain, false);
    let loose = format!("^{}$", b.s(LOOSEPLAIN));
    b.token("LOOSE", loose, false);

    b.token("GTLT", "((?:<|>)?=?)".into(), false);

    // X-ranges: "2.*", "1.2.x". "x.x" is valid and means "any version".
    let xidl = format!(r"{}|x|X|\*", b.s(NUMERICIDENTIFIERLOOSE));
    b.token("XRANGEIDENTIFIERLOOSE", xidl, false);
    let xid = format!(r"{}|x|X|\*", b.s(NUMERICIDENTIFIER));
    b.token("XRANGEIDENTIFIER", xid, false);

    let xrp = format!(
        r"[v=\s]*({0})(?:\.({0})(?:\.({0})(?:{1})?{2}?)?)?",
        b.s(XRANGEIDENTIFIER),
        b.s(PRERELEASE),
        b.s(BUILD)
    );
    b.token("XRANGEPLAIN", xrp, false);
    let xrpl = format!(
        r"[v=\s]*({0})(?:\.({0})(?:\.({0})(?:{1})?{2}?)?)?",
        b.s(XRANGEIDENTIFIERLOOSE),
        b.s(PRERELEASELOOSE),
        b.s(BUILD)
    );
    b.token("XRANGEPLAINLOOSE", xrpl, false);

    let xr = format!(r"^{}\s*{}$", b.s(GTLT), b.s(XRANGEPLAIN));
    b.token("XRANGE", xr, false);
    let xrl = format!(r"^{}\s*{}$", b.s(GTLT), b.s(XRANGEPLAINLOOSE));
    b.token("XRANGELOOSE", xrl, false);

    // Coercion: extract anything that could conceivably be part of a semver.
    let cp = format!(
        r"(^|[^\d])(\d{{1,{0}}})(?:\.(\d{{1,{0}}}))?(?:\.(\d{{1,{0}}}))?",
        MAX_SAFE_COMPONENT_LENGTH
    );
    b.token("COERCEPLAIN", cp, false);
    let co = format!(r"{}(?:$|[^\d])", b.s(COERCEPLAIN));
    b.token("COERCE", co, false);
    let cf = format!(
        r"{}(?:{})?(?:{})?(?:$|[^\d])",
        b.s(COERCEPLAIN),
        b.s(PRERELEASE),
        b.s(BUILD)
    );
    b.token("COERCEFULL", cf, false);
    let crtl = b.s(COERCE).to_string();
    b.token("COERCERTL", crtl, true);
    let crtlf = b.s(COERCEFULL).to_string();
    b.token("COERCERTLFULL", crtlf, true);

    // Tilde ranges: "reasonably at or greater than".
    b.token("LONETILDE", "(?:~>?)".into(), false);
    let tt = format!(r"(\s*){}\s+", b.s(LONETILDE));
    b.token("TILDETRIM", tt, true);
    let td = format!("^{}{}$", b.s(LONETILDE), b.s(XRANGEPLAIN));
    b.token("TILDE", td, false);
    let tdl = format!("^{}{}$", b.s(LONETILDE), b.s(XRANGEPLAINLOOSE));
    b.token("TILDELOOSE", tdl, false);

    // Caret ranges: "at least and backwards compatible with".
    b.token("LONECARET", r"(?:\^)".into(), false);
    let ct = format!(r"(\s*){}\s+", b.s(LONECARET));
    b.token("CARETTRIM", ct, true);
    let cr = format!("^{}{}$", b.s(LONECARET), b.s(XRANGEPLAIN));
    b.token("CARET", cr, false);
    let crl = format!("^{}{}$", b.s(LONECARET), b.s(XRANGEPLAINLOOSE));
    b.token("CARETLOOSE", crl, false);

    // A simple gt/lt/eq thing, or just "" to indicate "any version".
    let cmpl = format!(r"^{}\s*({})$|^$", b.s(GTLT), b.s(LOOSEPLAIN));
    b.token("COMPARATORLOOSE", cmpl, false);
    let cmp = format!(r"^{}\s*({})$|^$", b.s(GTLT), b.s(FULLPLAIN));
    b.token("COMPARATOR", cmp, false);

    // Strips whitespace between a gtlt and what it modifies: `> 1.2.3` -> `>1.2.3`.
    let cmpt = format!(
        r"(\s*){}\s*({}|{})",
        b.s(GTLT),
        b.s(LOOSEPLAIN),
        b.s(XRANGEPLAIN)
    );
    b.token("COMPARATORTRIM", cmpt, true);

    // Hyphen ranges: `1.2.3 - 1.2.4`. Always built from the loose-tolerant
    // XRANGEPLAIN forms because they get re-checked strictly later.
    let hr = format!(
        r"^\s*({0})\s+-\s+({0})\s*$",
        b.s(XRANGEPLAIN)
    );
    b.token("HYPHENRANGE", hr, false);
    let hrl = format!(
        r"^\s*({0})\s+-\s+({0})\s*$",
        b.s(XRANGEPLAINLOOSE)
    );
    b.token("HYPHENRANGELOOSE", hrl, false);

    // Star ranges allow anything at all.
    b.token("STAR", r"(<|>)?=?\s*\*".into(), false);
    // `>=0.0.0` is like a star.
    b.token("GTE0", r"^\s*>=\s*0\.0\.0\s*$".into(), false);
    b.token("GTE0PRE", r"^\s*>=\s*0\.0\.0-0\s*$".into(), false);

    debug_assert_eq!(b.src.len(), TOKEN_COUNT);

    let safe = b.safe_src.iter().map(|p| compile(p)).collect();
    let raw = b.src.iter().map(|p| compile(p)).collect();

    Table { names: b.names, src: b.src, safe_src: b.safe_src, is_global: b.is_global, safe, raw }
}

pub static RE: LazyLock<Table> = LazyLock::new(build);

/// JavaScript `/\s+/` — the *unbounded* form. `classes/range.js` uses this
/// directly for whitespace collapsing and splitting, rather than the bounded
/// `safeRe` variants, so the port does too.
pub static SPACE_CHARACTERS: LazyLock<Regex> = LazyLock::new(|| compile(r"\s+"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialect_translation_is_class_aware() {
        // Bare \d becomes a bracketed class...
        assert_eq!(js_to_rust_dialect(r"\d+"), "[0-9]+");
        // ...but inside an existing class it must splice, not nest.
        assert_eq!(js_to_rust_dialect(r"[v=\s]*"), format!("[v={JS_WHITESPACE_CLASS_BODY}]*"));
        // Escapes that are not \d or \s pass through untouched.
        assert_eq!(js_to_rust_dialect(r"\.\+\*"), r"\.\+\*");
    }

    #[test]
    fn make_safe_regex_bounds_quantifiers() {
        assert_eq!(make_safe_regex(r"\s*"), r"\s{0,1}");
        assert_eq!(make_safe_regex(r"\s+"), r"\s{1,1}");
        assert_eq!(make_safe_regex(r"\d+"), r"\d{1,256}");
        assert_eq!(make_safe_regex("[a-zA-Z0-9-]*"), "[a-zA-Z0-9-]{0,250}");
    }

    #[test]
    fn every_token_compiles() {
        assert_eq!(RE.safe.len(), TOKEN_COUNT);
        assert_eq!(RE.raw.len(), TOKEN_COUNT);
    }
}
