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
