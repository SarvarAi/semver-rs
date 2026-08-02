#!/usr/bin/env node
// Generate the call list that both oracles execute.
//
//   node tools/gen-calls.mjs fixtures > fuzz/calls-fixtures.jsonl
//   node tools/gen-calls.mjs sweep    > fuzz/calls-sweep.jsonl
//
// Note what this deliberately does NOT do: it never encodes an *expected*
// answer. The fixtures are mined for interesting (version, range, options)
// triples, and both implementations are asked the same questions. Upstream is
// the oracle, so the port is graded against the real thing rather than against
// my reading of what each fixture meant.
//
// Every call carries an `m` field naming the fixture it came from, so
// divergences can be reported per fixture file. Both oracles ignore `m`.

import { fileURLToPath } from 'node:url'
import path from 'node:path'
import fs from 'node:fs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const FX = path.join(ROOT, 'tests', 'original', 'fixtures')
const mode = process.argv[2] ?? 'fixtures'

let id = 0
const out = []
// JSON.stringify leaves U+2028/U+2029 raw, but Node's readline treats them as
// line terminators — so an unescaped one splits a JSONL record in half and the
// call is silently lost. Escape them on the wire; the value is unchanged.
const enc = (o) => JSON.stringify(o)
  .replace(/\u2028/g, '\\u2028')
  .replace(/\u2029/g, '\\u2029')
const emit = (m, fn, a) => out.push(enc({ id: id++, m, fn, a }))

const OPTS = [
  null,
  { loose: true, includePrerelease: false },
  { loose: false, includePrerelease: true },
  { loose: true, includePrerelease: true },
]

// Decode the tagged encoding written by tools/extract-fixtures.mjs.
const dec = (n) => {
  if (!n || typeof n !== 'object') { return undefined }
  switch (n.k) {
    case 'string': return n.v
    case 'bool': return n.v
    case 'number': return n.v
    case 'null': return null
    case 'undefined': return undefined
    case 'array': return n.v.map(dec)
    default: return undefined
  }
}
const str = (n) => (typeof dec(n) === 'string' ? dec(n) : undefined)

const readFixture = (name) =>
  JSON.parse(fs.readFileSync(path.join(FX, `${name}.json`), 'utf8'))

// --- version-shaped calls -------------------------------------------------
const versionCalls = (m, v, o) => {
  if (v === undefined) { return }
  emit(m, 'parse', [v, o])
  emit(m, 'valid', [v, o])
  emit(m, 'clean', [v, o])
  emit(m, 'prerelease', [v, o])
  emit(m, 'major', [v, o])
  emit(m, 'minor', [v, o])
  emit(m, 'patch', [v, o])
  for (const t of ['major', 'minor', 'patch', 'prerelease', 'premajor']) {
    emit(m, 'truncate', [v, t, o])
  }
}

// --- range-shaped calls ---------------------------------------------------
const rangeCalls = (m, r, o) => {
  if (r === undefined) { return }
  emit(m, 'validRange', [r, o])
  emit(m, 'rangeToString', [r, o])
  emit(m, 'rangeSet', [r, o])
  emit(m, 'toComparators', [r, o])
  emit(m, 'minVersion', [r, o])
}

// --- pair calls -----------------------------------------------------------
const pairCalls = (m, a, b, o) => {
  if (a === undefined || b === undefined) { return }
  for (const fn of ['compare', 'rcompare', 'compareBuild', 'gt', 'lt', 'eq', 'neq', 'gte', 'lte']) {
    emit(m, fn, [a, b, o])
  }
  for (const op of ['', '=', '==', '!=', '>', '>=', '<', '<=', '===', '!==']) {
    emit(m, 'cmp', [a, op, b, o])
  }
  emit(m, 'diff', [a, b])
  emit(m, 'sort', [[a, b], o])
  emit(m, 'rsort', [[a, b], o])
}

const versionRangeCalls = (m, v, r, o) => {
  if (v === undefined || r === undefined) { return }
  emit(m, 'satisfies', [v, r, o])
  emit(m, 'gtr', [v, r, o])
  emit(m, 'ltr', [v, r, o])
  emit(m, 'outside', [v, r, '>', o])
  emit(m, 'outside', [v, r, '<', o])
  emit(m, 'minSatisfying', [[v], r, o])
  emit(m, 'maxSatisfying', [[v], r, o])
  emit(m, 'simplifyRange', [[v], r, o])
}

if (mode === 'fixtures') {
  // ---- pairs of versions
  for (const name of ['comparisons', 'equality']) {
    const fx = readFixture(name)
    for (const row of fx.rows) {
      const [a, b] = [str(row.values[0]), str(row.values[1])]
      const o = row.options ?? null
      pairCalls(name, a, b, o)
      versionCalls(name, a, o)
      versionCalls(name, b, o)
    }
  }

  // ---- range + version
  for (const name of [
    'range-include', 'range-exclude',
    'version-gt-range', 'version-lt-range',
    'version-not-gt-range', 'version-not-lt-range',
  ]) {
    const fx = readFixture(name)
    for (const row of fx.rows) {
      const [r, v] = [str(row.values[0]), str(row.values[1])]
      const o = row.options ?? null
      versionRangeCalls(name, v, r, o)
      rangeCalls(name, r, o)
      versionCalls(name, v, o)
    }
  }

  // ---- range canonicalisation
  {
    const fx = readFixture('range-parse')
    for (const row of fx.rows) {
      const r = str(row.values[0])
      rangeCalls('range-parse', r, row.options ?? null)
    }
  }

  // ---- valid / invalid versions, under every options combination
  for (const name of ['valid-versions', 'invalid-versions']) {
    const fx = readFixture(name)
    for (const row of fx.rows) {
      const v = str(row.values[0])
      for (const o of OPTS) { versionCalls(name, v, o) }
    }
  }

  // ---- increments
  {
    const fx = readFixture('increments')
    for (const row of fx.rows) {
      const v = str(row.values[0])
      const rel = str(row.values[1])
      const o = row.options ?? null
      const identifier = dec(row.values[4])
      const identifierBase = dec(row.values[5])
      if (v === undefined || rel === undefined) { continue }
      emit('increments', 'inc', [
        v, rel, o,
        identifier === undefined ? null : identifier,
        identifierBase === undefined ? null : identifierBase,
      ])
    }
  }

  // ---- truncations
  {
    const fx = readFixture('truncations')
    for (const row of fx.rows) {
      const v = str(row.values[0])
      const t = str(row.values[1])
      if (v !== undefined && t !== undefined) { emit('truncations', 'truncate', [v, t, null]) }
    }
  }

  // ---- intersections
  {
    const fx = readFixture('range-intersection')
    for (const row of fx.rows) {
      const [a, b] = [str(row.values[0]), str(row.values[1])]
      if (a === undefined || b === undefined) { continue }
      for (const o of [null, { loose: false, includePrerelease: true }]) {
        emit('range-intersection', 'intersects', [a, b, o])
        emit('range-intersection', 'intersects', [b, a, o])
        emit('range-intersection', 'subset', [a, b, o])
      }
    }
  }
  {
    const fx = readFixture('comparator-intersection')
    for (const row of fx.rows) {
      const [a, b] = [str(row.values[0]), str(row.values[1])]
      const ip = dec(row.values[3])
      if (a === undefined || b === undefined) { continue }
      const o = ip === true ? { loose: false, includePrerelease: true } : null
      emit('comparator-intersection', 'comparatorIntersects', [a, b, o])
      emit('comparator-intersection', 'comparatorIntersects', [b, a, o])
      emit('comparator-intersection', 'comparatorValue', [a, o])
      emit('comparator-intersection', 'comparatorValue', [b, o])
    }
  }
} else if (mode === 'sweep') {
  // Broad sweep over Corpus B: every harvested string, every options combo.
  const corpus = JSON.parse(
    fs.readFileSync(path.join(ROOT, 'tests', 'original', 'corpus-b-strings.json'), 'utf8')
  )
  const all = corpus.entries.map((e) => e.s)
  const versions = corpus.entries.filter((e) => e.looseVersion || e.strictVersion).map((e) => e.s)
  const ranges = corpus.entries.filter((e) => e.validRange).map((e) => e.s)

  for (const s of all) {
    for (const o of OPTS) {
      versionCalls('sweep-unary', s, o)
      rangeCalls('sweep-unary', s, o)
    }
    for (const co of [
      { rtl: false, includePrerelease: false },
      { rtl: true, includePrerelease: false },
      { rtl: false, includePrerelease: true },
      { rtl: true, includePrerelease: true },
    ]) {
      emit('sweep-coerce', 'coerce', [s, co])
    }
  }

  // Deterministic pairing: every version against a rotating window of ranges,
  // so coverage is broad without going quadratic.
  const WINDOW = 24
  for (let i = 0; i < versions.length; i++) {
    for (let k = 0; k < WINDOW; k++) {
      const r = ranges[(i * 7 + k * 13) % ranges.length]
      versionRangeCalls('sweep-pair', versions[i], r, OPTS[(i + k) % OPTS.length])
    }
  }
  for (let i = 0; i < versions.length; i++) {
    for (let k = 0; k < 8; k++) {
      const b = versions[(i * 11 + k * 17) % versions.length]
      pairCalls('sweep-cmp', versions[i], b, OPTS[(i + k) % OPTS.length])
    }
  }
  for (let i = 0; i < ranges.length; i++) {
    for (let k = 0; k < 6; k++) {
      const b = ranges[(i * 5 + k * 19) % ranges.length]
      emit('sweep-intersect', 'intersects', [ranges[i], b, OPTS[(i + k) % OPTS.length]])
      emit('sweep-intersect', 'subset', [ranges[i], b, OPTS[(i + k) % OPTS.length]])
    }
  }
} else {
  process.stderr.write(`unknown mode: ${mode}\n`)
  process.exit(2)
}

process.stdout.write(`${out.join('\n')}\n`)
process.stderr.write(`generated ${out.length} calls (mode=${mode})\n`)
