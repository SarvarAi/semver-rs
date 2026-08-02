# Benchmark methodology

The claim being tested is narrow and worth stating plainly: **how much faster is
this port than the original at the work node-semver actually does?** Not "Rust is
faster than JavaScript."

## What is measured

Two regimes, because they answer different questions and a single number hides
the interesting half:

**Cold start** — process launch to first useful answer. This is what a CLI user
or a short-lived script feels. It is dominated by runtime startup, not by semver
work, and it is where the two implementations differ most.

**Warm throughput** — steady-state operations per second inside one process,
after any JIT warm-up. This is what a long-running program (a package manager
resolving a dependency graph) feels.

## Workload

Every measurement runs the same shared workload, drawn from the same corpus both
implementations are tested against elsewhere:

| Benchmark | Work |
|---|---|
| `parse` | Parse every string in Corpus B (~65% of them invalid) |
| `parse_valid` | Parse only the strings that *are* valid versions |
| `satisfies` | Match every version against a rotating window of ranges |
| `range_parse` | Construct `Range` objects from every valid range string |
| `sort` | Sort the full version list with `compareBuild` |
| `coerce` | Coerce every corpus string, left-to-right |
| `coerce_regex_only` | The COERCE regex match alone, no SemVer constructed |

Inputs come from `tests/original/corpus-b-strings.json` — the 1,133 strings
harvested from upstream's own test suite. Using real inputs rather than
synthetic ones matters here: a hot loop over `"1.2.3"` would measure the caches,
not the parser.

## Why there are two `parse` rows and a `coerce` split

The first version of this benchmark reported `parse` alone and showed the port
winning by 3.4x. That number was mostly an artefact: two thirds of Corpus B is
invalid input, so the workload was dominated by V8 constructing and throwing
exceptions versus Rust returning an `Err`. It measured error handling, not
parsing.

`parse_valid` restricts the same workload to inputs that actually parse, and the
result inverts — the port is roughly 3x *slower*. `coerce_regex_only` isolates
the same effect one layer down: it times the COERCE regex match with no version
constructed, and the port is about 2x slower there too.

Both rows are reported. A benchmark that quietly kept only the flattering
framing would be worse than no benchmark.

## How the numbers are produced

- **Distribution, not just the mean.** Each benchmark reports p50, p90, p99 and
  min across repeated iterations. A mean alone would hide GC pauses on the Node
  side and would make the comparison look cleaner than it is.
- **JIT warm-up is explicit.** The Node side runs untimed warm-up iterations
  before measurement begins, and the count is reported. Skipping this would
  overstate the Rust advantage substantially.
- **Rust is measured in release mode** (`opt-level = 3`, LTO, one codegen unit).
  Benchmarking a debug build would overstate it in the other direction.
- **Caches are acknowledged.** Both implementations memoize range parsing
  (upstream's `LRUCache`, and the port's mirror of it). The `range_parse`
  benchmark therefore measures cache-warm behaviour, which is the realistic case
  and is the same on both sides.
- **Same machine, same session**, run back to back.

## Confounders that are not controlled

Stated so the numbers are read correctly:

- **Process-level noise.** No CPU pinning, no disabled turbo/thermal scaling.
  Runs on a laptop; p99 in particular reflects that.
- **Cold-start is not apples-to-apples in kind.** Node must initialise V8 and
  load ~40 CommonJS modules; the Rust binary must page in one static executable
  and lazily compile 43 regexes on first use. Both numbers are honest measures of
  "time to first answer", but the work behind them is not the same work.
- **The first Rust call pays for the regex table.** `LazyLock` compiles all 43
  tokens on first use. That cost lands in cold start, and is deliberately *not*
  amortised out of it.
- **Allocation strategy differs.** The port returns owned `String`s in several
  places where upstream returns interned JS strings. No attempt was made to
  optimise this away, because doing so would have meant diverging from
  upstream's structure.
- **Single machine, single OS.** Results are Apple Silicon / macOS only.

## Reproducing

```sh
cargo build --release --features harness
node tools/bench.mjs            # writes bench/results.json and prints a summary
```

Results in `bench/results.json` carry the machine, OS, Node version, rustc
version and timestamp of the run that produced them.
