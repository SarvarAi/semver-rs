#!/usr/bin/env node
// Compare the ported CLI against upstream's bin/semver.js across a matrix of
// invocations, checking all three observable channels: stdout, stderr and exit
// code. Track F scores CLI fidelity explicitly, so nothing is assumed.
//
//   node tools/cli-parity.mjs [path-to-rust-binary]

import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
import fs from 'node:fs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const RUST = process.argv[2] ?? path.join(ROOT, 'target', 'release', 'semver')
const NODE_CLI = path.join(ROOT, 'vendor', 'node-semver', 'bin', 'semver.js')

if (!fs.existsSync(RUST)) {
  process.stderr.write(`missing ${RUST}\nRun: cargo build --release\n`)
  process.exit(2)
}

const VERSIONS = ['1.2.3', '0.0.1', '2.0.0-alpha.1', 'v1.2.3', '1.2.3+build.5', 'nonsense', '1.2.3tag', '=1.2.3']
const RANGES = ['^1.0.0', '~1.2.0', '1.2.x', '>=1.0.0 <2.0.0', '1.0.0 - 2.0.0', '*', '', '^1.2.3 || ~2.0.0', 'garbage']
const LEVELS = ['major', 'minor', 'patch', 'premajor', 'preminor', 'prepatch', 'prerelease', 'release', 'bogus']

const cases = []
const add = (...args) => cases.push(args)

add()
add('--help')
add('-h')
add('-?')

for (const v of VERSIONS) {
  add(v)
  add('-v', v)
  add('-l', v)
  add('-c', v)
  add('-c', '--rtl', v)
  add('-c', '--ltr', v)
  add('-p', v)
}

for (const v of VERSIONS) {
  for (const r of RANGES) {
    add('-r', r, v)
    add('--range', r, v, '-l')
    add('-p', '-r', r, v)
    add(`--range=${r}`, v)
  }
}

for (const lvl of LEVELS) {
  add('-i', lvl, '1.2.3')
  add('--inc', lvl, '2.0.0-alpha.1')
  add('-i', lvl, '1.2.3', '--preid', 'beta')
  add('-i', lvl, '1.2.3', '--preid', 'beta', '-n', 'false')
  add('-i', lvl, '1.2.3', '--preid', 'beta', '-n', '1')
  add('-i', lvl, '1.2.3', '-n', '0')
}

// Sorting, reversing, multi-version and the error paths.
add('1.2.3', '1.2.4', '0.0.1')
add('-rv', '1.2.3', '1.2.4', '0.0.1')
add('--reverse', '2.0.0', '1.0.0')
add('1.0.0', '2.0.0', '-r', '^1.0.0')
add('-i', 'patch', '1.2.3', '1.2.4')          // --inc with multiple versions
add('-i', 'patch', '1.2.3', '-r', '^1.0.0')   // --inc with a range
add('-i', '1.2.3', '1.2.3')                   // errant -i value warning path
add('-v')                                      // -v with nothing after it
add('nonsense')
add('-r', '^1.0.0', 'nonsense')

const run = (cmd, args) => {
  const r = spawnSync(cmd[0], [...cmd.slice(1), ...args], { encoding: 'utf8' })
  return { stdout: r.stdout ?? '', stderr: r.stderr ?? '', code: r.status }
}

let pass = 0
const fails = []
for (const args of cases) {
  const a = run([process.execPath, NODE_CLI], args)
  const b = run([RUST], args)
  if (a.stdout === b.stdout && a.stderr === b.stderr && a.code === b.code) {
    pass += 1
  } else {
    fails.push({ args, node: a, rust: b })
  }
}

process.stdout.write(`\n=== CLI parity: ${pass}/${cases.length} invocations identical ===\n`)
process.stdout.write('(comparing stdout, stderr and exit code)\n')
for (const fl of fails.slice(0, 20)) {
  process.stdout.write(`\nFAIL semver ${fl.args.map((x) => JSON.stringify(x)).join(' ')}\n`)
  for (const k of ['stdout', 'stderr', 'code']) {
    if (JSON.stringify(fl.node[k]) !== JSON.stringify(fl.rust[k])) {
      process.stdout.write(`  ${k}: node=${JSON.stringify(fl.node[k])} rust=${JSON.stringify(fl.rust[k])}\n`)
    }
  }
}
if (fails.length > 20) {
  process.stdout.write(`\n... and ${fails.length - 20} more\n`)
}
process.exit(fails.length ? 1 : 0)
