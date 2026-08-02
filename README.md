# semver-rs — a Rust port of `npm/node-semver`

A line-for-line port of npm's semantic-version library from JavaScript to Rust,
built for **Port Mortem 2026, Track F**.

| | |
|---|---|
| **Source** | [`npm/node-semver`](https://github.com/npm/node-semver) |
| **Pinned commit** | `6e05b7637396ac66522cff8731f07cfe0ef49a29` — exactly tag `v7.8.5` (2026-06-19) |
| **License** | ISC (permissive, OSI-approved; this port is ISC too) |
| **Track** | F — JavaScript → Rust |
| **Source size** | 2,004 lines of code across 49 files (`cloc`, port scope only) |
| **`unsafe` blocks** | **0** — measured, see [`baseline/unsafe-audit.txt`](baseline/unsafe-audit.txt) |

## Build and run

```sh
cargo build --release
./target/release/semver --help
./target/release/semver -r '^1.2.3' 1.4.0        # -> 1.4.0
./target/release/semver -i preminor 1.2.3 --preid beta   # -> 1.3.0-beta.0
```

```sh
cargo test          # 18,310 recorded upstream answers replayed. No Node needed.
```

That is the whole build. No Makefile, no codegen step, no network, and no Node
required to verify the parity claim — see [Proof](#proof) for why.

---

## Why this port

Rust already has an excellent `semver` crate (dtolnay/semver). It implements
**SemVer 2.0.0 as Cargo needs it**, and deliberately stops there. It does not
implement npm's range dialect, which is a substantially richer grammar:

| npm feature | in `dtolnay/semver`? |
|---|---|
| X-ranges — `1.2.x`, `1.x`, `*`, `""` | no |
| npm tilde rules — `~1.2.3` → `>=1.2.3 <1.3.0-0` | no |
| npm caret rules, including the `0.x` and `0.0.x` special cases | no |
| Hyphen ranges — `1.2.3 - 2.3.4` | no |
| `includePrerelease` semantics | no |
| `coerce()`, including right-to-left mode | no |
| `loose` parsing — `v1.2.3`, `1.2.3alpha`, `=1.2.3` | no |
| `subset()`, `simplifyRange()`, `minVersion()` | no |
| The `semver` CLI | no |

Cargo's `VersionReq` is a *different dialect* that shares syntax but not meaning:
Cargo reads a bare `1.2.3` requirement as caret-like, npm reads it as exact
equality. Anything that has to reason about npm version ranges from Rust — a
registry mirror, an audit tool, a resolver — currently cannot.

---

## Proof

Parity is not asserted here; it is measured, three independent ways.

### 1. Offline replay — `cargo test`

`tests/original/golden/fixtures.jsonl` records all 18,310 fixture-derived calls
**together with the answer the real node-semver v7.8.5 gave for each**. Plain
`cargo test` replays every one against the port.

| upstream fixture | rows | calls | agreement |
|---|---:|---:|---:|
| `comparator-intersection.js` | 34 | 136 | 136/136 |
| `comparisons.js` | 31 | 1,426 | 1,426/1,426 |
| `equality.js` | 37 | 1,702 | 1,702/1,702 |
| `increments.js` | 133 | 133 | 133/133 |
| `invalid-versions.js` | 10 | 336 | 336/336 |
| `range-exclude.js` | 98 | 2,430 | 2,430/2,430 |
| `range-include.js` | 126 | 3,150 | 3,150/3,150 |
| `range-intersection.js` | 54 | 324 | 324/324 |
| `range-parse.js` | 133 | 665 | 665/665 |
| `truncations.js` | 27 | 27 | 27/27 |
| `valid-versions.js` | 22 | 1,056 | 1,056/1,056 |
| `version-gt-range.js` | 56 | 1,400 | 1,400/1,400 |
| `version-lt-range.js` | 58 | 1,450 | 1,450/1,450 |
| `version-not-gt-range.js` | 80 | 2,000 | 2,000/2,000 |
| `version-not-lt-range.js` | 83 | 2,075 | 2,075/2,075 |
| **total** | **982** | **18,310** | **18,310 / 18,310 — 100.0000%** |

Zero value divergences, zero throw-vs-return divergences, zero error-message
divergences, zero panics.

`tests/re_table.rs` additionally asserts that all **43 regex tokens** the port
builds are byte-identical to upstream's live table — both the raw `src` form and
the ReDoS-hardened `safeSrc` form — comparing against a dump taken straight out
of the running `internal/re.js`.

### 2. Live differential oracle — port vs. real Node

`src/bin/semver-probe.rs` and `tools/node-oracle.mjs` speak the same JSONL
protocol; `tools/diff-results.mjs` compares them call for call.

| corpus | calls | agreement |
|---|---:|---:|
| Fixture-derived (Corpus A) | 18,310 | **100.0000%** |
| Harvested sweep (Corpus B) | 237,028 | **100.0000%** |

Corpus B is 1,133 string literals harvested from all 66 upstream test files —
most of upstream's assertions are inline rather than in fixtures, so this is what
actually drives coverage.

Neither corpus encodes an expected answer. Both implementations are asked the
same questions and their answers compared, so the port is graded against the real
thing rather than against anyone's reading of what a fixture meant.

### 3. Differential fuzzing

`tools/fuzz.mjs` generates seeded, reproducible inputs and runs both
implementations continuously. See [`fuzz/log.txt`](fuzz/log.txt) for the
timestamped log.

The generator over-weights where a JS→Rust port is most likely to be wrong:
integers straddling 2^53, the full ECMAScript whitespace set including U+FEFF,
lengths at the `MAX_LENGTH` and `safeRe` bounds, leading-zero identifiers, and
character-level mutations of real corpus strings.

**It found real bugs.** Three of them, all fixed — see D12 in
[`DECISIONS.md`](DECISIONS.md): JS exponential number formatting above 1e21
leaking into range strings; a null-set short-circuit that skipped a throw
upstream performs; and `new SemVer(existingSemVer, options)` re-parsing (and
sometimes failing) when the options flags differ. It also produced 20,188
*false* divergences first, which turned out to be two bugs in the harness itself
(D11) — worth knowing before trusting any fuzzer's output.

### 4. CLI parity

`tools/cli-parity.mjs` runs both binaries over 412 invocations — every flag, the
`--flag=value` form, all release levels, error paths, sorting, coercion — and
compares **stdout, stderr and exit code**.

**412 / 412 identical.** `--help` is byte-identical to upstream's.

---

## Benchmarks

Apple M3, macOS 25.5.0 arm64, Node v24.18.1, rustc 1.97.1, release profile
(`opt-level=3`, LTO, one codegen unit). Full numbers in
[`bench/results.json`](bench/results.json); methodology and confounders in
[`bench/methodology.md`](bench/methodology.md).

| workload | node p50 | rust p50 | speedup |
|---|---:|---:|---:|
| cold start (`semver -r '^1.2.3' 1.4.0`) | 30.63 ms | 17.05 ms | **1.80×** |
| `sort` (compareBuild over all versions) | 0.904 ms | 0.344 ms | **2.63×** |
| `parse` (mixed valid/invalid input) | 1.452 ms | 0.433 ms | **3.35×** |
| `satisfies` | 3.114 ms | 3.387 ms | 0.92× |
| `range_parse` | 0.267 ms | 0.305 ms | 0.88× |
| `coerce_regex_only` | 0.061 ms | 0.131 ms | 0.47× |
| `coerce` | 0.184 ms | 0.561 ms | 0.33× |
| `parse_valid` (valid input only) | 0.081 ms | 0.263 ms | **0.31×** |

**The port is slower at the thing it does most.** That deserves to be the
headline rather than a footnote.

The flattering `parse` row is largely an artefact: two thirds of the corpus is
invalid input, so it mostly measures V8 throwing exceptions against Rust
returning an `Err`. `parse_valid` restricts the workload to input that actually
parses and the result inverts — the port is about 3× slower. `coerce_regex_only`
isolates the cause one layer down: the raw regex match alone is ~2× slower than
V8's Irregexp.

The reason is a deliberate trade (D8): this port mirrors upstream's 43 composed
regexes rather than hand-writing a parser, and `safeRe`'s bounded quantifiers
(`{0,256}`, `{0,250}`) expand into large automata that Rust's `regex` crate
handles less efficiently than a JIT-compiled backtracking engine handles them. A
hand-written parser would very likely win, and would have put the 100% parity
result at risk. Parity was the goal; this is what it cost.

Rust wins where the work is not regex-bound: process startup, sorting, and error
paths.

---

## Untouched upstream

The rules ask that the original test suite and fixtures be left alone.
**No upstream file was modified — not one, and no edits are being disclosed
because there are none.**

`baseline/KICKOFF-HASHES.txt` is a SHA-256 manifest of all 154 upstream tracked
files, captured before a single line of port code existed. Re-verify it any time:

```sh
./tools/vendor.sh --verify     # clones the pinned commit and checks every hash
```

`baseline/kickoff-tap-output.txt` is the original suite's own output at that
commit, for reference: **10,471 assertions passing, 0 failing, 51 test files.**

Dev dependencies were installed with `npm install --no-save --no-package-lock`
specifically so that no upstream tracked file — including a would-be
`package-lock.json` — could be created or modified.

## Separation from the harness

The rules forbid the *port* from shelling out to Node or linking V8, while the
differential harness is expected to run the original side by side. Those are kept
architecturally separate, not merely promised apart:

- `cargo build --release` produces **one** binary, `semver`, from `src/lib.rs` +
  `src/bin/semver.rs`. Neither has any Node dependency of any kind.
- The probe and benchmark binaries are declared `required-features = ["harness"]`
  in `Cargo.toml`, so a default release build **cannot** produce them.

```sh
cargo build --release && ls target/release/semver*        # just `semver`
cargo build --release --features harness                  # opts into the harness
```

## Known limitations

Stated plainly rather than buried.

- **JavaScript's f64 precision loss is reproduced on purpose.**
  `1.0.0-9007199254740993` and `1.0.0-9007199254740992` compare **equal**,
  because upstream coerces numeric prerelease identifiers with `+a` and doubles
  cannot tell those apart. Rust could do better with `u64`; doing better would be
  a parity bug (D7).
- **`major`/`minor`/`patch` are `f64`, not integers.** Same reason — upstream
  stores JS numbers and range-checks against `MAX_SAFE_INTEGER`, and an
  over-long component coerces to `Infinity` there rather than failing to parse.
- **Slower than the original at parsing and coercion**, quantified above.
- **`Range::set` is `Vec<Arc<Vec<Comparator>>>`.** An implementation detail
  visible in the public API, adopted because upstream returns memoized comparator
  arrays by reference and deep-cloning them was measurably slower (D13).
- **The fuzzer does not generate astral-plane characters.** A UTF-16 length bug
  (D15) was found by reading rather than by fuzzing, which means that class of
  input is still under-tested.
- **`sort`'s error message depends on V8's sort order.** When a list contains
  more than one invalid version, which one gets named is determined by the order
  TimSort happens to compare elements. The port reproduces V8's probe order for
  the first comparison, which covers every case the corpus and fuzzer produce,
  but it is an emulation of an implementation detail rather than of a spec.
- Benchmarks are single-machine, Apple Silicon only, with no CPU pinning.

## Repository layout

```text
├── src/
│   ├── lib.rs                 constants, re, identifiers, options, util
│   ├── semver.rs              classes/semver.js
│   ├── comparator.rs          classes/comparator.js
│   ├── range.rs               classes/range.js
│   ├── functions.rs           functions/*.js
│   ├── ranges.rs              ranges/*.js  (outside, minVersion, simplify, ...)
│   ├── subset.rs              ranges/subset.js
│   ├── lrucache.rs            internal/lrucache.js
│   ├── probe_dispatch.rs      shared harness dispatch (include!d, not a module)
│   └── bin/
│       ├── semver.rs          the shipped CLI  (bin/semver.js)
│       ├── semver-probe.rs    harness-only differential probe
│       └── semver-bench.rs    harness-only benchmark driver
├── tests/
│   ├── golden_parity.rs       offline replay of 18,310 upstream answers
│   ├── re_table.rs            43-token regex table equivalence
│   └── original/              extracted corpora + golden answers
├── tools/                     vendor, extract, harvest, oracle, fuzz, bench, cli-parity
├── baseline/                  kickoff hashes, original tap output, unsafe audit
├── bench/                     methodology.md, results.json
├── fuzz/                      log.txt
├── demo/                      script.md
└── DECISIONS.md               17 entries, written as the work happened
```

## Attribution

Ported from [`npm/node-semver`](https://github.com/npm/node-semver), Copyright
Isaac Z. Schlueter and Contributors, ISC License. This port is distributed under
the same license; see [`LICENSE`](LICENSE).
