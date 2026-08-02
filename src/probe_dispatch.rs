// Shared call-dispatch table for the differential harness.
//
// Physically `include!`d by two consumers rather than exposed as a library
// module, so the shipped crate never grows a serde_json dependency:
//
//   * `src/bin/semver-probe.rs`  (behind the `harness` feature)
//   * `tests/golden_parity.rs`   (serde_json is a dev-dependency)
//
// Keeping one copy means the offline golden replay and the live Node
// comparison exercise byte-for-byte the same mapping from JSON call to port
// API — if they drifted, the golden corpus would stop proving anything.

use serde_json::{json, Map, Value};

use semver_npm::comparator::Comparator;
use semver_npm::functions::CoerceOptions;
use semver_npm::semver::IdentifierBase;
use semver_npm::{functions as f, options::Options, range::Range, ranges as r, semver::SemVer};

// Marker the differ recognises as "not ported yet" rather than a divergence.
const UNIMPLEMENTED: &str = "__UNIMPLEMENTED__";

/// Emit integral floats as JSON integers so `1` does not compare unequal to
/// `1.0` against Node, which has no separate integer type.
fn num(n: f64) -> Value {
    if n.is_finite() && n == n.trunc() && n.abs() < 9e15 {
        json!(n as i64)
    } else if n.is_finite() {
        json!(n)
    } else {
        Value::Null
    }
}

fn opts(v: Option<&Value>) -> Options {
    match v {
        None | Some(Value::Null) => Options::new(),
        Some(Value::Bool(b)) => Options::from(*b),
        Some(Value::Object(o)) => Options {
            loose: o.get("loose").and_then(Value::as_bool).unwrap_or(false),
            include_prerelease: o
                .get("includePrerelease")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
        // Any other truthy non-object is `{loose:true}` in upstream's parseOptions.
        Some(_) => Options::loose(),
    }
}

fn coerce_opts(v: Option<&Value>) -> CoerceOptions {
    match v {
        Some(Value::Object(o)) => CoerceOptions {
            rtl: o.get("rtl").and_then(Value::as_bool).unwrap_or(false),
            include_prerelease: o
                .get("includePrerelease")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            loose: o.get("loose").and_then(Value::as_bool).unwrap_or(false),
        },
        _ => CoerceOptions::default(),
    }
}

fn s(a: &[Value], i: usize) -> &str {
    a.get(i).and_then(Value::as_str).unwrap_or("")
}

fn list(a: &[Value], i: usize) -> Vec<String> {
    a.get(i)
        .and_then(Value::as_array)
        .map(|v| {
            v.iter()
                .map(|x| x.as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn describe(sv: Option<&SemVer>) -> Value {
    match sv {
        None => Value::Null,
        Some(sv) => {
            let mut m = Map::new();
            m.insert("version".into(), json!(sv.version));
            m.insert("major".into(), num(sv.major));
            m.insert("minor".into(), num(sv.minor));
            m.insert("patch".into(), num(sv.patch));
            m.insert(
                "prerelease".into(),
                Value::Array(
                    sv.prerelease
                        .iter()
                        .map(|id| match id {
                            semver_npm::Identifier::Num(n) => num(*n),
                            semver_npm::Identifier::Str(x) => json!(x),
                        })
                        .collect(),
                ),
            );
            m.insert("build".into(), json!(sv.build));
            m.insert("raw".into(), json!(sv.raw));
            Value::Object(m)
        }
    }
}

fn ident_base(v: Option<&Value>) -> IdentifierBase {
    match v {
        None | Some(Value::Null) => IdentifierBase::Undefined,
        Some(Value::Bool(false)) => IdentifierBase::False,
        Some(Value::Bool(true)) => IdentifierBase::Value("true".into()),
        Some(Value::String(x)) => IdentifierBase::Value(x.clone()),
        Some(Value::Number(n)) => IdentifierBase::Value(n.to_string()),
        Some(_) => IdentifierBase::Undefined,
    }
}

fn opt_str(a: &[Value], i: usize) -> Option<&str> {
    match a.get(i) {
        Some(Value::String(x)) => Some(x.as_str()),
        _ => None,
    }
}

type Out = Result<Value, String>;

fn dispatch(name: &str, a: &[Value]) -> Out {
    let e = |x: semver_npm::Error| x.message;
    match name {
        "parse" => Ok(describe(f::parse(s(a, 0), opts(a.get(1))).as_ref())),
        "valid" => Ok(json!(f::valid(s(a, 0), opts(a.get(1))))),
        "clean" => Ok(json!(f::clean(s(a, 0), opts(a.get(1))))),
        "satisfies" => Ok(json!(semver_npm::range::satisfies(
            s(a, 0),
            s(a, 1),
            opts(a.get(2))
        ))),
        "validRange" => Ok(json!(semver_npm::range::valid_range(s(a, 0), opts(a.get(1))))),

        "compare" => f::compare(s(a, 0), s(a, 1), opts(a.get(2))).map(|v| json!(v)).map_err(e),
        "rcompare" => f::rcompare(s(a, 0), s(a, 1), opts(a.get(2))).map(|v| json!(v)).map_err(e),
        "compareLoose" => f::compare_loose(s(a, 0), s(a, 1)).map(|v| json!(v)).map_err(e),
        "compareBuild" => {
            f::compare_build(s(a, 0), s(a, 1), opts(a.get(2))).map(|v| json!(v)).map_err(e)
        }
        "gt" => f::gt(s(a, 0), s(a, 1), opts(a.get(2))).map(|v| json!(v)).map_err(e),
        "lt" => f::lt(s(a, 0), s(a, 1), opts(a.get(2))).map(|v| json!(v)).map_err(e),
        "eq" => f::eq(s(a, 0), s(a, 1), opts(a.get(2))).map(|v| json!(v)).map_err(e),
        "neq" => f::neq(s(a, 0), s(a, 1), opts(a.get(2))).map(|v| json!(v)).map_err(e),
        "gte" => f::gte(s(a, 0), s(a, 1), opts(a.get(2))).map(|v| json!(v)).map_err(e),
        "lte" => f::lte(s(a, 0), s(a, 1), opts(a.get(2))).map(|v| json!(v)).map_err(e),
        "cmp" => {
            f::cmp(s(a, 0), s(a, 1), s(a, 2), opts(a.get(3))).map(|v| json!(v)).map_err(e)
        }

        "major" => f::major(s(a, 0), opts(a.get(1))).map(num).map_err(e),
        "minor" => f::minor(s(a, 0), opts(a.get(1))).map(num).map_err(e),
        "patch" => f::patch(s(a, 0), opts(a.get(1))).map(num).map_err(e),
        "prerelease" => Ok(match f::prerelease(s(a, 0), opts(a.get(1))) {
            None => Value::Null,
            Some(p) => Value::Array(
                p.iter()
                    .map(|id| match id {
                        semver_npm::Identifier::Num(n) => num(*n),
                        semver_npm::Identifier::Str(x) => json!(x),
                    })
                    .collect(),
            ),
        }),

        "inc" => Ok(json!(f::inc(
            s(a, 0),
            s(a, 1),
            opts(a.get(2)),
            opt_str(a, 3),
            &ident_base(a.get(4))
        ))),
        "diff" => f::diff(s(a, 0), s(a, 1)).map(|v| json!(v)).map_err(e),
        "truncate" => Ok(json!(f::truncate(s(a, 0), s(a, 1), opts(a.get(2))))),
        "coerce" => Ok(describe(f::coerce(s(a, 0), coerce_opts(a.get(1))).as_ref())),

        "sort" => f::sort(&list(a, 0), opts(a.get(1))).map(|v| json!(v)).map_err(e),
        "rsort" => f::rsort(&list(a, 0), opts(a.get(1))).map(|v| json!(v)).map_err(e),

        "minVersion" => r::min_version(s(a, 0), opts(a.get(1)))
            .map(|v| describe(v.as_ref()))
            .map_err(e),
        "minSatisfying" => Ok(json!(r::min_satisfying(&list(a, 0), s(a, 1), opts(a.get(2))))),
        "maxSatisfying" => Ok(json!(r::max_satisfying(&list(a, 0), s(a, 1), opts(a.get(2))))),
        "gtr" => r::gtr(s(a, 0), s(a, 1), opts(a.get(2))).map(|v| json!(v)).map_err(e),
        "ltr" => r::ltr(s(a, 0), s(a, 1), opts(a.get(2))).map(|v| json!(v)).map_err(e),
        "outside" => r::outside(s(a, 0), s(a, 1), s(a, 2), opts(a.get(3)))
            .map(|v| json!(v))
            .map_err(e),
        "toComparators" => semver_npm::range::to_comparators(s(a, 0), opts(a.get(1)))
            .map(|v| json!(v))
            .map_err(e),
        "intersects" => {
            r::intersects(s(a, 0), s(a, 1), opts(a.get(2))).map(|v| json!(v)).map_err(e)
        }
        "simplifyRange" => r::simplify_range(&list(a, 0), s(a, 1), opts(a.get(2)))
            .map(|v| json!(v))
            .map_err(e),
        "subset" => semver_npm::subset::subset(s(a, 0), s(a, 1), opts(a.get(2)))
            .map(|v| json!(v))
            .map_err(e),

        "rangeToString" => Range::new(s(a, 0), opts(a.get(1)))
            .map(|x| json!(x.range()))
            .map_err(e),
        "rangeSet" => Range::new(s(a, 0), opts(a.get(1)))
            .map(|x| {
                json!(x
                    .set
                    .iter()
                    .map(|cs| cs.iter().map(|c| c.value.clone()).collect::<Vec<_>>())
                    .collect::<Vec<_>>())
            })
            .map_err(e),
        "comparatorValue" => {
            Comparator::new(s(a, 0), opts(a.get(1))).map(|c| json!(c.value)).map_err(e)
        }
        "comparatorIntersects" => {
            let o = opts(a.get(2));
            match (Comparator::new(s(a, 0), o), Comparator::new(s(a, 1), o)) {
                (Ok(x), Ok(y)) => x.intersects(&y, o).map(|v| json!(v)).map_err(e),
                (Err(x), _) => Err(e(x)),
                (_, Err(y)) => Err(e(y)),
            }
        }
        "compareIdentifiers" => {
            use semver_npm::Identifier as I;
            let to_id = |v: Option<&Value>| match v {
                Some(Value::Number(n)) => I::Num(n.as_f64().unwrap_or(f64::NAN)),
                Some(Value::String(x)) => I::Str(x.clone()),
                Some(other) => I::Str(other.to_string()),
                None => I::Str(String::new()),
            };
            Ok(json!(semver_npm::compare_identifiers(
                &to_id(a.first()),
                &to_id(a.get(1))
            )))
        }

        other => Err(format!("unknown fn: {other}")),
    }
}
