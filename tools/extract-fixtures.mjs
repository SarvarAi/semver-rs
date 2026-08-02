#!/usr/bin/env node
// Extract node-semver's test fixtures into language-neutral JSON.
//
// READ-ONLY with respect to vendor/: this script `require()`s the upstream
// fixture modules and writes only into tests/original/.
//
// Why evaluate them under Node instead of parsing the JS?  Because the fixtures
// are not JSON — they legitimately contain `undefined`, regex literals such as
// /asdf/, and options objects like { loose: 123 } and { loose: null }. Those
// values are meaningful (see DECISIONS.md D6), and the only faithful way to
// learn what upstream makes of them is to let upstream's own parseOptions
// decide.
//
// Each value is encoded as a tagged node so nothing is silently coerced. For
// the fixtures whose documented shape has an options slot, the resolved
// {loose, includePrerelease} pair is recorded alongside the original spelling,
// so the reduction to two booleans is auditable rather than assumed.

import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import { inspect } from 'node:util'
import { createHash } from 'node:crypto'
import path from 'node:path'
import fs from 'node:fs'

const require = createRequire(import.meta.url)
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const VENDOR = path.join(ROOT, 'vendor', 'node-semver')
const FIXTURES = path.join(VENDOR, 'test', 'fixtures')
const OUT = path.join(ROOT, 'tests', 'original', 'fixtures')

const parseOptions = require(path.join(VENDOR, 'internal', 'parse-options.js'))

// Documented tuple shapes, transcribed from the comment header of each fixture.
// `options` is the index of the slot that is fed to parseOptions, if any.
const SHAPES = {
  'comparator-intersection': { shape: '[c0, c1, expectedIntersection, includePrerelease]', options: null },
  'comparisons': { shape: '[v1, v2, options]  // v1 > v2', options: 2 },
  'equality': { shape: '[v1, v2, options]  // v1 == v2', options: 2 },
  'increments': { shape: '[version, inc, result, options, identifier, identifierBase]', options: 3 },
  'invalid-versions': { shape: '[value, reason, options]', options: 2 },
  'range-exclude': { shape: '[range, version, options]  // NOT satisfied', options: 2 },
  'range-include': { shape: '[range, version, options]  // satisfied', options: 2 },
  'range-intersection': { shape: '[r0, r1, expectedIntersection]', options: null },
  'range-parse': { shape: '[range, canonical, options]  // null canonical = invalid', options: 2 },
  'truncations': { shape: '[version, releaseType, result]', options: null },
  'valid-versions': { shape: '[version, major, minor, patch, prerelease[], build[]]', options: null },
  'version-gt-range': { shape: '[range, version, options]  // version > range', options: 2 },
  'version-lt-range': { shape: '[range, version, options]  // version < range', options: 2 },
  'version-not-gt-range': { shape: '[range, version, options]  // NOT(version > range)', options: 2 },
  'version-not-lt-range': { shape: '[range, version, options]  // NOT(version < range)', options: 2 },
}

// Tagged encoding: never lose the distinction between undefined / null /
// "" / 0 / false, all of which appear in these fixtures and all of which mean
// different things to parseOptions.
function enc (v) {
  if (v === undefined) { return { k: 'undefined' } }
  if (v === null) { return { k: 'null' } }
  if (typeof v === 'string') { return { k: 'string', v } }
  if (typeof v === 'boolean') { return { k: 'bool', v } }
  if (typeof v === 'number') {
    return Number.isFinite(v) ? { k: 'number', v } : { k: 'number-special', v: String(v) }
  }
  if (Array.isArray(v)) { return { k: 'array', v: v.map(enc) } }
  if (v instanceof RegExp) { return { k: 'regexp', v: String(v) } }
  if (typeof v === 'object') {
    const o = {}
    for (const [key, val] of Object.entries(v)) { o[key] = enc(val) }
    return { k: 'object', v: o }
  }
  return { k: 'other', v: String(v) }
}

// Resolve an options slot exactly the way upstream does, then reduce to the two
// booleans that are its entire observable surface (DECISIONS.md D6).
function resolveOptions (raw) {
  const o = parseOptions(raw)
  return { loose: !!o.loose, includePrerelease: !!o.includePrerelease }
}

fs.mkdirSync(OUT, { recursive: true })

const index = []
let totalRows = 0

for (const name of Object.keys(SHAPES).sort()) {
  const file = path.join(FIXTURES, `${name}.js`)
  const rows = require(file)
  if (!Array.isArray(rows)) { throw new Error(`${name}: expected an array export`) }

  const { shape, options: optIdx } = SHAPES[name]
  const encoded = rows.map((row) => {
    const out = { values: row.map(enc) }
    if (optIdx !== null) {
      out.optionsJs = inspect(row[optIdx], { depth: 3, breakLength: Infinity })
      out.options = resolveOptions(row[optIdx])
    }
    return out
  })

  const payload = {
    source: `test/fixtures/${name}.js`,
    upstreamSha256: createHash('sha256').update(fs.readFileSync(file)).digest('hex'),
    shape,
    optionsIndex: optIdx,
    count: encoded.length,
    rows: encoded,
  }

  fs.writeFileSync(path.join(OUT, `${name}.json`), `${JSON.stringify(payload, null, 2)}\n`)
  index.push({ name, count: encoded.length, shape, optionsIndex: optIdx })
  totalRows += encoded.length
  process.stdout.write(`${String(encoded.length).padStart(5)}  ${name}\n`)
}

fs.writeFileSync(
  path.join(OUT, '_index.json'),
  `${JSON.stringify({
    generatedFrom: 'npm/node-semver @ 6e05b7637396ac66522cff8731f07cfe0ef49a29 (v7.8.5)',
    note: 'Generated by tools/extract-fixtures.mjs. Upstream files were read, never modified.',
    fixtures: index,
    totalRows,
  }, null, 2)}\n`
)

process.stdout.write(`${String(totalRows).padStart(5)}  TOTAL across ${index.length} fixtures\n`)
