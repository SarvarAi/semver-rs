#!/usr/bin/env node
// Dump upstream's regex token table (both the raw `src` and the ReDoS-hardened
// `safeSrc`) to JSON.
//
// src/re.rs rebuilds this table from scratch in Rust, and tests/port/re_table.rs
// asserts the Rust table is byte-identical to this dump. That turns "the regexes
// were ported faithfully" from a claim into a mechanical check.

import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
import fs from 'node:fs'

const require = createRequire(import.meta.url)
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const re = require(path.join(ROOT, 'vendor', 'node-semver', 'internal', 're.js'))

// t maps NAME -> index; invert it to get index -> NAME in declaration order.
const names = []
for (const [name, idx] of Object.entries(re.t)) { names[idx] = name }

const tokens = names.map((name, i) => ({
  index: i,
  name,
  src: re.src[i],
  safeSrc: re.safeSrc[i],
  global: re.re[i].flags.includes('g'),
}))

fs.writeFileSync(
  path.join(ROOT, 'tests', 'original', 're-table.json'),
  `${JSON.stringify({
    generatedFrom: 'npm/node-semver @ 6e05b7637396ac66522cff8731f07cfe0ef49a29 (v7.8.5)',
    note: 'Upstream internal/re.js token table. src = as written; safeSrc = after makeSafeRegex. The library matches with safeSrc (DECISIONS.md D5).',
    replacements: {
      tildeTrimReplace: re.tildeTrimReplace,
      caretTrimReplace: re.caretTrimReplace,
      comparatorTrimReplace: re.comparatorTrimReplace,
    },
    count: tokens.length,
    tokens,
  }, null, 2)}\n`
)

process.stdout.write(`dumped ${tokens.length} tokens\n`)
for (const tk of tokens) {
  if (tk.src !== tk.safeSrc) {
    process.stdout.write(`  ${tk.global ? 'g' : ' '} ${tk.name}  (safe form differs)\n`)
  }
}
