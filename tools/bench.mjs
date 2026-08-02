#!/usr/bin/env node
// Benchmark orchestrator: runs the same workloads against the original
// node-semver and the Rust port, and writes bench/results.json.
//
//   node tools/bench.mjs [--iterations 60] [--warmup 5] [--cold 20]
//
// Reports distributions (p50/p90/p99/min), not just means — see
// bench/methodology.md for what is and is not controlled.

import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import os from 'node:os'
import path from 'node:path'
import fs from 'node:fs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const arg = (n, d) => {
  const i = process.argv.indexOf(`--${n}`)
  return i === -1 ? d : Number(process.argv[i + 1])
}

const ITERATIONS = arg('iterations', 60)
const WARMUP = arg('warmup', 5)
const COLD = arg('cold', 20)

const RUST_BENCH = path.join(ROOT, 'target', 'release', 'semver-bench')
const RUST_CLI = path.join(ROOT, 'target', 'release', 'semver')
const NODE_CLI = path.join(ROOT, 'vendor', 'node-semver', 'bin', 'semver.js')

for (const p of [RUST_BENCH, RUST_CLI]) {
  if (!fs.existsSync(p)) {
    process.stderr.write(`missing ${p}\nRun: cargo build --release --features harness\n`)
    process.exit(2)
  }
}

const WORKLOADS = ['parse', 'parse_valid', 'satisfies', 'range_parse', 'sort', 'coerce', 'coerce_regex_only']

const pct = (sorted, p) => sorted[Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length))]
const stats = (ms) => {
  const s = [...ms].sort((a, b) => a - b)
  return {
    min: +s[0].toFixed(4),
    p50: +pct(s, 50).toFixed(4),
    p90: +pct(s, 90).toFixed(4),
    p99: +pct(s, 99).toFixed(4),
    mean: +(s.reduce((a, b) => a + b, 0) / s.length).toFixed(4),
    samples: s.length,
  }
}

// --- warm throughput -------------------------------------------------------
const warm = {}
for (const w of WORKLOADS) {
  const n = spawnSync(process.execPath,
    [path.join(ROOT, 'tools', 'node-bench.mjs'), w, String(ITERATIONS), String(WARMUP)],
    { encoding: 'utf8', maxBuffer: 1 << 26 })
  const r = spawnSync(RUST_BENCH, [w, String(ITERATIONS), String(WARMUP)],
    { encoding: 'utf8', maxBuffer: 1 << 26 })
  if (n.status !== 0) { process.stderr.write(`node bench ${w} failed: ${n.stderr}\n`); process.exit(1) }
  if (r.status !== 0) { process.stderr.write(`rust bench ${w} failed: ${r.stderr}\n`); process.exit(1) }
  const nodeStats = stats(JSON.parse(n.stdout).ms)
  const rustStats = stats(JSON.parse(r.stdout).ms)
  warm[w] = {
    node: nodeStats,
    rust: rustStats,
    speedup_p50: +(nodeStats.p50 / rustStats.p50).toFixed(2),
    speedup_p99: +(nodeStats.p99 / rustStats.p99).toFixed(2),
  }
  process.stderr.write(`warm ${w.padEnd(12)} node p50=${nodeStats.p50}ms  rust p50=${rustStats.p50}ms  ${warm[w].speedup_p50}x\n`)
}

// --- cold start ------------------------------------------------------------
// Process launch to first useful answer, measured through the CLI both
// implementations ship, doing identical work.
const coldSamples = (cmd, args) => {
  const ms = []
  for (let i = 0; i < COLD; i++) {
    const t = process.hrtime.bigint()
    const r = spawnSync(cmd[0], [...cmd.slice(1), ...args], { encoding: 'utf8' })
    ms.push(Number(process.hrtime.bigint() - t) / 1e6)
    if (r.status !== 0) { throw new Error(`cold-start command failed: ${r.stderr}`) }
  }
  return ms
}

const COLD_ARGS = ['-r', '^1.2.3', '1.4.0']
const nodeCold = stats(coldSamples([process.execPath, NODE_CLI], COLD_ARGS))
const rustCold = stats(coldSamples([RUST_CLI], COLD_ARGS))
process.stderr.write(`cold ${'start'.padEnd(12)} node p50=${nodeCold.p50}ms  rust p50=${rustCold.p50}ms  ${(nodeCold.p50 / rustCold.p50).toFixed(2)}x\n`)

const rustcVersion = spawnSync('rustc', ['--version'], { encoding: 'utf8' }).stdout?.trim()
  ?? spawnSync(path.join(os.homedir(), '.cargo', 'bin', 'rustc'), ['--version'], { encoding: 'utf8' }).stdout?.trim()

const results = {
  generatedAt: new Date().toISOString(),
  machine: {
    platform: `${os.platform()} ${os.release()}`,
    arch: os.arch(),
    cpu: os.cpus()[0]?.model ?? 'unknown',
    cores: os.cpus().length,
    memoryGB: +(os.totalmem() / 1024 ** 3).toFixed(1),
  },
  toolchain: {
    node: process.version,
    rustc: rustcVersion ?? 'unknown',
    rustProfile: 'release (opt-level=3, lto=true, codegen-units=1)',
  },
  upstream: 'npm/node-semver 6e05b7637396ac66522cff8731f07cfe0ef49a29 (v7.8.5)',
  config: { iterations: ITERATIONS, warmup: WARMUP, coldRuns: COLD },
  note: 'All timings in milliseconds. See bench/methodology.md for confounders.',
  coldStart: {
    command: `semver ${COLD_ARGS.join(' ')}`,
    node: nodeCold,
    rust: rustCold,
    speedup_p50: +(nodeCold.p50 / rustCold.p50).toFixed(2),
  },
  warmThroughput: warm,
}

fs.mkdirSync(path.join(ROOT, 'bench'), { recursive: true })
fs.writeFileSync(path.join(ROOT, 'bench', 'results.json'), `${JSON.stringify(results, null, 2)}\n`)

// --- summary table ---------------------------------------------------------
const rows = [
  ['workload', 'node p50', 'rust p50', 'node p99', 'rust p99', 'speedup p50'],
  ['--------', '--------', '--------', '--------', '--------', '-----------'],
]
for (const w of WORKLOADS) {
  const e = warm[w]
  rows.push([w, `${e.node.p50}ms`, `${e.rust.p50}ms`, `${e.node.p99}ms`, `${e.rust.p99}ms`, `${e.speedup_p50}x`])
}
rows.push(['cold start', `${nodeCold.p50}ms`, `${rustCold.p50}ms`, `${nodeCold.p99}ms`, `${rustCold.p99}ms`,
  `${(nodeCold.p50 / rustCold.p50).toFixed(2)}x`])

const widths = rows[0].map((_, i) => Math.max(...rows.map((r) => String(r[i]).length)))
process.stdout.write('\n')
for (const r of rows) {
  process.stdout.write(`${r.map((c, i) => String(c).padEnd(widths[i])).join('  ')}\n`)
}
process.stdout.write(`\nwrote bench/results.json\n`)
