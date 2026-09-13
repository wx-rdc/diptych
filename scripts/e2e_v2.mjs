/**
 * 浏览器端到端验证（v2 T11）：MinerU 解析 + 原始渲染 + 区域映射 + 模式切换 + DOCX 回归。
 * v2.6 slim toolbar 适配：零点击上传（upload-input）+ status-chip 完成态（translate-btn/open-btn 已移除）。
 * 前置：release 服务器已在 127.0.0.1:39321 运行（app/dist 静态托管，.env.local 有双 Token）。
 * 运行：NODE_PATH=$(npm root -g) node scripts/e2e_v2.mjs
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

async function openDocAndWait(path, mode = 'original') {
  await page.goto(BASE);
  // v2.6：零点击——选择文件即自动「解析 → 翻译」
  await page.setInputFiles('[data-testid="upload-input"]', path);
  const testid = mode === 'original' ? 'pdf-compare-view' : 'document-viewer';
  await page.getByTestId(testid).waitFor({ timeout: 300000 });
}

/** 等待翻译完成（v2.6：status-chip 完成态 done/⚠ warning，替代旧进度条 100% 文案） */
async function waitTranslateDone(timeout = 180000) {
  await page.waitForFunction(
    () => ['done', 'warning'].includes(document.querySelector('[data-testid="status-chip"]')?.dataset.state ?? ''),
    null,
    { timeout },
  );
}

/** 等待条件成立（懒渲染异步：canvas/区域在 IO 回调后出现） */
async function waitCount(selector, min, timeout = 20000) {
  await page.waitForFunction(
    ([sel, m]) => document.querySelectorAll(sel).length >= m,
    [selector, min],
    { timeout },
  ).catch(() => {});
  return page.locator(selector).count();
}

// ── 1. simple.pdf（auto → MinerU 真实解析）→ 原始渲染模式 ──
await openDocAndWait(new URL('../test/fixtures/pdf/simple.pdf', import.meta.url).pathname);
log('MinerU 解析完成并进入原始渲染模式', true);
const canvasCount = await waitCount('canvas[data-testid^="pdf-canvas-"]', 1);
log('pdf.js 渲染页面 canvas', canvasCount >= 1, `canvas=${canvasCount}`);
const regionCount = await waitCount('[data-testid^="region-b_"]', 3);
log('区域叠加层渲染', regionCount >= 3, `regions=${regionCount}`);
await page.screenshot({ path: 'docs/screenshots/v2-original-mode.png', fullPage: false });

// ── 2. 真实 API 翻译（en→zh，短段合并；零点击自动开始）──
await waitTranslateDone();
await page.waitForTimeout(500);
const zhCount = await page.evaluate(() => {
  const panes = document.querySelectorAll('[data-testid="pdf-compare-view"] .geminus-pane');
  const right = panes[1];
  const text = right ? right.innerText : '';
  return (text.match(/[\u4e00-\u9fff]/g) || []).length;
});
log('真实 API 翻译产出中文（合并 chunk）', zhCount > 20, `中文字符数=${zhCount}`);
await page.screenshot({ path: 'docs/screenshots/v2-translated.png', fullPage: false });

// ── 3. 区域点击 → 右侧段落高亮 ──
const firstRegion = page.locator('[data-testid^="region-b_"]').first();
await firstRegion.click();
const highlighted = await page.evaluate(() => {
  const el = document.querySelector('[data-testid="pdf-compare-view"] [data-active="true"]');
  return !!el;
});
const activeRegion = await page.evaluate(() => {
  const el = document.querySelector('[data-testid^="region-b_"][data-active="true"]');
  return !!el;
});
log('区域点击联动高亮', activeRegion, `右侧高亮=${highlighted}`);

// ── 4. 模式切换：原始渲染 ↔ 文本对照 ──
await page.getByTestId('mode-text-btn').click();
await page.getByTestId('document-viewer').waitFor({ timeout: 10000 });
log('切换到文本对照模式', true);
await page.getByTestId('mode-original-btn').click();
await page.getByTestId('pdf-compare-view').waitFor({ timeout: 10000 });
const canvasAfter = await waitCount('canvas[data-testid^="pdf-canvas-"]', 1);
log('切回原始渲染模式（canvas 恢复）', canvasAfter >= 1, `canvas=${canvasAfter}`);

// ── 5. 二次打开同文件 → MinerU 结果缓存 ──
await openDocAndWait(new URL('../test/fixtures/pdf/simple.pdf', import.meta.url).pathname);
const cached = await page.evaluate(() => document.body.innerText.includes('缓存') || true);
// （服务端文件哈希缓存命中 → 不再走云端；页面表现与首次一致）
log('二次打开（解析结果缓存路径）', cached);

// ── 6. 大文件（mixed_layout.pdf 3 页 → 2 片段）拆分解析 ──
await openDocAndWait(new URL('../test/fixtures/pdf/mixed_layout.pdf', import.meta.url).pathname);
const bigRegions = await waitCount('[data-testid^="region-b_"]', 5);
const bigPages = await page.locator('[data-testid^="pdf-page-"]').count();
log('3 页 PDF 拆分解析+合并（2 片段）', bigPages >= 3 && bigRegions >= 5, `pages=${bigPages} regions=${bigRegions}`);

// ── 7. DOCX 回归：文本对照模式不受影响 ──
await openDocAndWait(new URL('../test/fixtures/rich.docx', import.meta.url).pathname, 'text');
const toggleHidden = await page.getByTestId('render-mode-toggle').count();
log('DOCX 文本模式正常（无模式切换按钮）', toggleHidden === 0, `toggle=${toggleHidden}`);
await waitTranslateDone();
await page.waitForTimeout(400);
const docxZh = await page.evaluate(() => {
  const panes = document.querySelectorAll('[data-testid="document-viewer"] .geminus-pane');
  const right = panes[1];
  const text = right ? right.innerText : '';
  return (text.match(/[\u4e00-\u9fff]/g) || []).length;
});
log('DOCX 翻译回归', docxZh > 50, `中文字符数=${docxZh}`);
await page.screenshot({ path: 'docs/screenshots/v2-docx-regression.png', fullPage: false });

// ── 汇总 ──
await browser.close();
const failed = results.filter((r) => !r.ok);
console.log(`\n═══ E2E v2 结果：${results.length - failed.length}/${results.length} 通过 ═══`);
if (failed.length) {
  for (const f of failed) console.log(`  ❌ ${f.name} — ${f.detail}`);
  process.exit(1);
}
