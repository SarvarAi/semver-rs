# 5-minute demo script

Everything below is a command to run and a line to say. Nothing needs
investigating on camera — run `demo/setup.sh` once beforehand so the vendored
clone and dependencies are present, then work straight down this page.

Total: ~5:00. Timings are generous; the commands are fast.

---

## 0:00 — What this is (~25s)

> This is a Rust port of npm's semver library — `npm/node-semver`, pinned at
> commit `6e05b763`, which is tag v7.8.5. Track F, JavaScript to Rust.
>
> Rust already has a semver crate, but it implements SemVer 2.0.0 for Cargo. It
> doesn't implement npm's *range* dialect — X-ranges, npm's tilde and caret
> rules, hyphen ranges, `includePrerelease`, `coerce`. That's the gap this fills.

Show `README.md` top table on screen.

## 0:25 — One command to build (~20s)

```sh
cargo build --release
./target/release/semver -r '^1.2.3' 1.4.0
./target/release/semver -i preminor 1.2.3 --preid beta
```

> One command, no Makefile.

```sh
grep -A 3 'semver-probe' Cargo.toml
```

> And the harness is feature-gated — `required-features = ["harness"]` — so a
> default release build *cannot* produce the Node-comparing probe. On a fresh
> clone `cargo build --release` gives you exactly one binary.
>
> (In this working copy you'll see three, because I've already built the harness
> for the later sections. The clean-clone check at the end proves the default
> case.)

## 0:45 — The original suite's own fixtures, running against the port (~60s)

```sh
cargo test
```

> That's 18,310 calls derived from all fifteen of upstream's fixture files,
> replayed against the port — with the answers the *real* node-semver gave,
> recorded ahead of time. No Node needed to check it. 100% agreement.
>
> And this test right here asserts that all 43 regex tokens the port builds are
> byte-identical to upstream's live table — both the raw form and the
> ReDoS-hardened one it actually matches with.

Point at `tests/re_table.rs` in the output.

## 1:45 — Live, against real Node (~45s)

```sh
node tools/gen-calls.mjs sweep > /tmp/calls.jsonl
node tools/node-oracle.mjs < /tmp/calls.jsonl > /tmp/node.jsonl
./target/release/semver-probe < /tmp/calls.jsonl > /tmp/rust.jsonl
node tools/diff-results.mjs /tmp/calls.jsonl /tmp/node.jsonl /tmp/rust.jsonl
```

> 237,000 calls, both implementations, same questions. 100%.
>
> Worth saying: the generator never encodes an expected answer. It mines the
> fixtures for interesting inputs and lets upstream be the oracle — so this
> grades the port against the real thing, not against my reading of a fixture.

## 2:30 — The interesting edge case (~70s)

> Here's my favourite thing I found.

```sh
node -e "const s=require('./vendor/node-semver'); \
  console.log('node:', s.compare('1.0.0-9007199254740992','1.0.0-9007199254740993'))"

echo '{"id":1,"fn":"compare","a":["1.0.0-9007199254740992","1.0.0-9007199254740993",null]}' \
  | ./target/release/semver-probe
```

Both print `0`.

> node-semver says those two versions are **equal**. They're obviously not — but
> it compares numeric prerelease identifiers by coercing with JavaScript's `+`,
> and past 2 to the 53 a double can't tell them apart.
>
> Rust could trivially do better with a `u64`. Doing better would be a *parity
> bug* — I'm porting node-semver, not an idealised semver. So the port
> reproduces the precision loss on purpose, and there's a test asserting it
> stays reproduced.

Show `src/identifiers.rs` — the `reproduces_javascript_f64_precision_loss` test.

## 3:40 — Fuzzing, and the bugs it found (~60s)

```sh
tail -20 fuzz/log.txt
```

> 13 million calls, 150 seconds, zero divergences.
>
> But it earned its keep before that. It found three real bugs. The best one:
> upstream computes `+m + 1` on a range component and pastes the result straight
> into a comparator string — so for a 32-digit component it produces
> `9.007199254740994e+31`, in exponential notation, because that's what
> JavaScript's `String()` does. Rust's formatter writes it out longhand, so the
> two implementations disagreed about what the range even *was*.
>
> It also gave me twenty thousand *false* divergences first — which turned out to
> be two bugs in my own harness. Node's `readline` treats U+2028 as a line break,
> and `JSON.stringify` doesn't escape it.

Show `DECISIONS.md` D11 and D12.

## 4:40 — Benchmarks, honestly (~40s)

```sh
head -30 bench/results.json
```

> Cold start is 1.8× faster, sorting 2.6×. But look at `parse_valid` — the port
> is about **three times slower** at parsing valid versions.
>
> That's the cost of a deliberate choice: I mirrored upstream's 43 composed
> regexes instead of hand-writing a parser, and V8's regex engine beats Rust's on
> exactly that shape. A hand-written parser would probably win and would have put
> the 100% parity at risk. I kept the parity and I'm reporting the number that
> makes the port look worse.

## 5:20 — Close (~20s)

```sh
./tools/vendor.sh --verify
cat baseline/unsafe-audit.txt | tail -3
```

> Every one of the 154 upstream files still hashes to what it did before I wrote
> a line of code. No test or fixture was modified. Zero `unsafe` — and that's
> `#![forbid(unsafe_code)]`, a compile error rather than a promise.

---

## Pre-flight checklist

```sh
./demo/setup.sh          # vendor clone + npm install + release build
cargo test               # should be green
node tools/cli-parity.mjs   # should print 412/412
```

## If a command fails on camera

- `vendor/node-semver` missing → `./tools/vendor.sh`
- `semver-probe` missing → `cargo build --release --features harness`
- `node` not found → `export PATH="$HOME/.local/node/bin:$PATH"`
