#!/usr/bin/env node
// Node side of the benchmark, running the ORIGINAL node-semver over exactly the
// same workload as src/bin/semver-bench.rs. Dev-time tool only.
//
//   node tools/node-bench.mjs <workload> <iterations> <warmup>

import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
import fs from 'node:fs'

const require = createRequire(import.meta.url)
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const semver = require(path.join(ROOT, 'vendor', 'node-semver', 'index.js'))
const Range = require(path.join(ROOT, 'vendor', 'node-semver', 'classes', 'range.js'))
const SemVer = require(path.join(ROOT, 'vendor', 'node-semver', 'classes', 'semver.js'))

const workload = process.argv[2] ?? 'parse'
const iterations = Number(process.argv[3] ?? 50)
const warmup = Number(process.argv[4] ?? 3)

const corpus = JSON.parse(
  fs.readFileSync(path.join(ROOT, 'tests', 'original', 'corpus-b-strings.json'), 'utf8')
)
const all = corpus.entries.map((e) => e.s)
const versions = corpus.entries.filter((e) => e.looseVersion || e.strictVersion).map((e) => e.s)
const ranges = corpus.entries.filter((e) => e.validRange).map((e) => e.s)

// Keep a reference so V8 cannot eliminate the measured work.
let sink = 0

const RUNS = {
  parse () {
    let n = 0
    for (const s of all) {
      try { new SemVer(s, true); n += 1 } catch { /* invalid */ }
    }
    sink += n
  },
  // Valid inputs only — see the Rust side.
  parse_valid () {
    let n = 0
    for (const s of versions) {
      try { new SemVer(s, true); n += 1 } catch { /* invalid */ }
    }
    sink += n
  },
  coerce_regex_only () {
    const { safeRe, t } = require(path.join(ROOT, 'vendor', 'node-semver', 'internal', 're.js'))
    let n = 0
    for (const s of all) { if (safeRe[t.COERCE].exec(s)) { n += 1 } }
    sink += n
  },
  satisfies () {
    let n = 0
    for (let i = 0; i < versions.length; i++) {
      for (let k = 0; k < 8; k++) {
        const r = ranges[(i * 7 + k * 13) % ranges.length]
        if (semver.satisfies(versions[i], r)) { n += 1 }
      }
    }
    sink += n
  },
  range_parse () {
    let n = 0
    for (const r of ranges) {
      try { new Range(r); n += 1 } catch { /* invalid */ }
    }
    sink += n
  },
  sort () {
    sink += semver.sort(versions.slice(), true).length
  },
  coerce () {
    let n = 0
    for (const s of all) { if (semver.coerce(s)) { n += 1 } }
    sink += n
  },
}

const run = RUNS[workload]
if (!run) {
  process.stderr.write(`unknown workload: ${workload}\n`)
  process.exit(2)
}

for (let i = 0; i < warmup; i++) { run() }

const ms = []
for (let i = 0; i < iterations; i++) {
  const t = process.hrtime.bigint()
  run()
  ms.push(Number(process.hrtime.bigint() - t) / 1e6)
}

process.stdout.write(`${JSON.stringify({
  impl: 'node', workload, iterations, warmup, ms, sink,
})}\n`)
