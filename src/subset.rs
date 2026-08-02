//! Port of `ranges/subset.js`.
//!
//! `subset(sub, dom)` answers "does every version satisfying `sub` also satisfy
//! `dom`?" without enumerating versions. The algorithm reduces each simple range
//! to at most one `=`, one lower bound and one upper bound, then reasons about
//! those bounds against the dominator's.
//!
//! Two upstream subtleties drive the shape of this port:
//!
//! 1. `simpleSubset` is **tri-state** — `true`, `false`, or `null` meaning "this
//!    simple range is the null set". The null set is a subset of everything, but
//!    null branches inside a `||` are ignored rather than making the whole thing
//!    true, which is what `sawNonNull` tracks.
//! 2. `gtltComp` is left `undefined` unless *both* a lower and an upper bound
//!    exist, and upstream then tests `gtltComp !== 0` — which is **true** for
//!    `undefined`. Modelling it as `Option<i32>` and comparing against
//!    `Some(0)` preserves that; using a plain `i32` default of 0 would silently
//!    invert three separate branches.

use crate::comparator::Comparator;
use crate::options::Options;
use crate::range::Range;
use crate::semver::SemVer;
use crate::Result;

/// Port of `subset()`.
pub fn subset(sub: &str, dom: &str, options: impl Into<Options> + Copy) -> Result<bool> {
    if sub == dom {
        return Ok(true);
    }
    let options = options.into();
    let sub_range = Range::new(sub, options)?;
    let dom_range = Range::new(dom, options)?;

    let min_version = vec![Comparator::new(">=0.0.0", Options::new())?];
    let min_version_pre = vec![Comparator::new(">=0.0.0-0", Options::new())?];

    let mut saw_non_null = false;

    'outer: for simple_sub in &sub_range.set {
        for simple_dom in &dom_range.set {
            let is_sub = simple_subset(
                simple_sub,
                simple_dom,
                options,
                &min_version,
                &min_version_pre,
            )?;
            saw_non_null = saw_non_null || is_sub.is_some();
            if is_sub == Some(true) {
                continue 'outer;
            }
        }
        // The null set is a subset of everything, but a null simple range inside
        // a complex range is ignored. So a non-subset only counts once some
        // non-null range has been seen.
        if saw_non_null {
            return Ok(false);
        }
    }
    Ok(true)
}

fn values(cs: &[Comparator]) -> Vec<&str> {
    cs.iter().map(|c| c.value.as_str()).collect()
}

/// `None` is upstream's `null` — "this simple range is the null set".
fn simple_subset(
    sub: &[Comparator],
    dom: &[Comparator],
    options: Options,
    min_version: &[Comparator],
    min_version_pre: &[Comparator],
) -> Result<Option<bool>> {
    // Upstream's `sub === dom` is an object-identity check that really can fire:
    // Range memoizes parsed comparator lists, so two textually identical simple
    // ranges share one array. Comparing by value is equivalent here (a range is
    // always a subset of itself) and does not depend on the cache.
    if values(sub) == values(dom) {
        return Ok(Some(true));
    }

    let sub: &[Comparator] = if sub.len() == 1 && sub[0].is_any() {
        if dom.len() == 1 && dom[0].is_any() {
            return Ok(Some(true));
        } else if options.include_prerelease {
            min_version_pre
        } else {
            min_version
        }
    } else {
        sub
    };

    let dom: &[Comparator] = if dom.len() == 1 && dom[0].is_any() {
        if options.include_prerelease {
            return Ok(Some(true));
        }
        min_version
    } else {
        dom
    };

    // Reduce `sub` to at most one equality and one bound in each direction.
    let mut eq_set: Vec<&SemVer> = Vec::new();
    let mut gt: Option<&Comparator> = None;
    let mut lt: Option<&Comparator> = None;
    for c in sub {
        match c.operator.as_str() {
            ">" | ">=" => gt = Some(higher_gt(gt, c)),
            "<" | "<=" => lt = Some(lower_lt(lt, c)),
            _ => {
                if let Some(sv) = &c.semver {
                    eq_set.push(sv);
                }
            }
        }
    }

    if eq_set.len() > 1 {
        return Ok(None);
    }

    // Left as `None` unless both bounds exist — see the module docs.
    let mut gtlt_comp: Option<i32> = None;
    if let (Some(g), Some(l)) = (gt, lt) {
        let (Some(gs), Some(ls)) = (&g.semver, &l.semver) else {
            return Ok(None);
        };
        let c = gs.compare(ls);
        gtlt_comp = Some(c);
        if c > 0 {
            return Ok(None);
        }
        if c == 0 && (g.operator != ">=" || l.operator != "<=") {
            return Ok(None);
        }
    }

    // Iterates once or not at all.
    if let Some(eq) = eq_set.first() {
        if let Some(g) = gt {
            if !Range::new(&g.value, options)?.test(eq) {
                return Ok(None);
            }
        }
        if let Some(l) = lt {
            if !Range::new(&l.value, options)?.test(eq) {
                return Ok(None);
            }
        }
        for c in dom {
            if !Range::new(&c.value, options)?.test(eq) {
                return Ok(Some(false));
            }
        }
        return Ok(Some(true));
    }

    let mut has_dom_lt = false;
    let mut has_dom_gt = false;

    // A prerelease bound in `sub` needs a dominator comparator with the same
    // major.minor.patch tuple that *also* carries a prerelease; otherwise the
    // sub range admits prereleases the dominator does not.
    let mut need_dom_lt_pre: Option<&SemVer> = lt.and_then(|l| l.semver.as_ref()).filter(|sv| {
        !options.include_prerelease && !sv.prerelease.is_empty()
    });
    let mut need_dom_gt_pre: Option<&SemVer> = gt.and_then(|g| g.semver.as_ref()).filter(|sv| {
        !options.include_prerelease && !sv.prerelease.is_empty()
    });

    // Exception: `<1.2.3-0` means the same thing as `<1.2.3`.
    if let (Some(sv), Some(l)) = (need_dom_lt_pre, lt) {
        if sv.prerelease.len() == 1
            && l.operator == "<"
            && sv.prerelease[0] == crate::Identifier::Num(0.0)
        {
            need_dom_lt_pre = None;
        }
    }

    for c in dom {
        has_dom_gt = has_dom_gt || c.operator == ">" || c.operator == ">=";
        has_dom_lt = has_dom_lt || c.operator == "<" || c.operator == "<=";

        if let Some(g) = gt {
            if let Some(need) = need_dom_gt_pre {
                if let Some(cs) = &c.semver {
                    if !cs.prerelease.is_empty()
                        && cs.major == need.major
                        && cs.minor == need.minor
                        && cs.patch == need.patch
                    {
                        need_dom_gt_pre = None;
                    }
                }
            }
            if c.operator == ">" || c.operator == ">=" {
                // `higher === c && higher !== gt` — i.e. the dominator's lower
                // bound is strictly higher, so sub escapes below it.
                if higher_gt_picks_b(Some(g), c) {
                    return Ok(Some(false));
                }
            } else if g.operator == ">=" {
                if let Some(gs) = &g.semver {
                    if !c.test(gs) {
                        return Ok(Some(false));
                    }
                }
            }
        }

        if let Some(l) = lt {
            if let Some(need) = need_dom_lt_pre {
                if let Some(cs) = &c.semver {
                    if !cs.prerelease.is_empty()
                        && cs.major == need.major
                        && cs.minor == need.minor
                        && cs.patch == need.patch
                    {
                        need_dom_lt_pre = None;
                    }
                }
            }
            if c.operator == "<" || c.operator == "<=" {
                if lower_lt_picks_b(Some(l), c) {
                    return Ok(Some(false));
                }
            } else if l.operator == "<=" {
                if let Some(ls) = &l.semver {
                    if !c.test(ls) {
                        return Ok(Some(false));
                    }
                }
            }
        }

        if c.operator.is_empty() && (lt.is_some() || gt.is_some()) && gtlt_comp != Some(0) {
            return Ok(Some(false));
        }
    }

    // A bound with nothing opposing it in the dominator fails, unless the sub
    // range was itself pinned from the other side —
    // `>1.0.0 <1.0.1` is still a subset of `<2.0.0`.
    if gt.is_some() && has_dom_lt && lt.is_none() && gtlt_comp != Some(0) {
        return Ok(Some(false));
    }
    if lt.is_some() && has_dom_gt && gt.is_none() && gtlt_comp != Some(0) {
        return Ok(Some(false));
    }

    // Needed a prerelease dominator for a specific tuple and never found one:
    // `>=1.2.3-pre` is not a subset of `>=1.0.0`, because it admits prereleases
    // in the 1.2.3 tuple that `>=1.0.0` excludes.
    if need_dom_gt_pre.is_some() || need_dom_lt_pre.is_some() {
        return Ok(Some(false));
    }

    Ok(Some(true))
}

/// Does `higherGT(a, b)` pick `b`? `>=1.2.3` is lower than `>1.2.3`.
fn higher_gt_picks_b(a: Option<&Comparator>, b: &Comparator) -> bool {
    let Some(a) = a else { return true };
    let (Some(av), Some(bv)) = (&a.semver, &b.semver) else {
        return false;
    };
    let comp = av.compare(bv);
    if comp > 0 {
        false
    } else if comp < 0 {
        true
    } else {
        b.operator == ">" && a.operator == ">="
    }
}

fn higher_gt<'a>(a: Option<&'a Comparator>, b: &'a Comparator) -> &'a Comparator {
    if higher_gt_picks_b(a, b) {
        b
    } else {
        a.expect("picks_b is true when a is None")
    }
}

/// Does `lowerLT(a, b)` pick `b`? `<=1.2.3` is higher than `<1.2.3`.
fn lower_lt_picks_b(a: Option<&Comparator>, b: &Comparator) -> bool {
    let Some(a) = a else { return true };
    let (Some(av), Some(bv)) = (&a.semver, &b.semver) else {
        return false;
    };
    let comp = av.compare(bv);
    if comp < 0 {
        false
    } else if comp > 0 {
        true
    } else {
        b.operator == "<" && a.operator == "<="
    }
}

fn lower_lt<'a>(a: Option<&'a Comparator>, b: &'a Comparator) -> &'a Comparator {
    if lower_lt_picks_b(a, b) {
        b
    } else {
        a.expect("picks_b is true when a is None")
    }
}
