//! Small JavaScript-semantics helpers.
//!
//! The port has to reproduce a handful of implicit JS coercions that upstream
//! relies on — `+str`, `String(num)`, `isNaN` — because their results are
//! observable in the output. Keeping them in one place makes the places where
//! Rust deliberately behaves like JavaScript easy to audit.

/// JavaScript's `String(n)`, implementing ECMA-262 `Number::toString`.
///
/// This is not academic. `replaceXRange` evaluates `+m + 1` on a captured
/// component and interpolates the result straight into a comparator string, so
/// a range like `<=0.90071992547409939007199254740990` makes upstream produce
/// `<0.9.007199254740994e+31.0-0` — exponential notation and all. Rust's
/// `{}` would have written `90071992547409940000000000000000` there, and the
/// two implementations would then disagree about what the range even is.
/// The differential fuzzer found exactly this.
///
/// The spec's rules, given the shortest digit string `s` (length `k`) and
/// decimal exponent `n` such that the value is `0.s × 10^n`:
///
/// | condition        | output                       |
/// |------------------|------------------------------|
/// | `k ≤ n ≤ 21`     | `s` then `n-k` zeros         |
/// | `0 < n ≤ 21`     | `s` with a point after `n`   |
/// | `-6 < n ≤ 0`     | `0.` then `-n` zeros then `s`|
/// | otherwise        | exponential, `e±(n-1)`       |
pub fn js_number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if n == 0.0 {
        // JS prints "0" for negative zero too.
        return "0".into();
    }

    let negative = n < 0.0;
    let x = n.abs();

    // Rust's LowerExp gives the shortest round-tripping form, `d.ddde±X`,
    // which is exactly the `s` and `n` the spec asks for.
    let repr = format!("{x:e}");
    let (mantissa, exponent) = repr.split_once('e').expect("LowerExp always emits 'e'");
    let exp: i32 = exponent.parse().expect("LowerExp exponent is an integer");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };

    let k = digits.len() as i32;
    let n_pos = exp + 1;

    let body = if k <= n_pos && n_pos <= 21 {
        format!("{digits}{}", "0".repeat((n_pos - k) as usize))
    } else if 0 < n_pos && n_pos <= 21 {
        let (head, tail) = digits.split_at(n_pos as usize);
        format!("{head}.{tail}")
    } else if -6 < n_pos && n_pos <= 0 {
        format!("0.{}{digits}", "0".repeat((-n_pos) as usize))
    } else {
        let e = n_pos - 1;
        let sign = if e >= 0 { '+' } else { '-' };
        let mag = e.abs();
        if k == 1 {
            format!("{digits}e{sign}{mag}")
        } else {
            let (head, tail) = digits.split_at(1);
            format!("{head}.{tail}e{sign}{mag}")
        }
    };

    if negative {
        format!("-{body}")
    } else {
        body
    }
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
        assert_eq!(js_number_to_string(f64::INFINITY), "Infinity");
    }

    /// Expected values taken from `node -e 'console.log(String(x))'`.
    /// The exponential cases are the ones the differential fuzzer caught.
    #[test]
    fn number_to_string_matches_js_across_magnitudes() {
        let cases: &[(f64, &str)] = &[
            (1e20, "100000000000000000000"),
            (1e21, "1e+21"),
            (9.007199254740994e31, "9.007199254740994e+31"),
            (1.2345678901234569e23, "1.2345678901234569e+23"),
            (1e-6, "0.000001"),
            (1e-7, "1e-7"),
            (5e-7, "5e-7"),
            (1.5e-7, "1.5e-7"),
            (0.5, "0.5"),
            (-12.25, "-12.25"),
            (1234.5678, "1234.5678"),
        ];
        for (input, want) in cases {
            assert_eq!(js_number_to_string(*input), *want, "String({input:e})");
        }
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
