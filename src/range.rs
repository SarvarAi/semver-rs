//! Port of `classes/range.js`.
//!
//! A range is a set of comparator sets: the outer list is OR (`||`), each inner
//! list is AND. Getting there means running the raw string through upstream's
//! exact replacement pipeline — hyphen ranges, then comparator/tilde/caret
//! whitespace trimming, then caret, tilde, X-range and star desugaring — in that
//! order, because each stage assumes the previous one has run.

use std::cell::RefCell;
use std::sync::Arc;

use regex::Captures;

use crate::comparator::Comparator;
use crate::lrucache::LruCache;
use crate::options::Options;
use crate::re::{self, RE};
use crate::semver::{js_trim, SemVer};
use crate::util::{js_increment, js_to_number};
use crate::{Error, Result};

thread_local! {
    /// Mirrors upstream's module-level `const cache = new LRU()`. Range parsing
    /// is a hot and fully deterministic path.
    static RANGE_CACHE: RefCell<LruCache<Arc<Vec<Comparator>>>> = RefCell::new(LruCache::new());
}

/// JavaScript `s.replace(/\s+/g, ' ')`.
pub fn js_collapse_whitespace(s: &str) -> String {
    re::SPACE_CHARACTERS.replace_all(s, " ").into_owned()
}

/// JavaScript `s.split(/\s+/)`, which — unlike Rust's `split_whitespace` —
/// yields leading/trailing empty strings when the input has edge whitespace.
fn js_split_ws(s: &str) -> Vec<&str> {
    re::SPACE_CHARACTERS.split(s).collect()
}

#[derive(Debug, Clone)]
pub struct Range {
    pub raw: String,
    /// Outer list is `||`; inner list is AND.
    ///
    /// Shared via `Arc` because `parse_range` is memoized and upstream hands
    /// back the cached array *by reference*. Deep-cloning it on every cache hit
    /// made `Range` construction measurably slower than the original.
    pub set: Vec<Arc<Vec<Comparator>>>,
    pub options: Options,
    pub loose: bool,
    pub include_prerelease: bool,
    formatted: String,
}

fn is_null_set(c: &Comparator) -> bool {
    c.value == "<0.0.0-0"
}

fn is_any(c: &Comparator) -> bool {
    c.value.is_empty()
}

impl Range {
    pub fn new(range: &str, options: impl Into<Options>) -> Result<Self> {
        let options = options.into();

        // Collapse whitespace up front so the pipeline never has to rely on
        // potentially slow `\s*` matching. Upstream keeps this normalized form
        // as `raw`, including for error messages.
        let raw = js_collapse_whitespace(js_trim(range));

        let mut set: Vec<Arc<Vec<Comparator>>> = Vec::new();
        for part in raw.split("||") {
            let comps = Self::parse_range(js_trim(part), options)?;
            // Empty comparator lists mean the segment was not a valid range,
            // which is allowed in loose mode; a wholly invalid range still throws.
            if !comps.is_empty() {
                set.push(comps);
            }
        }

        if set.is_empty() {
            return Err(Error::new(format!("Invalid SemVer Range: {raw}")));
        }

        if set.len() > 1 {
            // Throw out null sets, but keep the first in case they all are.
            let first = set[0].clone();
            set.retain(|c| !is_null_set(&c[0]));
            if set.is_empty() {
                set = vec![first];
            } else if set.len() > 1 {
                // If any set is `*`, the whole range is `*`.
                if let Some(any) = set.iter().find(|c| c.len() == 1 && is_any(&c[0])) {
                    let any = any.clone();
                    set = vec![any];
                }
            }
        }

        let mut r = Range {
            raw,
            set,
            options,
            loose: options.loose,
            include_prerelease: options.include_prerelease,
            formatted: String::new(),
        };
        r.formatted = r.compute_format();
        Ok(r)
    }

    /// Port of the `range` getter.
    fn compute_format(&self) -> String {
        let mut out = String::new();
        for (i, comps) in self.set.iter().enumerate() {
            if i > 0 {
                out.push_str("||");
            }
            for (k, c) in comps.iter().enumerate() {
                if k > 0 {
                    out.push(' ');
                }
                out.push_str(c.value.trim());
            }
        }
        out
    }

    /// The canonical range string, e.g. `>=1.2.3 <2.0.0-0`.
    pub fn range(&self) -> &str {
        &self.formatted
    }

    fn parse_range(range: &str, options: Options) -> Result<Arc<Vec<Comparator>>> {
        // Strip build metadata so it cannot bleed into the version. Upstream
        // deliberately uses the *unbounded* BUILD source with a global flag here.
        let range = RE.raw(re::BUILD).replace_all(range, "").into_owned();

        let memo_key = format!("{}:{}", options.flags(), range);
        if let Some(hit) = RANGE_CACHE.with(|c| c.borrow_mut().get(&memo_key)) {
            return Ok(hit);
        }

        let loose = options.loose;

        // `1.2.3 - 1.2.4` => `>=1.2.3 <=1.2.4`  (non-global: first match only)
        let hr = if loose { re::HYPHENRANGELOOSE } else { re::HYPHENRANGE };
        let range = RE
            .safe(hr)
            .replace(&range, |caps: &Captures| hyphen_replace(caps, options.include_prerelease))
            .into_owned();

        // `> 1.2.3 < 1.2.5` => `>1.2.3 <1.2.5`
        let range = RE
            .safe(re::COMPARATORTRIM)
            .replace_all(&range, re::COMPARATOR_TRIM_REPLACE)
            .into_owned();

        // `~ 1.2.3` => `~1.2.3`
        let range = RE
            .safe(re::TILDETRIM)
            .replace_all(&range, re::TILDE_TRIM_REPLACE)
            .into_owned();

        // `^ 1.2.3` => `^1.2.3`
        let range = RE
            .safe(re::CARETTRIM)
            .replace_all(&range, re::CARET_TRIM_REPLACE)
            .into_owned();

        // Fully trimmed now; split into comparators.
        let joined = range
            .split(' ')
            .map(|comp| parse_comparator(comp, options))
            .collect::<Vec<_>>()
            .join(" ");

        let mut range_list: Vec<String> = js_split_ws(&joined)
            .into_iter()
            .map(|comp| replace_gte0(comp, options))
            .collect();

        if loose {
            // In loose mode, throw out anything that is not a valid comparator.
            range_list.retain(|comp| RE.safe(re::COMPARATORLOOSE).is_match(comp));
        }

        // Upstream builds the whole comparator array with `.map()` *before*
        // scanning it for the null set, so an invalid comparator later in the
        // list still throws even when an earlier one was `<0.0.0-0`. Collecting
        // first — rather than short-circuiting inside the loop — preserves that
        // ordering. The differential fuzzer caught the difference.
        let comparators: Vec<Comparator> = range_list
            .iter()
            .map(|comp| Comparator::new(comp, options))
            .collect::<Result<Vec<_>>>()?;

        // Dedupe by comparator value, preserving first-insertion order (JS Map).
        let mut ordered: Vec<(String, Comparator)> = Vec::new();
        for c in comparators {
            if is_null_set(&c) {
                return Ok(Arc::new(vec![c]));
            }
            match ordered.iter_mut().find(|(k, _)| *k == c.value) {
                Some(slot) => slot.1 = c,
                None => ordered.push((c.value.clone(), c)),
            }
        }
        if ordered.len() > 1 {
            ordered.retain(|(k, _)| !k.is_empty());
        }

        let result: Arc<Vec<Comparator>> =
            Arc::new(ordered.into_iter().map(|(_, c)| c).collect());
        RANGE_CACHE.with(|c| c.borrow_mut().set(memo_key, Arc::clone(&result)));
        Ok(result)
    }

    /// Port of `test(version)` for a parsed version.
    ///
    /// Fallible for the same reason `Comparator::test` is: a version whose
    /// flags differ from this range's gets re-parsed inside `cmp`, and that
    /// re-parse can throw.
    pub fn test(&self, version: &SemVer) -> Result<bool> {
        for s in &self.set {
            if test_set(s, version, self.options)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Port of `test(version)` for a string. An empty or unparseable version is
    /// `false`, matching upstream's `if (!version) return false` plus its
    /// swallowed constructor error.
    pub fn test_str(&self, version: &str) -> Result<bool> {
        if version.is_empty() {
            return Ok(false);
        }
        match SemVer::new(version, self.options) {
            Ok(v) => self.test(&v),
            Err(_) => Ok(false),
        }
    }

    /// Port of `intersects(range, options)`.
    pub fn intersects(&self, other: &Range, options: impl Into<Options>) -> Result<bool> {
        let options = options.into();
        for this_comparators in &self.set {
            if !is_satisfiable(this_comparators, options)? {
                continue;
            }
            for range_comparators in &other.set {
                if !is_satisfiable(range_comparators, options)? {
                    continue;
                }
                let mut all = true;
                'outer: for tc in this_comparators.iter() {
                    for rc in range_comparators.iter() {
                        if !tc.intersects(rc, options)? {
                            all = false;
                            break 'outer;
                        }
                    }
                }
                if all {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

impl std::fmt::Display for Range {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.formatted)
    }
}

/// Port of the module-private `isSatisfiable`.
fn is_satisfiable(comparators: &[Comparator], options: Options) -> Result<bool> {
    let mut remaining = comparators.to_vec();
    let Some(mut test_comparator) = remaining.pop() else {
        return Ok(true);
    };
    let mut result = true;
    while result && !remaining.is_empty() {
        for other in &remaining {
            if !test_comparator.intersects(other, options)? {
                result = false;
                break;
            }
        }
        match remaining.pop() {
            Some(c) => test_comparator = c,
            None => break,
        }
    }
    Ok(result)
}

/// Port of `parseComparator`: caret, then tilde, then X-range, then star.
fn parse_comparator(comp: &str, options: Options) -> String {
    // Non-global: strips the first build-metadata occurrence only.
    let comp = RE.safe(re::BUILD).replace(comp, "").into_owned();
    let comp = replace_carets(&comp, options);
    let comp = replace_tildes(&comp, options);
    let comp = replace_xranges(&comp, options);
    replace_stars(&comp)
}

fn is_x(id: &str) -> bool {
    id.is_empty() || id.eq_ignore_ascii_case("x") || id == "*"
}

fn invalid_xrange_order(m: &str, mi: &str, p: &str) -> bool {
    (is_x(m) && !is_x(mi)) || (is_x(mi) && !p.is_empty() && !is_x(p))
}

/// Group accessor that mirrors JS: an unmatched group is `undefined`, which
/// these replacers treat the same as an empty string via `isX`.
fn g<'a>(caps: &Captures<'a>, i: usize) -> &'a str {
    caps.get(i).map_or("", |m| m.as_str())
}

// ---------------------------------------------------------------------------
// tilde
// ---------------------------------------------------------------------------

fn replace_tildes(comp: &str, options: Options) -> String {
    js_split_ws(js_trim(comp))
        .into_iter()
        .map(|c| replace_tilde(c, options))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `~1.2.3` => `>=1.2.3 <1.3.0-0`
fn replace_tilde(comp: &str, options: Options) -> String {
    let token = if options.loose { re::TILDELOOSE } else { re::TILDE };
    // With includePrerelease the lower bound is `-0`, keeping `~1.2` equivalent
    // to the `1.2.x` X-range it is documented as.
    let z = if options.include_prerelease { "-0" } else { "" };
    RE.safe(token)
        .replace(comp, |caps: &Captures| {
            let (m, mi, p, pr) = (g(caps, 1), g(caps, 2), g(caps, 3), g(caps, 4));
            if is_x(m) {
                String::new()
            } else if is_x(mi) {
                format!(">={m}.0.0{z} <{}.0.0-0", js_increment(m))
            } else if is_x(p) {
                format!(">={m}.{mi}.0{z} <{m}.{}.0-0", js_increment(mi))
            } else if !pr.is_empty() {
                format!(">={m}.{mi}.{p}-{pr} <{m}.{}.0-0", js_increment(mi))
            } else {
                format!(">={m}.{mi}.{p} <{m}.{}.0-0", js_increment(mi))
            }
        })
        .into_owned()
}

// ---------------------------------------------------------------------------
// caret
// ---------------------------------------------------------------------------

fn replace_carets(comp: &str, options: Options) -> String {
    js_split_ws(js_trim(comp))
        .into_iter()
        .map(|c| replace_caret(c, options))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `^1.2.3` => `>=1.2.3 <2.0.0-0`, with the `0.x` and `0.0.x` special cases.
fn replace_caret(comp: &str, options: Options) -> String {
    let token = if options.loose { re::CARETLOOSE } else { re::CARET };
    let z = if options.include_prerelease { "-0" } else { "" };
    RE.safe(token)
        .replace(comp, |caps: &Captures| {
            let (m, mi, p, pr) = (g(caps, 1), g(caps, 2), g(caps, 3), g(caps, 4));
            if is_x(m) {
                String::new()
            } else if is_x(mi) {
                format!(">={m}.0.0{z} <{}.0.0-0", js_increment(m))
            } else if is_x(p) {
                if m == "0" {
                    format!(">={m}.{mi}.0{z} <{m}.{}.0-0", js_increment(mi))
                } else {
                    format!(">={m}.{mi}.0{z} <{}.0.0-0", js_increment(m))
                }
            } else if !pr.is_empty() {
                if m == "0" {
                    if mi == "0" {
                        format!(">={m}.{mi}.{p}-{pr} <{m}.{mi}.{}-0", js_increment(p))
                    } else {
                        format!(">={m}.{mi}.{p}-{pr} <{m}.{}.0-0", js_increment(mi))
                    }
                } else {
                    format!(">={m}.{mi}.{p}-{pr} <{}.0.0-0", js_increment(m))
                }
            } else if m == "0" {
                if mi == "0" {
                    format!(">={m}.{mi}.{p} <{m}.{mi}.{}-0", js_increment(p))
                } else {
                    format!(">={m}.{mi}.{p} <{m}.{}.0-0", js_increment(mi))
                }
            } else {
                format!(">={m}.{mi}.{p} <{}.0.0-0", js_increment(m))
            }
        })
        .into_owned()
}

// ---------------------------------------------------------------------------
// X-ranges
// ---------------------------------------------------------------------------

/// Note: no `trim()` here, unlike the tilde and caret variants. Faithful to
/// upstream, which splits the raw string.
fn replace_xranges(comp: &str, options: Options) -> String {
    js_split_ws(comp)
        .into_iter()
        .map(|c| replace_xrange(c, options))
        .collect::<Vec<_>>()
        .join(" ")
}

fn replace_xrange(comp: &str, options: Options) -> String {
    let comp = js_trim(comp);
    let token = if options.loose { re::XRANGELOOSE } else { re::XRANGE };
    RE.safe(token)
        .replace(comp, |caps: &Captures| {
            let mut gtlt = g(caps, 1).to_string();
            let mut m = g(caps, 2).to_string();
            let mut mi = g(caps, 3).to_string();
            let mut p = g(caps, 4).to_string();

            if invalid_xrange_order(&m, &mi, &p) {
                return comp.to_string();
            }

            let x_major = is_x(&m);
            let x_minor = x_major || is_x(&mi);
            let x_patch = x_minor || is_x(&p);
            let any_x = x_patch;

            if gtlt == "=" && any_x {
                gtlt.clear();
            }

            // With includePrerelease the bound needs `-0`, the lowest prerelease.
            let mut pr = if options.include_prerelease { "-0" } else { "" };

            if x_major {
                return if gtlt == ">" || gtlt == "<" {
                    // Nothing is allowed.
                    "<0.0.0-0".to_string()
                } else {
                    // Nothing is forbidden.
                    "*".to_string()
                };
            }

            if !gtlt.is_empty() && any_x {
                // Patch is an x, because we have any x at all.
                if x_minor {
                    mi = "0".into();
                }
                p = "0".into();

                if gtlt == ">" {
                    // `>1` => `>=2.0.0`, `>1.2` => `>=1.3.0`
                    gtlt = ">=".into();
                    if x_minor {
                        m = js_increment(&m);
                        mi = "0".into();
                        p = "0".into();
                    } else {
                        mi = js_increment(&mi);
                        p = "0".into();
                    }
                } else if gtlt == "<=" {
                    // `<=0.7.x` is really `<0.8.0`.
                    gtlt = "<".into();
                    if x_minor {
                        m = js_increment(&m);
                    } else {
                        mi = js_increment(&mi);
                    }
                }

                if gtlt == "<" {
                    pr = "-0";
                }

                return format!("{gtlt}{m}.{mi}.{p}{pr}");
            } else if x_minor {
                return format!(">={m}.0.0{pr} <{}.0.0-0", js_increment(&m));
            } else if x_patch {
                return format!(">={m}.{mi}.0{pr} <{m}.{}.0-0", js_increment(&mi));
            }

            // No x at all: leave the whole match untouched.
            g(caps, 0).to_string()
        })
        .into_owned()
}

/// `*` is AND-ed with everything, and `""` already means "any", so drop it.
fn replace_stars(comp: &str) -> String {
    RE.safe(re::STAR).replace(js_trim(comp), "").into_owned()
}

/// `>=0.0.0` is equivalent to `*`.
fn replace_gte0(comp: &str, options: Options) -> String {
    let token = if options.include_prerelease { re::GTE0PRE } else { re::GTE0 };
    RE.safe(token).replace(js_trim(comp), "").into_owned()
}

// ---------------------------------------------------------------------------
// hyphen ranges
// ---------------------------------------------------------------------------

/// Port of `hyphenReplace`.
///
/// Group layout, since XRANGEPLAIN contributes five groups per side and each
/// side is additionally wrapped: 1=from 2=fM 3=fm 4=fp 5=fpr 6=fbuild
/// 7=to 8=tM 9=tm 10=tp 11=tpr (12=tbuild, unused — upstream ignores it too).
fn hyphen_replace(caps: &Captures, inc_pr: bool) -> String {
    let (from, f_major, f_minor, f_patch, f_pre) =
        (g(caps, 1), g(caps, 2), g(caps, 3), g(caps, 4), g(caps, 5));
    let (to, t_major, t_minor, t_patch, t_pre) =
        (g(caps, 7), g(caps, 8), g(caps, 9), g(caps, 10), g(caps, 11));

    let z = if inc_pr { "-0" } else { "" };

    let from = if is_x(f_major) {
        String::new()
    } else if is_x(f_minor) {
        format!(">={f_major}.0.0{z}")
    } else if is_x(f_patch) {
        format!(">={f_major}.{f_minor}.0{z}")
    } else if !f_pre.is_empty() {
        format!(">={from}")
    } else {
        format!(">={from}{z}")
    };

    let to = if is_x(t_major) {
        String::new()
    } else if is_x(t_minor) {
        format!("<{}.0.0-0", js_increment(t_major))
    } else if is_x(t_patch) {
        format!("<{t_major}.{}.0-0", js_increment(t_minor))
    } else if !t_pre.is_empty() {
        format!("<={t_major}.{t_minor}.{t_patch}-{t_pre}")
    } else if inc_pr {
        format!("<{t_major}.{t_minor}.{}-0", js_increment(t_patch))
    } else {
        format!("<={to}")
    };

    format!("{from} {to}").trim().to_string()
}

// ---------------------------------------------------------------------------
// test
// ---------------------------------------------------------------------------

/// Port of `testSet`.
fn test_set(set: &[Comparator], version: &SemVer, options: Options) -> Result<bool> {
    for c in set {
        if !c.test(version)? {
            return Ok(false);
        }
    }

    if !version.prerelease.is_empty() && !options.include_prerelease {
        // `^1.2.3-pr.1` desugars to `>=1.2.3-pr.1 <2.0.0`, which should admit
        // `1.2.3-pr.2` but NOT `1.2.4-alpha.notready`. So a prerelease version
        // only passes if some comparator pins the same major.minor.patch and
        // itself carries a prerelease.
        for c in set {
            let Some(allowed) = &c.semver else {
                continue;
            };
            if !allowed.prerelease.is_empty()
                && allowed.major == version.major
                && allowed.minor == version.minor
                && allowed.patch == version.patch
            {
                return Ok(true);
            }
        }
        return Ok(false);
    }

    Ok(true)
}

/// Port of `ranges/valid.js` — `validRange`.
pub fn valid_range(range: &str, options: impl Into<Options>) -> Option<String> {
    // `'*'` rather than `''` so that truthiness works, matching upstream.
    let r = Range::new(range, options).ok()?;
    Some(if r.range().is_empty() { "*".to_string() } else { r.range().to_string() })
}

/// Port of `ranges/to-comparators.js`.
pub fn to_comparators(range: &str, options: impl Into<Options>) -> Result<Vec<Vec<String>>> {
    let r = Range::new(range, options)?;
    Ok(r.set
        .iter()
        .map(|comps| {
            comps
                .iter()
                .map(|c| c.value.clone())
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .split(' ')
                .map(str::to_string)
                .collect()
        })
        .collect())
}

/// Port of `functions/satisfies.js`.
/// Upstream lets a `test` error escape `satisfies`, but that branch is
/// unreachable from a string version: the version is constructed with the
/// range's own options, so the flags always match and no re-parse happens.
pub fn satisfies(version: &str, range: &str, options: impl Into<Options> + Copy) -> bool {
    match Range::new(range, options) {
        Ok(r) => r.test_str(version).unwrap_or(false),
        Err(_) => false,
    }
}

/// `js_to_number` is used by the X-range order checks; re-exported for tests.
#[doc(hidden)]
pub fn _js_to_number(s: &str) -> f64 {
    js_to_number(s)
}
