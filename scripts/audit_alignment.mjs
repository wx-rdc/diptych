#!/usr/bin/env node
/**
 * 对齐审计（v2.1 T1.4）：对指定文件走解析（文件哈希缓存命中则零云端调用）
 * + 翻译（段落级缓存）→ GET /api/debug/audit → missing/extra 非空则 exit 1。
 * 用法：node scripts/audit_alignment.mjs /path/to/file [parser]
 * 前置：release 服务器已在 127.0.0.1:39321 运行（.env.local 有 Token）。
 */
const BASE = 'http://127.0.0.1:39321';
const file = process.argv[2];
const parser = process.argv[3] ?? 'mineru';
if (!file) {
  console.error('用法: node scripts/audit_alignment.mjs <文件路径> [parser=mineru|builtin]');
  process.exit(2);
}

/** POST 并消费 SSE 流（progress 回调），返回 done/complete 载荷；同步 JSON 直接返回 */
async function sseJson(url, body, onData) {
  const res = await fetch(url, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
  const ct = res.headers.get('content-type') ?? '';
  if (!ct.includes('text/event-stream')) return res.json();

  const reader = res.body.getReader();
  const dec = new TextDecoder();
  let buf = '';
  let done = null;
  outer: while (true) {
    const { value, done: rd } = await reader.read();
    if (rd) break;
    buf += dec.decode(value, { stream: true });
    let idx;
    while ((idx = buf.indexOf('\n\n')) >= 0) {
      const block = buf.slice(0, idx);
      buf = buf.slice(idx + 2);
      let event = 'message';
      let data = '';
      for (const line of block.split('\n')) {
        if (line.startsWith('event:')) event = line.slice(6).trim();
        else if (line.startsWith('data:')) data += line.slice(5).trim();
      }
      if (!data) continue;
      const payload = JSON.parse(data);
      if (event === 'done' || event === 'complete') {
        done = payload;
        break outer;
      }
      if (event === 'error') throw new Error(payload.message ?? 'SSE error');
      onData?.(payload);
    }
  }
  if (!done) throw new Error('连接中断：未收到 done/complete 事件');
  return done;
}

const doc = await sseJson(`${BASE}/api/parse`, { file_path: file, parser }, (p) => {
  if (p.stage) console.log(`  [parse] ${p.stage}: ${p.message ?? ''}`);
});
console.log(`解析完成：${doc.blocks.length} 块 (doc_id=${doc.id.slice(0, 8)}…)`);

await sseJson(`${BASE}/api/translate`, { doc_id: doc.id, source_lang: 'en', target_lang: 'zh' }, (p) => {
  if (p.status === 'error') console.log(`  [translate] ⚠️ ${p.block_id}: ${p.error}`);
});

const res = await fetch(`${BASE}/api/debug/audit?doc_id=${doc.id}`);
if (!res.ok) throw new Error(`审计失败: HTTP ${res.status} ${await res.text()}`);
const r = await res.json();
console.log(`审计：original=${r.original_blocks} translated=${r.translated_blocks}`);
console.log(
  `  missing=${r.missing.length} extra=${r.extra.length} empty=${r.empty_translation.length} fallback=${r.fallbacks.length}`,
);
if (r.missing.length) console.log('  missing:', r.missing.join(', '));
if (r.extra.length) console.log('  extra:', r.extra.join(', '));
if (r.empty_translation.length) console.log('  empty:', r.empty_translation.join(', '));
if (r.fallbacks.length) console.log('  fallback:', r.fallbacks.join(', '));
if (r.missing.length || r.extra.length) process.exit(1);
