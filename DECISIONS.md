# Decisions

Engineering decisions made while porting `npm/node-semver` (JavaScript) to Rust,
written as they happened rather than reconstructed afterwards. Timestamps are
UTC and correspond to the commit that carried the change.

Contest window: kickoff 2026-07-31 18:00 UTC, freeze 2026-08-03 18:00 UTC.
Work on this port began 2026-08-02 ~17:47 UTC (~24 h of runway).

---

## D1 — Port target: `npm/node-semver`, not "semver" generically

**2026-08-02 17:50 UTC**

Rust already has an excellent `semver` crate (dtolnay/semver). It implements
**SemVer 2.0.0 as Cargo needs it** and deliberately stops there.

What it does *not* implement, and what this port therefore targets:

| npm feature | in dtolnay/semver? |
|---|---|
| X-ranges (`1.2.x`, `1.x`, `*`, `""`) | no |
| npm tilde bump rules (`~1.2.3` → `>=1.2.3 <1.3.0-0`) | no |
| npm caret bump rules incl. the `0.x` / `0.0.x` special cases | no |
| Hyphen ranges (`1.2.3 - 2.3.4`) | no |
| `includePrerelease` semantics | no |
| `coerce()` / `coerce(rtl)` | no |
| `loose` parsing mode (`v1.2.3`, `1.2.3alpha`, `=1.2.3`) | no |
| The npm CLI surface (`bin/semver.js`) | no |

Cargo's own `VersionReq` is a *different dialect* with overlapping syntax and
non-overlapping meaning — e.g. Cargo treats a bare `1.2.3` requirement as
caret-like, npm treats it as exact equality. Porting npm's grammar is a genuine
gap, not a re-implementation of existing Rust work.

## D2 — Pin the source at a tagged commit, not `main`

**2026-08-02 17:58 UTC**

Cloned and checked out `6e05b7637396ac66522cff8731f07cfe0ef49a29`. That commit
happens to be exactly tag `v7.8.5` (released 2026-06-19), so the pin is both a
content hash and a human-meaningful release. Pinning to `main` would have made
the parity numbers unreproducible the moment upstream merged anything.

## D3 — Do not commit the upstream tree; prove it by hash instead

**2026-08-02 18:05 UTC**

Two ways to let a judge verify "the original test suite is untouched":

1. Vendor all 154 upstream files into this repo and invite a diff.
2. Keep the clone out of the repo, and commit a SHA-256 manifest of every
   upstream file captured *before any port code existed*.

Chose (2). It is strictly stronger evidence — a manifest can be re-verified
against upstream at any time with `./tools/vendor.sh --verify`, whereas a
vendored copy only proves it matches itself. It also keeps 1.4 MB of somebody
else's ISC code out of this repository.

`baseline/KICKOFF-HASHES.txt` — 154 files, manifest self-hash
`6493fb1398d584d1af260dfc38be8661db037dfa3d41e72c61c3ceeb0b3fac9c`.

**Edits made to upstream test files or fixtures so far: none.** The only thing
written into `vendor/` is `node_modules/`, installed with
`npm install --no-save --no-package-lock` specifically so that no upstream
tracked file — including a would-be `package-lock.json` — is created or modified.

## D4 — The differential harness is feature-gated out of the shipped artifact

**2026-08-02 18:10 UTC**

The rules forbid shelling out to Node or linking V8 **in the port**, while the
differential test harness is expected to run the original side-by-side. Those
two facts have to be visibly separated, not just asserted in prose.

`Cargo.toml` therefore declares the probe binary as:

```toml
[[bin]]
name = "semver-probe"
required-features = ["harness"]
```

so `cargo build --release` *cannot* build it. The shipped surface is
`src/lib.rs` + `src/bin/semver.rs`, and neither has any Node dependency of any
kind. A judge can confirm this in one glance at `Cargo.toml`, and by observing
that `target/release/` contains exactly one binary after a default build.

## D5 — Port `safeRe`, not `re`

**2026-08-02 18:15 UTC**

`internal/re.js` builds **two** parallel regex tables. `makeSafeRegex` rewrites
unbounded quantifiers into bounded ones to blunt ReDoS:

| token | becomes |
|---|---|
| `\s*` | `\s{0,1}` |
| `\s+` | `\s{1,1}` |
| `\d*` / `\d+` | `\d{0,256}` / `\d{1,256}` |
| `[a-zA-Z0-9-]*` / `+` | `{0,250}` / `{1,250}` |

Every class in the library (`SemVer`, `Comparator`, `Range`) imports
`{ safeRe: re }` — the bounded table. The unbounded `re` is exported only for
userland.

These bounds are **behavioural, not cosmetic**. `\s+` → `\s{1,1}` means exactly
one space, and `\d{1,256}` means a 300-digit major version fails to match. A
port that used the "obvious" unbounded regexes would silently diverge on long
and multi-space inputs. So this port mirrors `safeRe`, and mirrors it the same
way upstream builds it: compose the token from the **unsafe** `src` table, then
apply the safe transform to the composed string.

One deliberate exception, faithfully reproduced: `classes/range.js` builds its
own `BUILDSTRIPRE` from the **unbounded** `src[t.BUILD]` with a global flag, and
upstream comments it as such. This port keeps that asymmetry.

## D6 — `parseOptions` is a passthrough; only truthiness matters

**2026-08-02 18:18 UTC**

Reading `internal/parse-options.js`, it does far less than the name suggests:

```js
if (!options) { return emptyOpts }          // null, undefined, 0, false, ''
if (typeof options !== 'object') { return looseOption }  // true, 123, 'x'
return options                              // objects pass through UNCHANGED
```

There is no normalization. Downstream, every consumer reads `!!options.loose`
and `!!options.includePrerelease`. This explains the otherwise baffling values
in upstream fixtures: `/asdf/`, `{ loose: 123 }`, `{ loose: null }`,
`{ loose: 0 }`, bare `true`, `undefined`.

The resolution rules that matter:

- `/asdf/` is `typeof "object"` with no `.loose` → **loose: false**
- bare `true` is not an object → **loose: true**
- `{ loose: 123 }` → truthy → **true**; `{ loose: 0 }` / `{ loose: null }` → **false**

Rust models this as a plain `Options { loose: bool, include_prerelease: bool }`,
because the two booleans are the entire observable surface. The exotic JS values
are resolved once, at corpus-extraction time, by running them through upstream's
*own* `parseOptions`. The corpus records both the original source spelling and
the resolved booleans so the reduction is auditable rather than assumed.

## D7 — Replicate JavaScript's f64 identifier coercion, bugs included

**2026-08-02 18:22 UTC**

`internal/identifiers.js` compares two numeric-looking prerelease identifiers by
coercing with JS `+a`, i.e. IEEE-754 double:

```js
if (anum && bnum) { a = +a; b = +b }
return a === b ? 0 : ...
```

Above 2^53 that coercion is lossy, so upstream reports
`1.0.0-9007199254740993` and `1.0.0-9007199254740992` as **equal**.

`classes/semver.js` compounds it: a numeric prerelease identifier is only stored
as a number when `num >= 0 && num < MAX_SAFE_INTEGER` (strict `<`), so anything
at or above 9007199254740991 stays a *string* and then hits exactly that lossy
comparison path.

Rust could trivially do better with `u64` or a bignum. **That would be a parity
bug.** The contract being ported is "behave like node-semver", not "behave like
an idealised semver". So `compare_identifiers` parses to `f64` and compares
`f64`, reproducing the precision loss deliberately. This is called out in the
README limitations section as intended behaviour, not an oversight.

## D8 — Mirror the regex table with the `regex` crate rather than hand-rolling a parser

**2026-08-02 18:25 UTC**

A hand-written recursive-descent parser would benchmark faster and carry zero
dependencies. It would also be the single largest source of silent divergence in
the whole port, because the observable behaviour of node-semver *is* the
behaviour of 44 specific regexes composed out of each other.

Verified by scan that `internal/re.js` uses no lookahead, no backreference and
no named groups, so the table is expressible in the `regex` crate's RE2-style
engine with no dialect gap. The port therefore builds the same 44 tokens by the
same composition, in the same order, and applies the same `makeSafeRegex`
transform — a judge can diff the token table against the original line by line.

Parity is the thing being scored; a faster parser that is subtly wrong is worth
less than a mirror that is right. Rust still wins the benchmark comfortably
without the extra risk (see `bench/`).

## D9 — Translate the JS regex dialect rather than trusting `\d` and `\s`

**2026-08-02 18:30 UTC**

The token *sources* are byte-identical to upstream's (D8, proven by
`tests/re_table.rs`), but what gets **compiled** cannot be, because the two
engines disagree about two escapes:

| escape | JavaScript | Rust `regex` |
|---|---|---|
| `\d` | exactly `[0-9]` | Unicode `\p{Nd}` — Arabic-Indic digits and more |
| `\s` | ECMAScript WhiteSpace + LineTerminator, **including U+FEFF** | Unicode `White_Space`, which **excludes** U+FEFF |

Left alone, a version numbered with Arabic-Indic digits would parse in the port
and not upstream, and a BOM-prefixed version would trim in upstream and not the
port. So `re::js_to_rust_dialect` rewrites both escapes into explicit classes.

The rewrite has to be character-class aware: `\s` appears bare (`^\s*>=`) *and*
inside a class (`[v=\s]*`). Splicing `[...]` into the latter would be a syntax
error, so inside a class only the body is inserted. `SemVer::js_trim` spells the
same set out for `String.prototype.trim`, whose whitespace definition is the
same one `\s` matches.

## D10 — The differential oracle grades against upstream, never against my reading

**2026-08-02 18:55 UTC**

The obvious way to use the fixtures is to hand-write Rust assertions: "for each
row in `range-include.js`, assert `satisfies(version, range) == true`". That
tests my *interpretation* of the fixture as much as the port.

`tools/gen-calls.mjs` instead mines the fixtures purely for interesting
`(version, range, options)` triples, and never encodes an expected answer. Both
implementations are asked the same questions and the answers are compared. When
the port and upstream disagree, upstream is right by definition — which is the
only definition that matters for a port.

This also made the corpus far larger than the fixtures alone: 982 fixture rows
expand to 18,310 calls, and Corpus B's 1,133 harvested strings expand to 237,028.

## D11 — Two harness bugs that masqueraded as port bugs

**2026-08-02 18:35 UTC**

The first fuzz run reported 20,188 divergences in 640,000 calls. None were real.

**U+2028 / U+2029.** `JSON.stringify` leaves those two characters raw, but Node's
`readline` treats them as line terminators. Any call whose payload contained one
was split into two unparseable halves and its result vanished, which the differ
correctly reported as "no result". Fixed by escaping them on the wire in every
JS producer and in the Rust probe's output. Same value, transport-safe.

**Chunk-boundary decoding.** The fuzzer accumulated child stdout with
`out += chunk`, which decodes each `Buffer` independently — so a multi-byte
UTF-8 character landing on a chunk boundary came back as replacement characters
and looked like a divergence. Fixed by collecting `Buffer`s and concatenating
once.

Worth recording because the instinct on seeing 20,000 failures is to start
"fixing" the port. Both bugs were in the measuring instrument, and three genuine
bugs (D12) were hiding underneath the noise.

## D12 — Three real bugs the fuzzer found, and why each was wrong

**2026-08-02 18:40 UTC**

Once the harness was trustworthy, 640k calls yielded 26 divergences with three
distinct causes.

**1. Number formatting above 1e21.** `replaceXRange` computes `+m + 1` and
interpolates the result straight into a comparator string. For a 32-digit
component upstream produces `<0.9.007199254740994e+31.0-0` — exponential
notation, because that is what `String(9.007199254740994e31)` gives. Rust's `{}`
writes `90071992547409940000000000000000`, so the two implementations disagreed
about what the range *was*. `util::js_number_to_string` now implements
ECMA-262 `Number::toString` properly.

**2. Null-set short-circuit ordering.** `parseRange` builds the entire comparator
array with `.map()` and only then scans it for `<0.0.0-0`. The port checked for
the null set inside the construction loop, so an invalid comparator *after* a
null-set one never got constructed and never threw. Upstream throws. Fixed by
collecting first.

**3. Re-parsing on an options mismatch.** `new SemVer(existingSemVer, options)`
returns the same object when the flags match and otherwise **re-parses from
`.version`** — which can fail. A version parsed leniently can canonicalise to
something strict parsing rejects, e.g. a prerelease identifier of
`009007199254740991` (kept as a string because it is not below
`MAX_SAFE_INTEGER`, and invalid strictly because of the leading zeroes).
`minVersion` and `outside` both hit this. `SemVer::with_options` now models the
constructor's instance branch, and `Comparator::test` / `Range::test` became
fallible to carry the error.

The sharpest part of (3): upstream's `outside` passes `options` to its `gtfn`
call but **not** to `ltfn`, so the latter re-parses strictly. That asymmetry
looks like an upstream oversight, but it is observable, so the port reproduces it.

## D13 — Two performance defects, found by benchmarking rather than guessed at

**2026-08-02 18:50 UTC**

The first benchmark run had the port *losing* on `range_parse` (0.66x) and
`satisfies` (0.76x). Both traced to the same area:

- `LruCache::get` scanned a `VecDeque` to find the key — O(n) with n up to 1000,
  where upstream gets O(1) from `Map.delete` + `Map.set`. Replaced with a
  monotonic tick plus a `BTreeMap` ordering.
- Every cache hit deep-cloned the comparator list. Upstream hands back the
  cached array *by reference*. `Range::set` now holds `Arc<Vec<Comparator>>`.

`range_parse` 0.66x → 0.88x, `satisfies` 0.76x → 0.92x, with parity unchanged.
`Arc` rather than `Rc` so `Range` stays `Send + Sync`; the cache is thread-local,
so the atomics are uncontended.

## D14 — Report the benchmark that makes the port look worse

**2026-08-02 19:00 UTC**

The initial `parse` benchmark showed the port 3.4x faster. That number was close
to meaningless: two thirds of Corpus B is invalid input, so the workload mostly
measured V8 constructing and throwing exceptions against Rust returning an `Err`.

Adding `parse_valid` — the same workload restricted to inputs that actually
parse — inverts the result to roughly **0.31x, i.e. the port is ~3x slower**.
`coerce_regex_only` isolates it further: the raw COERCE match alone is ~2x slower
than V8's Irregexp.

That is the real cost of D8. Mirroring 43 composed regexes, with `safeRe`'s
bounded quantifiers expanding into large automata, is slower than a JIT-compiled
backtracking engine on exactly this shape of pattern. A hand-written parser would
very likely win, and would have put the 100% parity result at risk.

Both framings are in `bench/results.json` and the README. Keeping only the
flattering one would have been the easiest thing to do and the least honest.

## D15 — `String.prototype.length` is UTF-16, and the port was measuring code points

**2026-08-02 18:50 UTC**

`SemVer`'s length guard used `version.chars().count()`. Upstream tests
`version.length`, which is **UTF-16 code units** — the two differ for any astral
character, where JS counts 2 and code points count 1. A 200-emoji string is
over the 256 limit for upstream and under it for the port.

Now checked with `encode_utf16().count()`, short-circuited by a byte-length test
because UTF-8 length is always ≥ UTF-16 length, so the common case never pays for
the scan. Found by reading, not by the fuzzer — the generator does not emit
astral characters, which is a real gap in it worth naming.

## D16 — `cargo test` must prove parity without Node

**2026-08-02 18:20 UTC**

The differential harness needs Node, a vendored clone and an `npm install`. A
judge should not need any of that to check the headline claim.

`tests/original/golden/fixtures.jsonl` therefore records all 18,310
fixture-derived calls *together with the answer the real node-semver gave*, and
`tests/golden_parity.rs` replays them. Plain `cargo test` on a fresh clone proves
parity with no Node, no npm and no network. Reproducing the harness is only
needed to re-derive the corpus, not to verify it.

The dispatch table is `include!`d from `src/probe_dispatch.rs` by both the live
Node probe and the offline replay, specifically so the two cannot drift — if they
did, the golden corpus would silently stop proving anything. It is `include!`d
rather than exposed as a module so that the shipped library never acquires a
`serde_json` dependency.

## D17 — Upstream's LRU deletes on re-set, and the port keeps the quirk

**2026-08-02 19:05 UTC**

`LRUCache.set` reads:

```js
const deleted = this.delete(key)
if (!deleted && value !== undefined) { /* ...insert... */ }
```

So setting a key that is already present **removes** it and does not re-insert.
That is almost certainly not the intent, but it is unreachable in practice:
`Range.parseRange` only calls `set` after a `get` miss, and cached values are
always truthy arrays. Reproduced faithfully (with a test asserting the quirk) and
not reported upstream, because there is no input that reaches it.
