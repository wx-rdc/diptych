#!/usr/bin/env node
// v2.2 T1：解析覆盖度门禁脚本。
// 用法：先在 UI/E2E 打开文档（触发解析与 raw 缓存登记），再跑本脚本。
//   node scripts/audit_coverage.mjs --doc <doc_id>
// violations 非空或 coverage 低于门禁 → exit 1（CI/E2E 断言用）。
import process from 'node:process';

const BASE = process.env.GEMINUS_BASE ?? 'http://127.0.0.1:39321';
const CHAR_GATE = 0.98;
const AREA_GATE = 0.95;

const docId = process.argv.includes('--doc')
  ? process.argv[process.argv.indexOf('--doc') + 1]
  : null;
if (!docId) {
  console.error('用法: node scripts/audit_coverage.mjs --doc <doc_id>');
  process.exit(2);
}

const res = await fetch(`${BASE}/api/debug/coverage?doc_id=${encodeURIComponent(docId)}`);
if (!res.ok) {
  console.error(`HTTP ${res.status}: ${await res.text()}`);
  process.exit(2);
}
const r = await res.json();

let fail = false;
for (const v of r.violations ?? []) {
  console.error(`❌ ${v.kind}: dropped=${v.dropped} sample_bbox=${JSON.stringify(v.sample_bbox)}`);
  fail = true;
}
if ((r.char_coverage ?? 1) < CHAR_GATE) {
  console.error(`❌ char_coverage=${r.char_coverage} < ${CHAR_GATE}`);
  fail = true;
}
if ((r.area_coverage ?? 1) < AREA_GATE) {
  console.error(`❌ area_coverage=${r.area_coverage} < ${AREA_GATE}`);
  fail = true;
}
console.log(`per_type: ${JSON.stringify(r.per_type)}`);
console.log(`char=${r.char_coverage} area=${r.area_coverage} violations=${(r.violations ?? []).length}`);
console.log(fail ? 'FAIL' : 'PASS');
process.exit(fail ? 1 : 0);
