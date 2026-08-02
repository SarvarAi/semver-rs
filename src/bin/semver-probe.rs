//! Rust side of the two-sided differential oracle.
//!
//! DEV-TIME TOOL ONLY. This binary is gated behind the non-default `harness`
//! feature (see Cargo.toml), so `cargo build --release` cannot produce it. It
//! exists purely so `tools/diff-results.mjs` can compare this port against the
//! original Node implementation call-for-call.
//!
//! Speaks the same JSONL protocol as `tools/node-oracle.mjs`:
//!   in : {"id":1,"fn":"satisfies","a":["1.2.3","^1.0.0",null]}
//!   out: {"id":1,"ok":true,"v":true}  |  {"id":1,"ok":false,"e":"..."}

use std::io::{self, BufRead, Write};

include!("../probe_dispatch.rs");

fn main() -> io::Result<()> {
    let stdin = io::BufReader::with_capacity(1 << 20, io::stdin());
    let stdout = io::stdout();
    let mut out = io::BufWriter::with_capacity(1 << 20, stdout.lock());

    for line in stdin.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let call: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(err) => {
                writeln!(out, "{}", json!({"id": null, "ok": false, "e": format!("bad input line: {err}")}))?;
                continue;
            }
        };
        let id = call.get("id").cloned().unwrap_or(Value::Null);
        let name = call.get("fn").and_then(Value::as_str).unwrap_or("");
        let args: Vec<Value> = call
            .get("a")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        // A panic here would be a port bug; catch it so one bad input cannot
        // abort a fuzz run, and report it as a distinctive error.
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dispatch(name, &args)));
        let line = match res {
            Ok(Ok(v)) => json!({"id": id, "ok": true, "v": v}),
            Ok(Err(msg)) => json!({"id": id, "ok": false, "e": msg}),
            Err(_) => json!({"id": id, "ok": false, "e": "__PANIC__"}),
        };
        writeln!(out, "{line}")?;
    }
    out.flush()
}
