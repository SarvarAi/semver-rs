#!/usr/bin/env node
// Compare the Node oracle's results against the Rust port's, call for call.
//
//   node tools/diff-results.mjs <calls.jsonl> <node.jsonl> <rust.jsonl> [--max N]
//
// Divergences are classified, because they are not all equally interesting:
//
//   value        both succeeded, different answers      -> a real bug
//   throw-vs-ok  one threw and the other did not        -> a real bug
//   message      both threw, different wording          -> cosmetic, tracked
//   unimplemented the port has not ported that fn yet   -> a known gap
//   panic        the port panicked                      -> a serious bug
//
// Exit code is non-zero only for the categories that represent real defects.

import fs from 'node:fs'
import readline from 'node:readline'

const [callsPath, nodePath, rustPath] = process.argv.slice(2)
const maxShow = (() => {
  const i = process.argv.indexOf('--max')
  return i === -1 ? 25 : Number(process.argv[i + 1])
})()

if (!callsPath || !nodePath || !rustPath) {
  process.stderr.write('usage: diff-results.mjs <calls> <node> <rust> [--max N]\n')
  process.exit(2)
}

async function loadById (p) {
  const map = new Map()
  const rl = readline.createInterface({
    input: fs.createReadStream(p), crlfDelay: Infinity,
  })
  for await (const line of rl) {
    if (!line.trim()) { continue }
    const o = JSON.parse(line)
    map.set(o.id, o)
  }
  return map
}

// Structural equality with JS/Rust JSON quirks accounted for: -0 vs 0, and
// key order in objects.
function eq (a, b) {
  if (a === b) { return true }
  if (typeof a !== typeof b) { return false }
  if (a === null || b === null) { return a === b }
  if (typeof a === 'number' && typeof b === 'number') {
    return a === b || (Number.isNaN(a) && Number.isNaN(b))
  }
  if (Array.isArray(a) !== Array.isArray(b)) { return false }
  if (Array.isArray(a)) {
    return a.length === b.length && a.every((x, i) => eq(x, b[i]))
  }
  if (typeof a === 'object') {
    const ka = Object.keys(a).sort()
    const kb = Object.keys(b).sort()
    if (ka.length !== kb.length || !ka.every((k, i) => k === kb[i])) { return false }
    return ka.every((k) => eq(a[k], b[k]))
  }
  return false
}

const calls = await loadById(callsPath)
const nodeRes = await loadById(nodePath)
const rustRes = await loadById(rustPath)

const buckets = {
  value: [], 'throw-vs-ok': [], message: [], unimplemented: [], panic: [], missing: [],
}
const perFixture = new Map()
const bump = (m, key) => {
  if (!perFixture.has(m)) {
    perFixture.set(m, { total: 0, agree: 0, value: 0, throwVsOk: 0, message: 0, unimplemented: 0, panic: 0 })
  }
  const e = perFixture.get(m)
  e[key] += 1
  return e
}

for (const [id, call] of calls) {
  const m = call.m ?? '(unlabelled)'
  bump(m, 'total')
  const n = nodeRes.get(id)
  const r = rustRes.get(id)

  if (!n || !r) {
    buckets.missing.push({ call, n, r })
    continue
  }

  if (!r.ok && r.e === '__UNIMPLEMENTED__') {
    buckets.unimplemented.push({ call })
    bump(m, 'unimplemented')
    continue
  }
  if (!r.ok && r.e === '__PANIC__') {
    buckets.panic.push({ call, n, r })
    bump(m, 'panic')
    continue
  }

  if (n.ok && r.ok) {
    if (eq(n.v, r.v)) { bump(m, 'agree') } else {
      buckets.value.push({ call, node: n.v, rust: r.v })
      bump(m, 'value')
    }
  } else if (!n.ok && !r.ok) {
    if (n.e === r.e) { bump(m, 'agree') } else {
      buckets.message.push({ call, node: n.e, rust: r.e })
      bump(m, 'message')
    }
  } else {
    buckets['throw-vs-ok'].push({
      call,
      node: n.ok ? { ok: n.v } : { threw: n.e },
      rust: r.ok ? { ok: r.v } : { threw: r.e },
    })
    bump(m, 'throwVsOk')
  }
}

const total = calls.size
const hard = buckets.value.length + buckets['throw-vs-ok'].length +
  buckets.panic.length + buckets.missing.length
const soft = buckets.message.length
const known = buckets.unimplemented.length
const agree = total - hard - soft - known

process.stdout.write(`\n=== differential comparison: ${total} calls ===\n`)
process.stdout.write(`  agree            ${String(agree).padStart(8)}  (${((agree / total) * 100).toFixed(4)}%)\n`)
process.stdout.write(`  value diverge    ${String(buckets.value.length).padStart(8)}\n`)
process.stdout.write(`  throw-vs-ok      ${String(buckets['throw-vs-ok'].length).padStart(8)}\n`)
process.stdout.write(`  panic (port)     ${String(buckets.panic.length).padStart(8)}\n`)
process.stdout.write(`  missing result   ${String(buckets.missing.length).padStart(8)}\n`)
process.stdout.write(`  error-message    ${String(soft).padStart(8)}  (cosmetic)\n`)
process.stdout.write(`  not yet ported   ${String(known).padStart(8)}\n`)

process.stdout.write('\n=== per source ===\n')
const rows = [...perFixture.entries()].sort((a, b) => a[0].localeCompare(b[0]))
for (const [name, e] of rows) {
  const bad = e.value + e.throwVsOk + e.panic
  const flag = bad === 0 ? 'ok  ' : 'FAIL'
  process.stdout.write(
    `  ${flag} ${name.padEnd(26)} ${String(e.agree).padStart(8)}/${String(e.total).padEnd(8)}` +
    `${bad ? ` value=${e.value} throwVsOk=${e.throwVsOk} panic=${e.panic}` : ''}` +
    `${e.message ? ` msg=${e.message}` : ''}${e.unimplemented ? ` unported=${e.unimplemented}` : ''}\n`
  )
}

for (const [name, list] of Object.entries(buckets)) {
  if (!list.length || name === 'unimplemented') { continue }
  process.stdout.write(`\n=== ${name} (${list.length}, showing up to ${maxShow}) ===\n`)
  for (const d of list.slice(0, maxShow)) {
    process.stdout.write(`  ${d.call.fn}(${JSON.stringify(d.call.a)})  [${d.call.m}]\n`)
    process.stdout.write(`     node: ${JSON.stringify(d.node ?? d.n)}\n`)
    process.stdout.write(`     rust: ${JSON.stringify(d.rust ?? d.r)}\n`)
  }
}

process.exit(hard > 0 ? 1 : 0)
