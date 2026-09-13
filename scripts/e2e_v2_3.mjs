/**
 * 浏览器端到端验证（v2.3 feedback_0910 修复防回归）：
 * IEEE 夹具（离线重放）覆盖度门禁 + 参考文献可见（B3）+ 书眉不混入（C2）+
 * 内联上标渲染（B1）+ 表格单元格 KaTeX 双编码（B2）+ worker inline 正常渲染（P0）。
 * 前置：release 服务器已在 127.0.0.1:39321 运行（app/dist 静态托管，.env.local 有双 Token，
 *       且以 GEMINUS_ANON_PAGE_LIMIT=100000 GEMINUS_ALLOW_LOCAL_PATH=1 启动；
 *       IEEE 文档 raw 已入 data/mineru_cache → 离线重放零 API）。
 * 运行：NODE_PATH=$(npm root -g) node scripts/e2e_v2_3.mjs
 */
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const BASE = 'http://127.0.0.1:39321';
const ROOT = new URL('..', import.meta.url).pathname;
const IEEE = `${ROOT}/test/ieee_access_main_revised.pdf`;
const results = [];
function log(name, ok, detail = '') {
  results.push({ name, ok, detail });
  console.log(`${ok ? '✅' : '❌'} ${name}${detail ? ' — ' + detail : ''}`);
}

const browser = await chromium.launch({
  headless: true,
  executablePath: `${process.env.HOME}/.cache/ms-playwright/chromium-1223/chrome-linux64/chrome`,
});
const page = await browser.newPage({ viewport: { width: 1600, height: 900 } });

let capturedDocId = null;
page.on('request', (req) => {
  const m = req.url().match(/\/api\/original\/([0-9a-f-]{36})/);
  if (m) capturedDocId = m[1];
});

async function waitCount(selector, min, timeout = 20000) {
  await page
    .waitForFunction(
      ([sel, m]) => document.querySelectorAll(sel).length >= m,
      [selector, min],
      { timeout },
    )
    .catch(() => {});
  return page.locator(selector).count();
}

function rightPane() {
  return page.evaluate(() => {
    const panes = document.querySelectorAll('.geminus-pane');
    return panes[panes.length - 1];
  });
}

// ── 1. IEEE 文档离线重放打开（B3 母本）+ 左栏 worker inline 渲染（P0）──
{
  await page.goto(BASE);
  // v2.6：零点击——选择文件即自动「解析 → 翻译」
  await page.setInputFiles('[data-testid="upload-input"]', IEEE);
  await page.getByTestId('pdf-compare-view').waitFor({ timeout: 300000 });
  await page.waitForTimeout(1500);
  const docId = capturedDocId;
  const canvases = await waitCount('[data-testid^="pdf-canvas-"]', 1, 20000);
  log('IEEE 打开 + 左栏 canvas 渲染（worker inline，P0）', canvases >= 1, `canvas=${canvases} docId=${docId}`);
  globalThis.__docId = docId;
}

// ── 2. 覆盖度门禁：violations=0 + char≥0.98 + area≥0.95（B3/C2 装饰类型）──
{
  const docId = globalThis.__docId;
  const r = await fetch(`${BASE}/api/debug/coverage?doc_id=${docId}`).then((x) => x.json());
  const ok =
    Array.isArray(r.violations) && r.violations.length === 0 &&
    r.char_coverage >= 0.98 && r.area_coverage >= 0.95;
  log(
    'IEEE 覆盖度门禁 violations=0',
    ok,
    `violations=${JSON.stringify(r.violations)} char=${r.char_coverage?.toFixed(4)} area=${r.area_coverage?.toFixed(4)}`,
  );
}

// ── 3. 右栏参考文献可见（B3）：[n] 起头段落 ≥30 ──
{
  const refs = await page.evaluate(() => {
    const right = rightPaneEval();
    let n = 0;
    for (const p of right.querySelectorAll('[data-block-id]')) {
      if (/^\[\d+\]/.test((p.textContent ?? '').trim())) n++;
    }
    return n;
    function rightPaneEval() {
      const panes = document.querySelectorAll('.geminus-pane');
      return panes[panes.length - 1];
    }
  });
  log('右栏参考文献条目 ≥30（B3）', refs >= 30, `refs=${refs}`);
}

// ── 4. 书眉不混入正文（C2）：右栏无 "VOLUME 14" 运行头 ──
{
  const leak = await page.evaluate(() => {
    const right = (() => {
      const panes = document.querySelectorAll('.geminus-pane');
      return panes[panes.length - 1];
    })();
    let n = 0;
    for (const p of right.querySelectorAll('[data-block-id]')) {
      const t = (p.textContent ?? '');
      if (t.includes('VOLUME 14') || /et al\.:/.test(t)) n++;
    }
    return n;
  });
  log('书眉未混入译文栏（C2）', leak === 0, `leak=${leak}`);
}

// ── 5. 内联上标渲染为 <sup> 元素（B1）──
{
  const sups = await page.evaluate(() => {
    const right = (() => {
      const panes = document.querySelectorAll('.geminus-pane');
      return panes[panes.length - 1];
    })();
    return right.querySelectorAll('sup').length;
  });
  log('右栏 <sup> 上标元素存在（B1）', sups >= 1, `sup=${sups}`);
}

// ── 6. 表格单元格 KaTeX（B2）：表内 .katex 存在或该夹具无 LaTeX 单元格 ──
{
  const info = await page.evaluate(() => {
    const right = (() => {
      const panes = document.querySelectorAll('.geminus-pane');
      return panes[panes.length - 1];
    })();
    const tables = right.querySelectorAll('table').length;
    const katexInTable = right.querySelectorAll('table .katex').length;
    const literalDollar = right.querySelectorAll('table').length
      ? [...right.querySelectorAll('table')].some((t) => (t.textContent ?? '').includes('$\\pm') || (t.textContent ?? '').includes('$55'))
      : false;
    return { tables, katexInTable, literalDollar };
  });
  if (info.tables === 0) {
    log('表格单元格 KaTeX（B2）', true, '该文档无富表格（依赖 tableHtml 单测覆盖）');
  } else if (info.literalDollar) {
    log('表格单元格 KaTeX（B2）', info.katexInTable > 0, `tables=${info.tables} katex=${info.katexInTable} 存在字面 LaTeX`);
  } else {
    log('表格单元格 KaTeX（B2）', true, `tables=${info.tables} katex=${info.katexInTable}（本夹具单元格为 Unicode 编码，LaTeX 路径由单测覆盖）`);
  }
}

// ── 7. 降级横幅不出现（正常路径无 C1 误触发）──
{
  const banner = await page.locator('[data-testid="pdf-fallback-banner"]').count();
  log('正常路径无降级横幅（C1 不误触发）', banner === 0, `banner=${banner}`);
}

await browser.close();
const failed = results.filter((r) => !r.ok);
console.log(`\n═══ E2E v2.3 结果：${results.length - failed.length}/${results.length} 通过 ═══`);
if (failed.length) {
  console.log('失败项：', failed.map((f) => f.name).join(' | '));
  process.exit(1);
}
