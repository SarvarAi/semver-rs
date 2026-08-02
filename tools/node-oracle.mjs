#!/usr/bin/env node
// Node side of the two-sided differential oracle.
//
// Protocol (JSONL, one call per line, in on stdin, out on stdout):
//   in : {"id":1,"fn":"satisfies","a":["1.2.3","^1.0.0",null]}
//   out: {"id":1,"ok":true,"v":true}
//        {"id":1,"ok":false,"e":"Invalid Version: x"}
//
// `src/bin/semver-probe.rs` speaks exactly this protocol against the Rust port.
// tools/diff-results.mjs compares the two streams.
//
// This file runs the ORIGINAL implementation and is a dev-time tool only. It is
// not part of the shipped port, which never links or invokes Node.

import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
import readline from 'node:readline'

const require = createRequire(import.meta.url)
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const semver = require(path.join(ROOT, 'vendor', 'node-semver', 'index.js'))
const Range = require(path.join(ROOT, 'vendor', 'node-semver', 'classes', 'range.js'))
const Comparator = require(path.join(ROOT, 'vendor', 'node-semver', 'classes', 'comparator.js'))
const { compareIdentifiers } = require(path.join(ROOT, 'vendor', 'node-semver', 'internal', 'identifiers.js'))

// JSON.stringify leaves U+2028/U+2029 raw, but Node's readline treats them as
// line terminators — so an unescaped one splits a JSONL record in half and the
// call is silently lost. Escape them on the wire; the value is unchanged.
const enc = (o) => JSON.stringify(o)
  .replace(/\u2028/g, '\\u2028')
  .replace(/\u2029/g, '\\u2029')

// `null` on the wire means "argument absent" (JS undefined).
const u = (x) => (x === null ? undefined : x)

// Describe a parsed version richly so divergence in any field is visible,
// not just in the canonical string.
const describe = (sv) =>
  sv === null || sv === undefined
    ? null
    : {
        version: sv.version,
        major: sv.major,
        minor: sv.minor,
        patch: sv.patch,
        prerelease: sv.prerelease,
        build: sv.build,
        raw: sv.raw,
      }

const FNS = {
  parse: ([v, o]) => describe(semver.parse(v, u(o))),
  valid: ([v, o]) => semver.valid(v, u(o)),
  clean: ([v, o]) => semver.clean(v, u(o)),
  satisfies: ([v, r, o]) => semver.satisfies(v, r, u(o)),
  validRange: ([r, o]) => semver.validRange(r, u(o)),

  compare: ([a, b, o]) => semver.compare(a, b, u(o)),
  rcompare: ([a, b, o]) => semver.rcompare(a, b, u(o)),
  compareLoose: ([a, b]) => semver.compareLoose(a, b),
  compareBuild: ([a, b, o]) => semver.compareBuild(a, b, u(o)),
  gt: ([a, b, o]) => semver.gt(a, b, u(o)),
  lt: ([a, b, o]) => semver.lt(a, b, u(o)),
  eq: ([a, b, o]) => semver.eq(a, b, u(o)),
  neq: ([a, b, o]) => semver.neq(a, b, u(o)),
  gte: ([a, b, o]) => semver.gte(a, b, u(o)),
  lte: ([a, b, o]) => semver.lte(a, b, u(o)),
  cmp: ([a, op, b, o]) => semver.cmp(a, op, b, u(o)),

  major: ([v, o]) => semver.major(v, u(o)),
  minor: ([v, o]) => semver.minor(v, u(o)),
  patch: ([v, o]) => semver.patch(v, u(o)),
  prerelease: ([v, o]) => semver.prerelease(v, u(o)),

  inc: ([v, rel, o, id, idBase]) => semver.inc(v, rel, u(o), u(id), u(idBase)),
  diff: ([a, b]) => semver.diff(a, b),
  truncate: ([v, t, o]) => semver.truncate(v, t, u(o)),
  coerce: ([v, o]) => describe(semver.coerce(v, u(o))),

  sort: ([list, o]) => semver.sort(list.slice(), u(o)),
  rsort: ([list, o]) => semver.rsort(list.slice(), u(o)),

  minVersion: ([r, o]) => describe(semver.minVersion(r, u(o))),
  minSatisfying: ([list, r, o]) => semver.minSatisfying(list, r, u(o)),
  maxSatisfying: ([list, r, o]) => semver.maxSatisfying(list, r, u(o)),
  gtr: ([v, r, o]) => semver.gtr(v, r, u(o)),
  ltr: ([v, r, o]) => semver.ltr(v, r, u(o)),
  outside: ([v, r, hilo, o]) => semver.outside(v, r, hilo, u(o)),
  toComparators: ([r, o]) => semver.toComparators(r, u(o)),
  intersects: ([a, b, o]) => semver.intersects(a, b, u(o)),
  simplifyRange: ([list, r, o]) => semver.simplifyRange(list, r, u(o)),
  subset: ([sub, dom, o]) => semver.subset(sub, dom, u(o)),

  // Object surfaces that have no top-level function equivalent.
  rangeToString: ([r, o]) => new Range(r, u(o)).range,
  rangeSet: ([r, o]) => new Range(r, u(o)).set.map((s) => s.map((c) => c.value)),
  comparatorValue: ([c, o]) => new Comparator(c, u(o)).value,
  comparatorIntersects: ([a, b, o]) =>
    new Comparator(a, u(o)).intersects(new Comparator(b, u(o)), u(o)),
  compareIdentifiers: ([a, b]) => compareIdentifiers(a, b),
}

const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity })
const out = []

for await (const line of rl) {
  if (!line.trim()) { continue }
  let call
  try {
    call = JSON.parse(line)
  } catch (e) {
    out.push(enc({ id: null, ok: false, e: `bad input line: ${e.message}` }))
    continue
  }
  const fn = FNS[call.fn]
  if (!fn) {
    out.push(enc({ id: call.id, ok: false, e: `unknown fn: ${call.fn}` }))
    continue
  }
  try {
    const v = fn(call.a)
    out.push(enc({ id: call.id, ok: true, v: v === undefined ? null : v }))
  } catch (e) {
    out.push(enc({ id: call.id, ok: false, e: String(e && e.message ? e.message : e) }))
  }
  if (out.length >= 4096) {
    process.stdout.write(`${out.join('\n')}\n`)
    out.length = 0
  }
}

if (out.length) { process.stdout.write(`${out.join('\n')}\n`) }
