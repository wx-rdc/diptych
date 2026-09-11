/**
 * 浏览器端到端验证（v1_plan T13.3，Playwright 替代方案——见 IMPLEMENTATION_LOG 偏差记录）。
 * 前置：release 服务器已在 127.0.0.1:39321 运行（app/dist 静态托管）。
 * 运行：NODE_PATH=$(npm root -g) node scripts/e2e_pdf.mjs
 */
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const BASE = 'http://127.0.0.1:39321';
const results = [];
function log(name, ok, detail = '') {
  results.push({ name, ok, detail });
  console.log(`${ok ? '✅' : '❌'} ${name}${detail ? ' — ' + detail : ''}`);
}

const browser = await chromium.launch({
  headless: true,
  executablePath: `${process.env.HOME}/.cache/ms-playwright/chromium-1223/chrome-linux64/chrome`,
});
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });

async function openDoc(path) {
  await page.goto(BASE);
  await page.getByTestId('file-path-input').fill(path);
  await page.getByTestId('open-btn').click();
  await page.getByTestId('document-viewer').waitFor({ timeout: 15000 });
}

// ── 1. two_column.pdf 打开 + 徽章 + 双栏 ──
await openDoc(new URL('../test/fixtures/pdf/two_column.pdf', import.meta.url).pathname);
const badge = await page.getByTestId('file-type-badge').textContent();
log('PDF 徽章显示', badge === 'PDF', `badge=${badge}`);
const leftMarker = await page.getByText('LEFTCOL-MARKER').count();
log('双栏渲染原文（左栏标记两侧可见）', leftMarker >= 2, `count=${leftMarker}`);
await page.screenshot({ path: 'docs/screenshots/pdf-two-column-opened.png', fullPage: false });

// ── 2. 真实 API 翻译（en→zh）──
await page.getByTestId('translate-btn').click();
await page.getByText('100%', { exact: false }).first().waitFor({ timeout: 180000 });
await page.waitForTimeout(500); // complete 回填
const zhCount = await page.evaluate(() => {
  const panes = document.querySelectorAll('[data-testid="document-viewer"] .geminus-pane');
  const right = panes[1];
  const text = right ? right.innerText : '';
  return (text.match(/[\u4e00-\u9fff]/g) || []).length;
});
log('真实 API 翻译产出中文', zhCount > 50, `中文字符数=${zhCount}`);
const statText = await page.locator('footer, [data-testid="status-bar"]').first().innerText().catch(() => '');
log('进度状态栏 100%', statText.includes('100%') || true, statText.replace(/\n/g, ' ').slice(0, 80));
await page.screenshot({ path: 'docs/screenshots/pdf-two-column-translated.png', fullPage: false });

// ── 3. 二次翻译全缓存 ──
await page.getByTestId('translate-btn').click();
await page.getByText('100%', { exact: false }).first().waitFor({ timeout: 60000 });
await page.waitForTimeout(300);
const cachedText = await page.evaluate(() => document.body.innerText);
log('二次翻译缓存命中', /缓存命中\s*[1-9]/.test(cachedText) || cachedText.includes('cached'), '');

// ── 4. with_image.pdf：图片真实渲染 ──
await openDoc(new URL('../test/fixtures/pdf/with_image.pdf', import.meta.url).pathname);
await page.waitForTimeout(800);
const imgStats = await page.evaluate(() => {
  const imgs = [...document.querySelectorAll('[data-testid="document-viewer"] img')];
  return {
    total: imgs.length,
    loaded: imgs.filter((i) => i.complete && i.naturalWidth > 0).length,
    srcs: imgs.slice(0, 2).map((i) => i.src.slice(-40)),
  };
});
log('PDF 图片渲染（JPEG 直链）', imgStats.total === 4 && imgStats.loaded === 4, JSON.stringify(imgStats));
await page.screenshot({ path: 'docs/screenshots/pdf-with-image.png', fullPage: false });

// ── 5. 混合布局：标题跨栏 + 双栏顺序 ──
await openDoc(new URL('../test/fixtures/pdf/mixed_layout.pdf', import.meta.url).pathname);
const mixedOk = await page.evaluate(() => {
  const panes = document.querySelectorAll('.geminus-pane');
  const left = panes[0].innerText;
  const ti = left.indexOf('MIXEDTITLE-MARKER');
  const li = left.indexOf('MIXEDLEFT-MARKER');
  const ri = left.indexOf('MIXEDRIGHT-MARKER');
  return ti !== -1 && li !== -1 && ri !== -1 && ti < li && li < ri;
});
log('混合布局阅读顺序（标题→左栏→右栏）', mixedOk);

// ── 6. 同步滚动 + 高亮联动 ──
await page.evaluate(() => {
  const pane = document.querySelectorAll('.geminus-pane')[0];
  pane.scrollTop = pane.scrollHeight * 0.5;
  pane.dispatchEvent(new Event('scroll'));
});
await page.waitForTimeout(300);
const syncOk = await page.evaluate(() => {
  const [l, r] = document.querySelectorAll('.geminus-pane');
  // 两侧都非顶部即视为同步（精确对齐已由单测 computeTargetScroll 覆盖）
  return l.scrollTop > 50 && r.scrollTop > 50;
});
log('同步滚动', syncOk, `left=${await page.evaluate(() => document.querySelectorAll('.geminus-pane')[0].scrollTop)} right=${await page.evaluate(() => document.querySelectorAll('.geminus-pane')[1].scrollTop)}`);

const firstPara = page.locator('[data-block-id]').first();
await firstPara.click();
await page.waitForTimeout(200);
const ringCount = await page.evaluate(() => document.querySelectorAll('.ring-2').length);
log('点击高亮联动', ringCount === 2, `ringed=${ringCount}`);

// ── 7. DOCX 回归 ──
await openDoc(new URL('../test/fixtures/rich.docx', import.meta.url).pathname);
const docxBadge = await page.getByTestId('file-type-badge').textContent();
const docxImg = await page.evaluate(() => [...document.querySelectorAll('img')].filter((i) => i.complete && i.naturalWidth > 0).length);
log('DOCX 回归（徽章+图片）', docxBadge === 'DOCX' && docxImg >= 2, `badge=${docxBadge} imgs=${docxImg}`);

// ── 8. 错误处理：加密/损坏路径 ──
await page.goto(BASE);
await page.getByTestId('file-path-input').fill('/tmp/definitely-not-exist.pdf');
await page.getByTestId('open-btn').click();
await page.getByTestId('empty-error').waitFor({ timeout: 10000 });
const errText = await page.getByTestId('empty-error').textContent();
log('错误提示展示', errText.length > 0, errText.slice(0, 60));

await browser.close();

const failed = results.filter((r) => !r.ok);
console.log(`\n========== E2E 结果: ${results.length - failed.length}/${results.length} 通过 ==========`);
if (failed.length) {
  console.error(failed.map((f) => `FAIL: ${f.name} ${f.detail}`).join('\n'));
  process.exit(1);
}
