//! Port of everything under `functions/`.
//!
//! Upstream has two failure conventions and this module preserves the
//! distinction: `parse`/`valid`/`clean`/`coerce`/`inc` swallow errors and return
//! `null` (here `Option`), while `compare`/`major`/`diff` and friends propagate
//! the `TypeError` (here `Result`).

use crate::constants::{MAX_SAFE_COMPONENT_LENGTH, RELEASE_TYPES};
use crate::identifiers::Identifier;
use crate::options::Options;
use crate::re::{self, RE};
use crate::semver::{js_trim, SemVer};
use crate::{Error, Result};

// ---------------------------------------------------------------------------
// parse / valid / clean
// ---------------------------------------------------------------------------

/// Port of `parse()`. Returns `None` where upstream returns `null`.
pub fn parse(version: &str, options: impl Into<Options>) -> Option<SemVer> {
    SemVer::new(version, options).ok()
}

/// Port of `parse(version, options, true)` — the throwing variant.
pub fn parse_throwing(version: &str, options: impl Into<Options>) -> Result<SemVer> {
    SemVer::new(version, options)
}

/// Port of `valid()`.
pub fn valid(version: &str, options: impl Into<Options>) -> Option<String> {
    parse(version, options).map(|v| v.version)
}

/// Port of `clean()`. Strips leading `=`/`v` characters before parsing.
pub fn clean(version: &str, options: impl Into<Options>) -> Option<String> {
    let trimmed = js_trim(version);
    let stripped = trimmed.trim_start_matches(['=', 'v']);
    parse(stripped, options).map(|v| v.version)
}

// ---------------------------------------------------------------------------
// component accessors
// ---------------------------------------------------------------------------

pub fn major(version: &str, options: impl Into<Options>) -> Result<f64> {
    SemVer::new(version, options).map(|v| v.major)
}

pub fn minor(version: &str, options: impl Into<Options>) -> Result<f64> {
    SemVer::new(version, options).map(|v| v.minor)
}

pub fn patch(version: &str, options: impl Into<Options>) -> Result<f64> {
    SemVer::new(version, options).map(|v| v.patch)
}

/// Port of `prerelease()`. `None` for both "invalid" and "no prerelease",
/// matching upstream's single `null` return.
pub fn prerelease(version: &str, options: impl Into<Options>) -> Option<Vec<Identifier>> {
    let p = parse(version, options)?;
    if p.prerelease.is_empty() {
        None
    } else {
        Some(p.prerelease)
    }
}

// ---------------------------------------------------------------------------
// comparison
// ---------------------------------------------------------------------------

pub fn compare(a: &str, b: &str, options: impl Into<Options> + Copy) -> Result<i32> {
    Ok(SemVer::new(a, options)?.compare(&SemVer::new(b, options)?))
}

pub fn rcompare(a: &str, b: &str, options: impl Into<Options> + Copy) -> Result<i32> {
    compare(b, a, options)
}

pub fn compare_loose(a: &str, b: &str) -> Result<i32> {
    compare(a, b, Options::loose())
}

/// Port of `compareBuild()`: primary comparison, then build metadata as a
/// tie-break. Build metadata is ignored by `compare` but not here.
pub fn compare_build(a: &str, b: &str, options: impl Into<Options> + Copy) -> Result<i32> {
    let va = SemVer::new(a, options)?;
    let vb = SemVer::new(b, options)?;
    let c = va.compare(&vb);
    Ok(if c != 0 { c } else { va.compare_build(&vb) })
}

pub fn gt(a: &str, b: &str, options: impl Into<Options> + Copy) -> Result<bool> {
    Ok(compare(a, b, options)? > 0)
}

pub fn lt(a: &str, b: &str, options: impl Into<Options> + Copy) -> Result<bool> {
    Ok(compare(a, b, options)? < 0)
}

pub fn eq(a: &str, b: &str, options: impl Into<Options> + Copy) -> Result<bool> {
    Ok(compare(a, b, options)? == 0)
}

pub fn neq(a: &str, b: &str, options: impl Into<Options> + Copy) -> Result<bool> {
    Ok(compare(a, b, options)? != 0)
}

pub fn gte(a: &str, b: &str, options: impl Into<Options> + Copy) -> Result<bool> {
    Ok(compare(a, b, options)? >= 0)
}

pub fn lte(a: &str, b: &str, options: impl Into<Options> + Copy) -> Result<bool> {
    Ok(compare(a, b, options)? <= 0)
}

/// Port of `cmp()`.
///
/// `===` and `!==` are identity comparisons in upstream: given strings they
/// compare the strings verbatim, given SemVer objects they compare `.version`.
/// This is the string-input form; [`cmp_semver`] is the object form.
pub fn cmp(a: &str, op: &str, b: &str, options: impl Into<Options> + Copy) -> Result<bool> {
    match op {
        "===" => Ok(a == b),
        "!==" => Ok(a != b),
        "" | "=" | "==" => eq(a, b, options),
        "!=" => neq(a, b, options),
        ">" => gt(a, b, options),
        ">=" => gte(a, b, options),
        "<" => lt(a, b, options),
        "<=" => lte(a, b, options),
        _ => Err(Error::new(format!("Invalid operator: {op}"))),
    }
}

/// Port of `cmp()` for already-parsed operands, where `===` compares the
/// canonical `.version` string rather than the raw input.
pub fn cmp_semver(a: &SemVer, op: &str, b: &SemVer) -> Result<bool> {
    match op {
        "===" => Ok(a.version == b.version),
        "!==" => Ok(a.version != b.version),
        "" | "=" | "==" => Ok(a.compare(b) == 0),
        "!=" => Ok(a.compare(b) != 0),
        ">" => Ok(a.compare(b) > 0),
        ">=" => Ok(a.compare(b) >= 0),
        "<" => Ok(a.compare(b) < 0),
        "<=" => Ok(a.compare(b) <= 0),
        _ => Err(Error::new(format!("Invalid operator: {op}"))),
    }
}

// ---------------------------------------------------------------------------
// sorting
// ---------------------------------------------------------------------------

/// Port of `sort()`. Uses `compareBuild`, and is stable, like `Array#sort`.
pub fn sort(list: &[String], options: impl Into<Options> + Copy) -> Result<Vec<String>> {
    sort_inner(list, options, false)
}

/// Port of `rsort()`.
pub fn rsort(list: &[String], options: impl Into<Options> + Copy) -> Result<Vec<String>> {
    sort_inner(list, options, true)
}

fn sort_inner(
    list: &[String],
    options: impl Into<Options> + Copy,
    reverse: bool,
) -> Result<Vec<String>> {
    // `Array.prototype.sort` never invokes the comparator for fewer than two
    // elements, so an invalid lone version is returned untouched rather than
    // throwing.
    if list.len() < 2 {
        return Ok(list.to_vec());
    }

    // Which version an invalid list blames is observable, so the parse order
    // has to match the order V8 first touches each element.
    //
    // V8 begins both binary-insertion sort and TimSort run detection with
    // `comparefn(list[1], list[0])`. `sort`'s comparator is
    // `(a, b) => compareBuild(a, b)`, which parses its first argument first —
    // so `sort` reports list[1]. `rsort`'s comparator is
    // `(a, b) => compareBuild(b, a)`, which swaps them, so it reports list[0].
    // Everything after index 1 is then touched in ascending order.
    let mut order: Vec<usize> = (0..list.len()).collect();
    if !reverse {
        order.swap(0, 1);
    }
    let mut by_index: Vec<Option<SemVer>> = (0..list.len()).map(|_| None).collect();
    for i in order {
        by_index[i] = Some(SemVer::new(&list[i], options)?);
    }

    let mut parsed: Vec<(SemVer, String)> = by_index
        .into_iter()
        .zip(list.iter())
        .map(|(sv, s)| (sv.expect("every index parsed"), s.clone()))
        .collect();
    parsed.sort_by(|a, b| {
        let (x, y) = if reverse { (&b.0, &a.0) } else { (&a.0, &b.0) };
        let c = x.compare(y);
        let c = if c != 0 { c } else { x.compare_build(y) };
        c.cmp(&0)
    });
    Ok(parsed.into_iter().map(|(_, s)| s).collect())
}

// ---------------------------------------------------------------------------
// diff
// ---------------------------------------------------------------------------

/// Port of `diff()`. Returns `None` where upstream returns `null` (equal
/// versions); propagates the parse error otherwise, since upstream uses the
/// throwing form of `parse` here.
pub fn diff(version1: &str, version2: &str) -> Result<Option<String>> {
    let v1 = SemVer::new(version1, Options::new())?;
    let v2 = SemVer::new(version2, Options::new())?;
    let comparison = v1.compare(&v2);

    if comparison == 0 {
        return Ok(None);
    }

    let v1_higher = comparison > 0;
    let (high, low) = if v1_higher { (&v1, &v2) } else { (&v2, &v1) };
    let high_has_pre = !high.prerelease.is_empty();
    let low_has_pre = !low.prerelease.is_empty();

    if low_has_pre && !high_has_pre {
        // Prerelease -> release needs special casing.
        if low.patch == 0.0 && low.minor == 0.0 {
            return Ok(Some("major".into()));
        }
        if low.compare_main(high) == 0 {
            if low.minor != 0.0 && low.patch == 0.0 {
                return Ok(Some("minor".into()));
            }
            return Ok(Some("patch".into()));
        }
    }

    let prefix = if high_has_pre { "pre" } else { "" };

    if v1.major != v2.major {
        return Ok(Some(format!("{prefix}major")));
    }
    if v1.minor != v2.minor {
        return Ok(Some(format!("{prefix}minor")));
    }
    if v1.patch != v2.patch {
        return Ok(Some(format!("{prefix}patch")));
    }
    Ok(Some("prerelease".into()))
}

// ---------------------------------------------------------------------------
// truncate
// ---------------------------------------------------------------------------

/// Port of `truncate()`.
pub fn truncate(
    version: &str,
    truncation: &str,
    options: impl Into<Options>,
) -> Option<String> {
    if !RELEASE_TYPES.contains(&truncation) {
        return None;
    }
    let mut v = parse(version, options)?;
    if truncation.starts_with("pre") {
        return Some(v.version);
    }
    v.prerelease = Vec::new();
    match truncation {
        "major" => {
            v.minor = 0.0;
            v.patch = 0.0;
        }
        "minor" => {
            v.patch = 0.0;
        }
        _ => {}
    }
    Some(v.format().to_string())
}

// ---------------------------------------------------------------------------
// coerce
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
pub struct CoerceOptions {
    pub rtl: bool,
    pub include_prerelease: bool,
    pub loose: bool,
}

/// Port of `coerce()`.
///
/// The right-to-left mode is the interesting half: upstream drives a global
/// regex by hand, rewinding `lastIndex` to `index + m[1].len + m[2].len` after
/// each hit so that *overlapping* matches are considered, and stops once a match
/// ends at the end of the string. `'1.2.3.4'` must coerce to `2.3.4`, not `3.4`
/// or `4`. This reproduces that walk with explicit offsets.
pub fn coerce(version: &str, options: CoerceOptions) -> Option<SemVer> {
    let token = if options.include_prerelease {
        re::COERCEFULL
    } else {
        re::COERCE
    };

    let caps = if !options.rtl {
        RE.safe(token).captures(version)?
    } else {
        let rtl_token = if options.include_prerelease {
            re::COERCERTLFULL
        } else {
            re::COERCERTL
        };
        let rx = RE.safe(rtl_token);

        let mut best: Option<regex::Captures> = None;
        let mut last_index = 0usize;
        loop {
            if last_index > version.len() {
                break;
            }
            let Some(next) = rx.captures_at(version, last_index) else {
                break;
            };
            let next_m = next.get(0).unwrap();

            // Upstream's loop condition: keep going while we have no match yet,
            // or the current best does not already end at end-of-string.
            if let Some(b) = &best {
                let bm = b.get(0).unwrap();
                if bm.end() == version.len() {
                    break;
                }
            }

            let replace = match &best {
                None => true,
                Some(b) => next_m.end() != b.get(0).unwrap().end(),
            };

            let advance = next_m.start()
                + next.get(1).map_or(0, |m| m.as_str().len())
                + next.get(2).map_or(0, |m| m.as_str().len());

            if replace {
                best = Some(next);
            }

            // Guarantee forward progress even if the groups were empty.
            last_index = if advance > last_index { advance } else { last_index + 1 };
        }
        best?
    };

    let major = caps.get(2)?.as_str();
    let minor = caps.get(3).map_or("0", |m| m.as_str());
    let patch = caps.get(4).map_or("0", |m| m.as_str());
    let pre = if options.include_prerelease {
        caps.get(5).map(|m| format!("-{}", m.as_str())).unwrap_or_default()
    } else {
        String::new()
    };
    let build = if options.include_prerelease {
        caps.get(6).map(|m| format!("+{}", m.as_str())).unwrap_or_default()
    } else {
        String::new()
    };

    let _ = MAX_SAFE_COMPONENT_LENGTH; // bound lives in the COERCE regex itself
    parse(
        &format!("{major}.{minor}.{patch}{pre}{build}"),
        Options { loose: options.loose, include_prerelease: options.include_prerelease },
    )
}

// ---------------------------------------------------------------------------
// inc
// ---------------------------------------------------------------------------

/// Port of `functions/inc.js`. Swallows every error into `None`, as upstream does.
pub fn inc(
    version: &str,
    release: &str,
    options: impl Into<Options>,
    identifier: Option<&str>,
    identifier_base: &crate::semver::IdentifierBase,
) -> Option<String> {
    let mut sv = SemVer::new(version, options).ok()?;
    sv.inc(release, identifier, identifier_base).ok()?;
    Some(sv.version.clone())
}
