//! Port of the remaining functions under `ranges/`.

use crate::comparator::Comparator;
use crate::identifiers::Identifier;
use crate::options::Options;
use crate::range::Range;
use crate::semver::SemVer;
use crate::{Error, Result};

/// Port of `outside()`.
///
/// `hilo` is `">"` (is the version above everything the range allows?) or
/// `"<"`. Upstream implements `<` by swapping the comparison functions and
/// reading the rest of the body as if it were in `>` mode; this keeps that
/// structure so the two stay in sync.
pub fn outside(
    version: &str,
    range: &str,
    hilo: &str,
    options: impl Into<Options> + Copy,
) -> Result<bool> {
    let options = options.into();
    let version = SemVer::new(version, options)?;
    let range = Range::new(range, options)?;

    // (gt, lte, lt) in ">" mode; (lt, gte, gt) in "<" mode.
    let (gtfn, ltefn, ltfn, comp, ecomp): (
        fn(&SemVer, &SemVer) -> bool,
        fn(&SemVer, &SemVer) -> bool,
        fn(&SemVer, &SemVer) -> bool,
        &str,
        &str,
    ) = match hilo {
        ">" => (
            |a, b| a.compare(b) > 0,
            |a, b| a.compare(b) <= 0,
            |a, b| a.compare(b) < 0,
            ">",
            ">=",
        ),
        "<" => (
            |a, b| a.compare(b) < 0,
            |a, b| a.compare(b) >= 0,
            |a, b| a.compare(b) > 0,
            "<",
            "<=",
        ),
        _ => return Err(Error::new("Must provide a hilo val of \"<\" or \">\"")),
    };

    // If it satisfies the range it is not outside.
    if range.test(&version) {
        return Ok(false);
    }

    let any_fallback = Comparator::new(">=0.0.0", Options::new())?;

    for comparators in &range.set {
        let mut high: Option<&Comparator> = None;
        let mut low: Option<&Comparator> = None;

        for c in comparators {
            let c = if c.is_any() { &any_fallback } else { c };
            if high.is_none() {
                high = Some(c);
            }
            if low.is_none() {
                low = Some(c);
            }
            let (Some(h), Some(l)) = (high, low) else { continue };
            let (Some(cs), Some(hs), Some(ls)) = (&c.semver, &h.semver, &l.semver) else {
                continue;
            };
            if gtfn(cs, hs) {
                high = Some(c);
            } else if ltfn(cs, ls) {
                low = Some(c);
            }
        }

        let (Some(high), Some(low)) = (high, low) else { continue };

        // If the edge comparator carries the operator, the version is not outside it.
        if high.operator == comp || high.operator == ecomp {
            return Ok(false);
        }

        let Some(low_semver) = &low.semver else { continue };
        if (low.operator.is_empty() || low.operator == comp) && ltefn(&version, low_semver) {
            return Ok(false);
        } else if low.operator == ecomp && ltfn(&version, low_semver) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Port of `gtr()` — is `version` greater than every version the range allows?
pub fn gtr(version: &str, range: &str, options: impl Into<Options> + Copy) -> Result<bool> {
    outside(version, range, ">", options)
}

/// Port of `ltr()` — is `version` less than every version the range allows?
pub fn ltr(version: &str, range: &str, options: impl Into<Options> + Copy) -> Result<bool> {
    outside(version, range, "<", options)
}

/// Port of `minVersion()` — the lowest version that can satisfy the range.
pub fn min_version(range: &str, options: impl Into<Options> + Copy) -> Result<Option<SemVer>> {
    let range = Range::new(range, options)?;

    let zero = SemVer::new("0.0.0", Options::new())?;
    if range.test(&zero) {
        return Ok(Some(zero));
    }
    let zero_pre = SemVer::new("0.0.0-0", Options::new())?;
    if range.test(&zero_pre) {
        return Ok(Some(zero_pre));
    }

    let mut minver: Option<SemVer> = None;
    for comparators in &range.set {
        let mut set_min: Option<SemVer> = None;
        for c in comparators {
            let Some(cs) = &c.semver else { continue };
            // Clone so the comparator's own version is never mutated.
            let mut compver = SemVer::new(&cs.version, Options::new())?;
            match c.operator.as_str() {
                ">" | "" | ">=" => {
                    if c.operator == ">" {
                        if compver.prerelease.is_empty() {
                            compver.patch += 1.0;
                        } else {
                            compver.prerelease.push(Identifier::Num(0.0));
                        }
                        compver.format();
                        compver.raw = compver.version.clone();
                    }
                    if set_min.as_ref().map_or(true, |m| compver.compare(m) > 0) {
                        set_min = Some(compver);
                    }
                }
                // Ignore maximum versions.
                "<" | "<=" => {}
                other => {
                    return Err(Error::new(format!("Unexpected operation: {other}")));
                }
            }
        }
        if let Some(sm) = set_min {
            if minver.as_ref().map_or(true, |m| m.compare(&sm) > 0) {
                minver = Some(sm);
            }
        }
    }

    Ok(match minver {
        Some(m) if range.test(&m) => Some(m),
        _ => None,
    })
}

/// Port of `minSatisfying()`.
pub fn min_satisfying(
    versions: &[String],
    range: &str,
    options: impl Into<Options> + Copy,
) -> Option<String> {
    let range_obj = Range::new(range, options).ok()?;
    let mut min: Option<(String, SemVer)> = None;
    for v in versions {
        if !range_obj.test_str(v) {
            continue;
        }
        let Ok(sv) = SemVer::new(v, options) else { continue };
        match &min {
            None => min = Some((v.clone(), sv)),
            Some((_, cur)) if cur.compare(&sv) == 1 => min = Some((v.clone(), sv)),
            _ => {}
        }
    }
    min.map(|(s, _)| s)
}

/// Port of `maxSatisfying()`.
pub fn max_satisfying(
    versions: &[String],
    range: &str,
    options: impl Into<Options> + Copy,
) -> Option<String> {
    let range_obj = Range::new(range, options).ok()?;
    let mut max: Option<(String, SemVer)> = None;
    for v in versions {
        if !range_obj.test_str(v) {
            continue;
        }
        let Ok(sv) = SemVer::new(v, options) else { continue };
        match &max {
            None => max = Some((v.clone(), sv)),
            Some((_, cur)) if cur.compare(&sv) == -1 => max = Some((v.clone(), sv)),
            _ => {}
        }
    }
    max.map(|(s, _)| s)
}

/// Port of `intersects()`.
pub fn intersects(r1: &str, r2: &str, options: impl Into<Options> + Copy) -> Result<bool> {
    let a = Range::new(r1, options)?;
    let b = Range::new(r2, options)?;
    a.intersects(&b, options)
}

/// Port of `simplifyRange()`.
///
/// Collapses a sorted version list into the shortest range that admits exactly
/// the same members — and returns the *original* range string if simplifying
/// did not actually make it shorter.
pub fn simplify_range(
    versions: &[String],
    range: &str,
    options: impl Into<Options> + Copy,
) -> Result<String> {
    let mut v: Vec<String> = versions.to_vec();

    // Upstream sorts with `compare`, which throws on a bad version — but
    // `Array.prototype.sort` never invokes the comparator for a list of fewer
    // than two elements, so `simplifyRange(['nonsense'], '*')` does NOT throw
    // upstream. Reproduce that boundary exactly.
    if v.len() >= 2 {
        let mut parsed: Vec<(String, SemVer)> = Vec::with_capacity(v.len());
        for s in &v {
            parsed.push((s.clone(), SemVer::new(s, options)?));
        }
        parsed.sort_by(|a, b| a.1.compare(&b.1).cmp(&0));
        v = parsed.into_iter().map(|(s, _)| s).collect();
    }

    let mut set: Vec<(String, Option<String>)> = Vec::new();
    let mut first: Option<String> = None;
    let mut prev: Option<String> = None;

    for version in &v {
        // Upstream calls `satisfies`, which swallows both an invalid version
        // and an invalid range into `false`. It never constructs a Range here,
        // so a malformed range is not an error either.
        if crate::range::satisfies(version, range, options) {
            prev = Some(version.clone());
            if first.is_none() {
                first = Some(version.clone());
            }
        } else {
            if let (Some(f), Some(p)) = (&first, &prev) {
                set.push((f.clone(), Some(p.clone())));
            }
            prev = None;
            first = None;
        }
    }
    if let Some(f) = &first {
        set.push((f.clone(), None));
    }

    let mut ranges: Vec<String> = Vec::new();
    for (min, max) in &set {
        match max {
            Some(mx) if mx == min => ranges.push(min.clone()),
            None if Some(min) == v.first() => ranges.push("*".into()),
            None => ranges.push(format!(">={min}")),
            Some(mx) if Some(min) == v.first() => ranges.push(format!("<={mx}")),
            Some(mx) => ranges.push(format!("{min} - {mx}")),
        }
    }

    // Upstream compares against `String(range)` for a string argument — the
    // input as given, not the whitespace-normalized `Range.raw` — and returns
    // that same original when simplifying did not shorten anything.
    let simplified = ranges.join(" || ");
    Ok(if simplified.chars().count() < range.chars().count() {
        simplified
    } else {
        range.to_string()
    })
}
