#!/usr/bin/env node
// Differential fuzzer: node-semver vs the Rust port.
//
//   node tools/fuzz.mjs [--seconds 60] [--seed 1] [--batch 40000] [--log fuzz/log.txt]
//
// Each round generates a fresh batch of calls, runs them through BOTH
// implementations, and compares every answer. Any divergence is written to the
// log in full, with the exact call needed to reproduce it.
//
// Generation is seeded and deterministic, so a divergence found here can be
// replayed exactly by re-running with the same --seed.
//
// The generator deliberately over-weights the places where a JS-to-Rust port is
// most likely to be wrong rather than sampling uniformly:
//   - integers straddling 2^53, where JS loses precision
//   - the whole ECMAScript whitespace set, including U+FEFF
//   - strings at and around MAX_LENGTH (256) and the safeRe bounds
//   - prerelease/build metadata with leading zeroes and mixed identifiers
//   - character-level mutations of real corpus strings

import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
import fs from 'node:fs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const arg = (name, dflt) => {
  const i = process.argv.indexOf(`--${name}`)
  return i === -1 ? dflt : process.argv[i + 1]
}

const SECONDS = Number(arg('seconds', 60))
const BATCH = Number(arg('batch', 40000))
const LOG = path.resolve(ROOT, arg('log', 'fuzz/log.txt'))
const PROBE = path.join(ROOT, 'target', 'release', 'semver-probe')
let seed = Number(arg('seed', 1))

if (!fs.existsSync(PROBE)) {
  process.stderr.write(`missing ${PROBE}\nRun: cargo build --release --features harness\n`)
  process.exit(2)
}

// xorshift32 — small, deterministic, and reproducible from the seed alone.
let state = seed >>> 0 || 1
const rnd = () => {
  state ^= state << 13; state >>>= 0
  state ^= state >>> 17
  state ^= state << 5; state >>>= 0
  return state / 0x100000000
}
// JSON.stringify leaves U+2028/U+2029 raw, but Node's readline treats them as
// line terminators — so an unescaped one splits a JSONL record in half and the
// call is silently lost. Escape them on the wire; the value is unchanged.
const enc = (o) => JSON.stringify(o)
  .replace(/\u2028/g, '\\u2028')
  .replace(/\u2029/g, '\\u2029')

const pick = (a) => a[Math.floor(rnd() * a.length)]
const int = (n) => Math.floor(rnd() * n)

const corpus = JSON.parse(
  fs.readFileSync(path.join(ROOT, 'tests', 'original', 'corpus-b-strings.json'), 'utf8')
).entries.map((e) => e.s)

// ECMAScript whitespace, including U+FEFF which Unicode White_Space excludes.
const WS = ['\t', '\n', '\v', '\f', '\r', ' ', ' ', ' ', ' ', ' ',
  ' ', ' ', ' ', '　', '﻿']
// Numbers chosen to straddle the f64 integer boundary.
const NUMS = ['0', '1', '7', '00', '007', '9', '10', '99999',
  '9007199254740990', '9007199254740991', '9007199254740992', '9007199254740993',
  '18014398509481984', '99999999999999999999', String(2 ** 53), String(2 ** 53 + 2)]
const IDS = ['alpha', 'beta', 'rc', 'x', 'X', '0', '00', '0a', 'a0', '-', 'a-b', 'pre', 'dev']
const OPS = ['', '=', '>', '>=', '<', '<=', '~', '^', '~>']
const XS = ['x', 'X', '*', '']

const genVersionish = () => {
  const r = rnd()
  if (r < 0.15) { return pick(corpus) }
  let v = `${pick(NUMS)}.${pick(NUMS)}.${pick(NUMS)}`
  if (rnd() < 0.45) { v += `-${pick(IDS)}${rnd() < 0.5 ? `.${pick(NUMS)}` : ''}` }
  if (rnd() < 0.25) { v += `+${pick(IDS)}${rnd() < 0.4 ? `.${pick(NUMS)}` : ''}` }
  if (rnd() < 0.20) { v = `v${v}` }
  if (rnd() < 0.10) { v = `${pick(WS)}${v}${pick(WS)}` }
  return v
}

const genRangeish = () => {
  const r = rnd()
  if (r < 0.15) { return pick(corpus) }
  const simple = () => {
    const t = rnd()
    if (t < 0.20) { return `${pick(NUMS)}.${pick(XS)}.${pick(XS)}` }
    if (t < 0.35) { return `${pick(OPS)}${genVersionish()}` }
    if (t < 0.45) { return `${genVersionish()} - ${genVersionish()}` }
    if (t < 0.55) { return `${pick(OPS)} ${genVersionish()}` }
    return `${pick(OPS)}${pick(NUMS)}.${pick(NUMS)}.${pick(NUMS)}`
  }
  let out = simple()
  const n = int(3)
  for (let i = 0; i < n; i++) { out += `${rnd() < 0.5 ? ' ' : ' || '}${simple()}` }
  return out
}

// Character-level mutation of an existing string.
const mutate = (s) => {
  if (!s.length) { return pick(WS) }
  const i = int(s.length)
  const t = int(6)
  const c = pick(['.', '-', '+', '*', 'x', 'v', '=', '>', '<', '~', '^', '0', '9', ' ', pick(WS)])
  if (t === 0) { return s.slice(0, i) + c + s.slice(i) }
  if (t === 1) { return s.slice(0, i) + s.slice(i + 1) }
  if (t === 2) { return s.slice(0, i) + c + s.slice(i + 1) }
  if (t === 3) { return s.slice(0, i) + s.slice(i, i + 1 + int(4)) + s.slice(i) }
  if (t === 4) { return s.repeat(1 + int(3)) }
  return s.slice(0, i) + s.slice(i).toUpperCase()
}

const OPTS = [
  null,
  { loose: true, includePrerelease: false },
  { loose: false, includePrerelease: true },
  { loose: true, includePrerelease: true },
]
const COERCE_OPTS = [
  { rtl: false, includePrerelease: false }, { rtl: true, includePrerelease: false },
  { rtl: false, includePrerelease: true }, { rtl: true, includePrerelease: true },
]
const RELEASES = ['major', 'minor', 'patch', 'premajor', 'preminor', 'prepatch',
  'prerelease', 'release', 'bogus']

const UNARY = ['parse', 'valid', 'clean', 'prerelease', 'major', 'minor', 'patch']
const RANGE_UNARY = ['validRange', 'rangeToString', 'rangeSet', 'toComparators', 'minVersion']
const PAIR = ['compare', 'rcompare', 'compareBuild', 'gt', 'lt', 'eq', 'neq', 'gte', 'lte']
const VR = ['satisfies', 'gtr', 'ltr']
const RR = ['intersects', 'subset']

let id = 0
function genCall () {
  const o = pick(OPTS)
  const t = rnd()
  const v = rnd() < 0.3 ? mutate(genVersionish()) : genVersionish()
  const r = rnd() < 0.3 ? mutate(genRangeish()) : genRangeish()

  if (t < 0.16) { return { id: id++, fn: pick(UNARY), a: [v, o] } }
  if (t < 0.30) { return { id: id++, fn: pick(RANGE_UNARY), a: [r, o] } }
  if (t < 0.46) { return { id: id++, fn: pick(PAIR), a: [v, genVersionish(), o] } }
  if (t < 0.62) { return { id: id++, fn: pick(VR), a: [v, r, o] } }
  if (t < 0.70) { return { id: id++, fn: pick(RR), a: [r, genRangeish(), o] } }
  if (t < 0.76) { return { id: id++, fn: 'coerce', a: [rnd() < 0.5 ? v : pick(corpus), pick(COERCE_OPTS)] } }
  if (t < 0.82) {
    return {
      id: id++, fn: 'inc',
      a: [v, pick(RELEASES), o, rnd() < 0.5 ? pick(IDS) : null,
        pick([null, false, '0', '1', 'false'])],
    }
  }
  if (t < 0.86) { return { id: id++, fn: 'truncate', a: [v, pick(RELEASES), o] } }
  if (t < 0.90) { return { id: id++, fn: 'diff', a: [v, genVersionish()] } }
  if (t < 0.93) { return { id: id++, fn: 'cmp', a: [v, pick(['', '=', '==', '!=', '>', '>=', '<', '<=', '===', '!==', '?']), genVersionish(), o] } }
  if (t < 0.96) {
    const list = Array.from({ length: 1 + int(5) }, () => genVersionish())
    return { id: id++, fn: pick(['sort', 'rsort']), a: [list, o] }
  }
  if (t < 0.98) {
    const list = Array.from({ length: 1 + int(5) }, () => genVersionish())
    return { id: id++, fn: pick(['minSatisfying', 'maxSatisfying', 'simplifyRange']), a: [list, r, o] }
  }
  return { id: id++, fn: 'outside', a: [v, r, pick(['>', '<', '?']), o] }
}

function runProc (cmd, args, input) {
  return new Promise((resolve, reject) => {
    const p = spawn(cmd, args, { stdio: ['pipe', 'pipe', 'pipe'] })
    // Collect Buffers and concat once. `out += chunk` would decode each chunk
    // independently, so a multi-byte UTF-8 character landing on a chunk
    // boundary would come back as replacement characters and look like a
    // divergence — which is exactly what it did before this was fixed.
    const outChunks = []; const errChunks = []
    p.stdout.on('data', (d) => outChunks.push(d))
    p.stderr.on('data', (d) => errChunks.push(d))
    p.on('error', reject)
    p.on('close', (code) => resolve({
      out: Buffer.concat(outChunks).toString('utf8'),
      err: Buffer.concat(errChunks).toString('utf8'),
      code,
    }))
    p.stdin.on('error', () => {})
    p.stdin.end(input)
  })
}

const parseLines = (s) => {
  const m = new Map()
  for (const line of s.split('\n')) {
    if (!line.trim()) { continue }
    try { const o = JSON.parse(line); m.set(o.id, o) } catch { /* ignore */ }
  }
  return m
}

function eq (a, b) {
  if (a === b) { return true }
  if (typeof a !== typeof b) { return false }
  if (a === null || b === null) { return a === b }
  if (Array.isArray(a) !== Array.isArray(b)) { return false }
  if (Array.isArray(a)) { return a.length === b.length && a.every((x, i) => eq(x, b[i])) }
  if (typeof a === 'object') {
    const ka = Object.keys(a).sort(); const kb = Object.keys(b).sort()
    return ka.length === kb.length && ka.every((k, i) => k === kb[i]) && ka.every((k) => eq(a[k], b[k]))
  }
  return false
}

fs.mkdirSync(path.dirname(LOG), { recursive: true })
const log = fs.createWriteStream(LOG, { flags: 'w' })
const w = (s) => { log.write(`${s}\n`); }

const started = new Date()
w('='.repeat(78))
w('differential fuzz: npm/node-semver v7.8.5 (6e05b763) vs semver-rs port')
w(`started      ${started.toISOString()}`)
w(`seed         ${seed}`)
w(`batch size   ${BATCH}`)
w(`target time  ${SECONDS}s`)
w(`node         ${process.version}`)
w(`probe        ${PROBE}`)
w('='.repeat(78))

const t0 = Date.now()
let round = 0
let totalCalls = 0
let divergences = 0
let agreements = 0

while ((Date.now() - t0) / 1000 < SECONDS) {
  round += 1
  const calls = []
  for (let i = 0; i < BATCH; i++) { calls.push(genCall()) }
  const input = `${calls.map((c) => enc(c)).join('\n')}\n`

  const [nodeRes, rustRes] = await Promise.all([
    runProc(process.execPath, [path.join(ROOT, 'tools', 'node-oracle.mjs')], input),
    runProc(PROBE, [], input),
  ])

  const n = parseLines(nodeRes.out)
  const r = parseLines(rustRes.out)

  let roundDiv = 0
  for (const call of calls) {
    const a = n.get(call.id); const b = r.get(call.id)
    totalCalls += 1
    let same
    if (!a || !b) {
      same = false
    } else if (a.ok && b.ok) {
      same = eq(a.v, b.v)
    } else if (!a.ok && !b.ok) {
      same = a.e === b.e
    } else {
      same = false
    }
    if (same) { agreements += 1 } else {
      divergences += 1; roundDiv += 1
      w('')
      w(`DIVERGENCE #${divergences}  round ${round}  ${new Date().toISOString()}`)
      w(`  call : ${enc({ fn: call.fn, a: call.a })}`)
      w(`  node : ${a ? JSON.stringify(a.ok ? { ok: a.v } : { threw: a.e }) : '(no result)'}`)
      w(`  rust : ${b ? JSON.stringify(b.ok ? { ok: b.v } : { threw: b.e }) : '(no result)'}`)
      w(`  repro: node tools/fuzz.mjs --seed ${seed} --seconds 5`)
    }
  }

  const elapsed = ((Date.now() - t0) / 1000).toFixed(1)
  const line = `round ${String(round).padStart(3)}  t=${elapsed.padStart(6)}s  calls=${String(totalCalls).padStart(9)}  divergences=${divergences}`
  w(line)
  process.stderr.write(`${line}\r`)
  if (rustRes.code !== 0) { w(`  !! probe exited ${rustRes.code}: ${rustRes.err.slice(0, 500)}`) }
  if (rustRes.err.includes('panicked')) { w(`  !! probe stderr: ${rustRes.err.slice(0, 2000)}`) }
}

const finished = new Date()
const wall = ((Date.now() - t0) / 1000).toFixed(1)
w('')
w('='.repeat(78))
w(`finished     ${finished.toISOString()}`)
w(`wall clock   ${wall}s`)
w(`rounds       ${round}`)
w(`total calls  ${totalCalls}`)
w(`agreements   ${agreements}`)
w(`divergences  ${divergences}`)
w(`result       ${divergences === 0 ? 'CLEAN — no divergence found' : 'DIVERGENCES FOUND (see above)'}`)
w('='.repeat(78))
// Wait for the log stream to flush before exiting: `process.exit()` on a
// pending write truncates the file, which silently ate the summary block on an
// earlier run.
await new Promise((resolve) => log.end(resolve))

process.stderr.write('\n')
process.stdout.write(`fuzz complete: ${totalCalls} calls in ${wall}s, ${divergences} divergences\n`)
process.stdout.write(`log: ${path.relative(ROOT, LOG)}\n`)
process.exitCode = divergences === 0 ? 0 : 1
