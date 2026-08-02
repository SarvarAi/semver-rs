#!/usr/bin/env node
// Build Corpus B: the string alphabet used to drive differential testing.
//
// The 15 fixture files are only part of upstream's coverage — most assertions
// live inline in the ~66 test/**/*.js files (test/functions/coerce.js alone is
// 6.5 KB of them). This harvests every string literal appearing anywhere in the
// upstream test tree, classifies each with the real implementation, and emits a
// flat alphabet.
//
// That alphabet is used twice: as the input space for the two-sided oracle, and
// as the seed corpus the fuzzer mutates.
//
// READ-ONLY with respect to vendor/.

import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
import fs from 'node:fs'

const require = createRequire(import.meta.url)
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const VENDOR = path.join(ROOT, 'vendor', 'node-semver')
const semver = require(path.join(VENDOR, 'index.js'))

function walk (dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    if (e.name === 'node_modules') { continue }
    const p = path.join(dir, e.name)
    if (e.isDirectory()) { walk(p, out) } else if (e.name.endsWith('.js')) { out.push(p) }
  }
  return out
}

// Single-quoted, double-quoted and simple template literals, with escapes.
const STRING_RE = /'((?:[^'\\\n]|\\.)*)'|"((?:[^"\\\n]|\\.)*)"|`((?:[^`\\$]|\\.)*)`/g

function unescape (s) {
  return s.replace(/\\(.)/g, (_, c) =>
    ({ n: '\n', t: '\t', r: '\r', 0: '\0', '\\': '\\', "'": "'", '"': '"', '`': '`' }[c] ?? c))
}

const files = walk(path.join(VENDOR, 'test')).sort()
const seen = new Set()

for (const f of files) {
  const text = fs.readFileSync(f, 'utf8')
  for (const m of text.matchAll(STRING_RE)) {
    const raw = m[1] ?? m[2] ?? m[3]
    if (raw === undefined) { continue }
    const s = unescape(raw)
    // Skip obvious non-inputs: require paths, tap messages, long prose.
    if (s.length > 300) { continue }
    if (/^[./]|^node:/.test(s) && s.includes('/')) { continue }
    seen.add(s)
  }
}

// Also fold in every string that appears anywhere in the extracted fixtures.
const fxDir = path.join(ROOT, 'tests', 'original', 'fixtures')
if (fs.existsSync(fxDir)) {
  const collect = (node) => {
    if (!node || typeof node !== 'object') { return }
    if (node.k === 'string') { seen.add(node.v); return }
    if (node.k === 'array') { node.v.forEach(collect); return }
    if (node.k === 'object') { Object.values(node.v).forEach(collect) }
  }
  for (const f of fs.readdirSync(fxDir)) {
    if (f === '_index.json') { continue }
    for (const row of JSON.parse(fs.readFileSync(path.join(fxDir, f), 'utf8')).rows) {
      row.values.forEach(collect)
    }
  }
}

const strings = [...seen].sort()

// Classify with the real implementation so the fuzzer can bias toward inputs
// that actually mean something, without ever excluding the ones that don't.
const classify = (s) => {
  const t = { strictVersion: false, looseVersion: false, validRange: false, coercible: false }
  try { t.strictVersion = semver.valid(s) !== null } catch {}
  try { t.looseVersion = semver.valid(s, true) !== null } catch {}
  try { t.validRange = semver.validRange(s) !== null } catch {}
  try { t.coercible = semver.coerce(s) !== null } catch {}
  return t
}

const entries = strings.map((s) => ({ s, ...classify(s) }))
const count = (k) => entries.filter((e) => e[k]).length

const payload = {
  generatedFrom: 'npm/node-semver @ 6e05b7637396ac66522cff8731f07cfe0ef49a29 (v7.8.5)',
  note: 'Corpus B. String literals harvested from all upstream test files plus the extracted fixtures. Classified with the upstream implementation; classification is metadata only and never filters the corpus.',
  sourceFiles: files.length,
  total: entries.length,
  summary: {
    strictVersion: count('strictVersion'),
    looseVersion: count('looseVersion'),
    validRange: count('validRange'),
    coercible: count('coercible'),
    unclassified: entries.filter((e) => !e.strictVersion && !e.looseVersion && !e.validRange && !e.coercible).length,
  },
  entries,
}

const outDir = path.join(ROOT, 'tests', 'original')
fs.mkdirSync(outDir, { recursive: true })
fs.writeFileSync(path.join(outDir, 'corpus-b-strings.json'), `${JSON.stringify(payload, null, 2)}\n`)

process.stdout.write(`harvested ${entries.length} unique strings from ${files.length} test files\n`)
for (const [k, v] of Object.entries(payload.summary)) {
  process.stdout.write(`  ${String(v).padStart(5)}  ${k}\n`)
}
