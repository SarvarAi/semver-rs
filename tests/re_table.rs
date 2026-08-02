//! Proves the ported regex table is byte-identical to upstream's.
//!
//! `tools/dump-re-table.mjs` dumps `internal/re.js`'s live `src` and `safeSrc`
//! arrays straight out of the running Node module. This test rebuilds the table
//! in Rust and compares every token, by index, by name, and by both source
//! spellings. If upstream ever reorders, renames, adds or edits a token, this
//! fails loudly instead of drifting silently.
//!
//! Note this compares the *source strings*, not the compiled programs — the
//! compiled form necessarily differs because JS and Rust disagree about `\d`
//! and `\s` (see `re::js_to_rust_dialect` and DECISIONS.md D9).

use semver_npm::re::{
    CARET_TRIM_REPLACE, COMPARATOR_TRIM_REPLACE, RE, TILDE_TRIM_REPLACE, TOKEN_COUNT,
};

fn upstream() -> serde_json::Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/original/re-table.json");
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!(
            "missing {path}: {e}\n\
             Run `./tools/vendor.sh && node tools/dump-re-table.mjs` to regenerate."
        )
    });
    serde_json::from_str(&text).expect("re-table.json is not valid JSON")
}

#[test]
fn token_count_matches_upstream() {
    let up = upstream();
    assert_eq!(
        up["count"].as_u64().unwrap() as usize,
        TOKEN_COUNT,
        "upstream token count changed"
    );
}

#[test]
fn every_token_matches_upstream_byte_for_byte() {
    let up = upstream();
    let tokens = up["tokens"].as_array().unwrap();

    let mut mismatches = Vec::new();
    for tk in tokens {
        let i = tk["index"].as_u64().unwrap() as usize;
        let name = tk["name"].as_str().unwrap();
        let src = tk["src"].as_str().unwrap();
        let safe = tk["safeSrc"].as_str().unwrap();
        let global = tk["global"].as_bool().unwrap();

        if RE.names[i] != name {
            mismatches.push(format!("[{i}] name: upstream {name:?} != port {:?}", RE.names[i]));
            continue;
        }
        if RE.src[i] != src {
            mismatches.push(format!(
                "[{i}] {name} src:\n  upstream: {src}\n  port    : {}",
                RE.src[i]
            ));
        }
        if RE.safe_src[i] != safe {
            mismatches.push(format!(
                "[{i}] {name} safeSrc:\n  upstream: {safe}\n  port    : {}",
                RE.safe_src[i]
            ));
        }
        if RE.is_global[i] != global {
            mismatches.push(format!(
                "[{i}] {name} global: upstream {global} != port {}",
                RE.is_global[i]
            ));
        }
    }

    assert!(
        mismatches.is_empty(),
        "{} of {} tokens diverge from upstream:\n{}",
        mismatches.len(),
        tokens.len(),
        mismatches.join("\n")
    );
}

#[test]
fn trim_replacements_match_upstream() {
    let up = upstream();
    let r = &up["replacements"];
    // Upstream spells these `$1~`; Rust needs `${1}~` so the group index does
    // not swallow following characters. Same meaning, different escape syntax.
    assert_eq!(r["tildeTrimReplace"].as_str().unwrap(), "$1~");
    assert_eq!(TILDE_TRIM_REPLACE, "${1}~");
    assert_eq!(r["caretTrimReplace"].as_str().unwrap(), "$1^");
    assert_eq!(CARET_TRIM_REPLACE, "${1}^");
    assert_eq!(r["comparatorTrimReplace"].as_str().unwrap(), "$1$2$3");
    assert_eq!(COMPARATOR_TRIM_REPLACE, "${1}${2}${3}");
}
