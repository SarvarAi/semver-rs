//! Small JavaScript-semantics helpers.
//!
//! The port has to reproduce a handful of implicit JS coercions that upstream
//! relies on — `+str`, `String(num)`, `isNaN` — because their results are
//! observable in the output. Keeping them in one place makes the places where
//! Rust deliberately behaves like JavaScript easy to audit.

/// JavaScript's `String(n)` for the number range this port actually produces
/// (non-negative integers, plus the occasional NaN from a failed coercion).
///
/// Full ECMAScript number-to-string is a much larger algorithm; upstream never
/// reaches the parts that differ, because every number that gets stringified
/// here came from parsing digits under `MAX_SAFE_INTEGER`.
pub fn js_number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if n == n.trunc() && n.abs() < 1e21 {
        // Avoid Rust's "-0" for negative zero; JS prints "0".
        if n == 0.0 {
            return "0".into();
        }
        return format!("{}", n as i64);
    }
    let s = format!("{n}");
    s
}

/// JavaScript's unary `+` on a string (`Number(s)`).
///
/// Whitespace-trimmed; empty or whitespace-only is `0`; anything unparseable is
/// `NaN`. Upstream uses this on regex captures such as the major component of
/// an X-range before incrementing it.
pub fn js_to_number(s: &str) -> f64 {
    let t = s.trim_matches(|c: char| {
        c.is_whitespace() || c == '\u{feff}'
    });
    if t.is_empty() {
        return 0.0;
    }
    // JS accepts a leading +/-, and rejects trailing garbage (unlike Rust's
    // parse, which also rejects it — the behaviours line up for our inputs).
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// `+s + 1`, the increment upstream performs on X-range and tilde/caret bounds.
pub fn js_increment(s: &str) -> String {
    js_number_to_string(js_to_number(s) + 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_to_string_matches_js_for_integers() {
        assert_eq!(js_number_to_string(0.0), "0");
        assert_eq!(js_number_to_string(-0.0), "0");
        assert_eq!(js_number_to_string(42.0), "42");
        assert_eq!(js_number_to_string(9_007_199_254_740_991.0), "9007199254740991");
        assert_eq!(js_number_to_string(f64::NAN), "NaN");
    }

    #[test]
    fn to_number_matches_js() {
        assert_eq!(js_to_number("12"), 12.0);
        assert_eq!(js_to_number(""), 0.0);
        assert_eq!(js_to_number("  7 "), 7.0);
        assert!(js_to_number("x").is_nan());
    }

    #[test]
    fn increment_bumps_range_bounds() {
        assert_eq!(js_increment("1"), "2");
        assert_eq!(js_increment("0"), "1");
        assert_eq!(js_increment("9"), "10");
    }
}
