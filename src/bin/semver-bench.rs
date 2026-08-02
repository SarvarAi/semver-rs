//! Benchmark driver for the Rust port. Dev-time tool, behind the `harness`
//! feature — see bench/methodology.md.
//!
//!   semver-bench <workload> <iterations> <warmup>
//!
//! Prints JSON: per-iteration wall-clock milliseconds. The orchestrator
//! (tools/bench.mjs) runs the identical workload against Node and reports the
//! distribution rather than a single mean.

use std::time::Instant;

use semver_npm::functions as f;
use semver_npm::options::Options;
use semver_npm::range::{satisfies, Range};
use semver_npm::semver::SemVer;

fn corpus() -> Vec<String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/original/corpus-b-strings.json");
    let text = std::fs::read_to_string(path).expect("corpus-b-strings.json");
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    v["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|e| e["s"].as_str().unwrap_or_default().to_string())
        .collect()
}

fn classified() -> (Vec<String>, Vec<String>) {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/original/corpus-b-strings.json");
    let text = std::fs::read_to_string(path).expect("corpus-b-strings.json");
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    let entries = v["entries"].as_array().expect("entries");
    let versions = entries
        .iter()
        .filter(|e| {
            e["looseVersion"].as_bool().unwrap_or(false)
                || e["strictVersion"].as_bool().unwrap_or(false)
        })
        .map(|e| e["s"].as_str().unwrap_or_default().to_string())
        .collect();
    let ranges = entries
        .iter()
        .filter(|e| e["validRange"].as_bool().unwrap_or(false))
        .map(|e| e["s"].as_str().unwrap_or_default().to_string())
        .collect();
    (versions, ranges)
}

/// `black_box` without the nightly intrinsic: returning through a volatile read
/// keeps the optimiser from deleting the work being measured.
#[inline(never)]
fn sink<T>(v: T) -> T {
    std::hint::black_box(v)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let workload = args.first().map(String::as_str).unwrap_or("parse");
    let iterations: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(50);
    let warmup: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);

    let all = corpus();
    let (versions, ranges) = classified();
    let loose = Options::loose();

    let run: Box<dyn Fn()> = match workload {
        "parse" => Box::new(|| {
            let mut n = 0usize;
            for s in &all {
                if SemVer::new(s, loose).is_ok() {
                    n += 1;
                }
            }
            sink(n);
        }),
        // Valid inputs only: the mixed `parse` workload is dominated by failure
        // handling, which costs V8 a thrown exception and Rust an `Err`.
        "parse_valid" => Box::new(|| {
            let mut n = 0usize;
            for s in &versions {
                if SemVer::new(s, loose).is_ok() {
                    n += 1;
                }
            }
            sink(n);
        }),
        "satisfies" => Box::new(|| {
            let mut n = 0usize;
            for (i, v) in versions.iter().enumerate() {
                for k in 0..8 {
                    let r = &ranges[(i * 7 + k * 13) % ranges.len()];
                    if satisfies(v, r, Options::new()) {
                        n += 1;
                    }
                }
            }
            sink(n);
        }),
        "range_parse" => Box::new(|| {
            let mut n = 0usize;
            for r in &ranges {
                if Range::new(r, Options::new()).is_ok() {
                    n += 1;
                }
            }
            sink(n);
        }),
        "sort" => Box::new(|| {
            let sorted = f::sort(&versions, loose);
            sink(sorted.map(|v| v.len()).unwrap_or(0));
        }),
        "coerce" => Box::new(|| {
            let mut n = 0usize;
            for s in &all {
                if f::coerce(s, f::CoerceOptions::default()).is_some() {
                    n += 1;
                }
            }
            sink(n);
        }),
        // Diagnostic split of `coerce`: regex match only, no SemVer built.
        "coerce_regex_only" => Box::new(|| {
            let mut n = 0usize;
            for s in &all {
                if semver_npm::re::RE.safe(semver_npm::re::COERCE).captures(s).is_some() {
                    n += 1;
                }
            }
            sink(n);
        }),
        other => {
            eprintln!("unknown workload: {other}");
            std::process::exit(2);
        }
    };

    for _ in 0..warmup {
        run();
    }

    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let t = Instant::now();
        run();
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }

    let json: Vec<String> = samples.iter().map(|ms| format!("{ms:.6}")).collect();
    println!(
        "{{\"impl\":\"rust\",\"workload\":\"{workload}\",\"iterations\":{iterations},\"warmup\":{warmup},\"ms\":[{}]}}",
        json.join(",")
    );
}
