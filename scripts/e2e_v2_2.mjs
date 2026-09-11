/**
 * 浏览器端到端验证（v2.2 T8）：解析覆盖度（b2 题注/算法/chart）/ 双栏几何统一（等宽+裁切）/
 * 块锚点同步 / 公式保真 / 控件锚定 / 覆盖度门禁 / DOCX 回归。
 * 前置：release 服务器已在 127.0.0.1:39321 运行（app/dist 静态托管，.env.local 有双 Token），
 *       且以 GEMINUS_ANON_PAGE_LIMIT=100000 GEMINUS_ALLOW_LOCAL_PATH=1 启动（部署加固流的门控放开）。
 * 运行：NODE_PATH=$(npm root -g) node scripts/e2e_v2_2.mjs
 */
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const BASE = 'http://127.0.0.1:39321';
const ROOT = new URL('..', import.meta.url).pathname;
const FIX = `${ROOT}/test/fixtures`;
const B2 = `${ROOT}/test/b2_Prototypical_Networks.pdf`;
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

// docId 捕获：监听 /api/original/<id> 请求（performance 条目时序不可靠）
let capturedDocId = null;
page.on('request', (req) => {
  const m = req.url().match(/\/api\/original\/([0-9a-f-]{36})/);
  if (m) capturedDocId = m[1];
});

async function openDocAndWait(path, mode = 'original') {
  await page.goto(BASE);
  await page.getByTestId('file-path-input').fill(path);
  await page.getByTestId('open-btn').click();
  const testid = mode === 'original' ? 'pdf-compare-view' : 'document-viewer';
  await page.getByTestId(testid).waitFor({ timeout: 300000 });
}

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

// ── 1. b2 离线重放解析（schema bump 后走 raw 重放，零 API）+ 块数 ──
let docId = null;
{
  await openDocAndWait(B2);
  await page.waitForTimeout(1500); // 等 pdf.js 拉取 /api/original/<id>
  docId = capturedDocId;
  const regions = await waitCount('[data-testid^="region-b_"]', 100, 30000);
  log('b2 解析（重放/缓存）区域数 ≥100', regions >= 100, `regions=${regions} docId=${docId}`);
}

// ── 2. 覆盖度门禁：violations=0 + char≥0.98 + area≥0.95 ──
{
  const r = await fetch(`${BASE}/api/debug/coverage?doc_id=${docId}`).then((x) => x.json());
  const ok =
    Array.isArray(r.violations) &&
    r.violations.length === 0 &&
    r.char_coverage >= 0.98 &&
    r.area_coverage >= 0.95;
  log(
    '覆盖度门禁 violations=0',
    ok,
    `violations=${JSON.stringify(r.violations)} char=${r.char_coverage?.toFixed(4)} area=${r.area_coverage?.toFixed(4)}`,
  );
}

// ── 3. 题注块：右栏含 Figure 1 题注（含 KaTeX 行内公式）+ 图注块数 ──
{
  const captionCount = await page.evaluate(() => {
    const panes = document.querySelectorAll('.geminus-pane');
    const right = panes[panes.length - 1];
    // is_caption 段渲染为 0.9em 居中——按文本特征找 "Figure 1:" / "Table N:"
    let n = 0;
    for (const p of right.querySelectorAll('[data-block-id]')) {
      const t = p.textContent ?? '';
      if (/^(Figure|Table)\s*\d+[:.]/.test(t.trim())) n++;
    }
    return n;
  });
  const fig1Katex = await page.evaluate(() => {
    const panes = document.querySelectorAll('.geminus-pane');
    const right = panes[panes.length - 1];
    for (const p of right.querySelectorAll('[data-block-id]')) {
      if ((p.textContent ?? '').trim().startsWith('Figure 1')) {
        return p.querySelectorAll('.katex').length;
      }
    }
    return -1;
  });
  log('题注块 ≥10（4 figure + 6 table）', captionCount >= 10, `captions=${captionCount}`);
  log('Figure 1 题注含行内公式 KaTeX', fig1Katex > 0, `katex=${fig1Katex}`);
}

// ── 4. 算法块：AlgorithmCard 存在 + 行数 + Output Label 行 + 高亮联动 ──
{
  const cards = await waitCount('[data-testid^="algorithm-card-"]', 1, 10000);
  const info = await page.evaluate(() => {
    const card = document.querySelector('[data-testid^="algorithm-card-"]');
    if (!card) return null;
    const lines = card.querySelectorAll('[data-testid^="algo-line-"]');
    const firstLabel = lines[0]?.getAttribute('data-kind');
    const firstText = lines[0]?.textContent ?? '';
    return { lines: lines.length, firstLabel, firstText: firstText.trim().slice(0, 40) };
  });
  log('AlgorithmCard 存在', cards >= 1 && !!info, `cards=${cards}`);
  log('算法行数 ≥10 且首行 Label 含 Output', !!info && info.lines >= 10 && info.firstLabel === 'label' && info.firstText.includes('Output'),
    JSON.stringify(info));
  // 左栏点击算法区域 → 右栏整卡 data-active
  const highlight = await page.evaluate(() => {
    const card = document.querySelector('[data-testid^="algorithm-card-"]');
    const id = card?.getAttribute('data-block-id');
    const region = document.querySelector(`[data-testid="region-${id}"]`);
    region?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    return new Promise((resolve) => {
      setTimeout(() => {
        const active = document.querySelector(`[data-testid="algorithm-card-${id}"]`);
        resolve(active?.getAttribute('data-active') === 'true');
      }, 300);
    });
  });
  log('左栏算法区域点击 ↔ 右栏整卡高亮', highlight);
}

// ── 5. 几何：两栏内容宽一致 + 首行文本像素靠上（白边裁切） ──
{
  const geo = await page.evaluate(() => {
    const panes = document.querySelectorAll('.geminus-pane');
    const [left, right] = panes;
    const padL = getComputedStyle(left).paddingLeft;
    const padR = getComputedStyle(right).paddingLeft;
    const contentL = left.clientWidth - parseFloat(padL) * 2;
    const contentR = right.clientWidth - parseFloat(padR) * 2;
    const canvas = document.querySelector('[data-testid="pdf-canvas-1"]');
    let firstInkY = -1;
    if (canvas) {
      const ctx = canvas.getContext('2d');
      const { width, height } = canvas;
      const data = ctx.getImageData(0, 0, width, height).data;
      outer: for (let y = 0; y < height; y++) {
        for (let x = 0; x < width; x += 4) {
          const i = (y * width + x) * 4;
          // 非白像素（容忍反锯齿阈值 235）
          if (data[i] < 235 || data[i + 1] < 235 || data[i + 2] < 235) {
            firstInkY = y;
            break outer;
          }
        }
      }
    }
    return {
      padL,
      padR,
      contentL: Math.round(contentL),
      contentR: Math.round(contentR),
      canvasW: canvas?.width ?? 0,
      canvasH: canvas?.height ?? 0,
      firstInkY,
      inkRatio: canvas && firstInkY >= 0 ? firstInkY / canvas.height : -1,
    };
  });
  log(
    '两栏 padding 令牌一致',
    geo.padL === geo.padR && geo.padL !== '0px',
    `pad=${geo.padL}/${geo.padR}`,
  );
  log(
    '两栏内容宽差 ≤4px',
    Math.abs(geo.contentL - geo.contentR) <= 4,
    `left=${geo.contentL} right=${geo.contentR}`,
  );
  log(
    '白边裁切：首行像素 y < 8% canvas 高',
    geo.inkRatio >= 0 && geo.inkRatio < 0.08,
    `firstInkY=${geo.firstInkY}/${geo.canvasH} ratio=${geo.inkRatio?.toFixed(3)}`,
  );
}

// ── 6. 公式保真：右栏无 $ 定界符残留 + .katex 数与 IR 行内公式资源相当 ──
{
  const math = await page.evaluate(async () => {
    const panes = document.querySelectorAll('.geminus-pane');
    const right = panes[panes.length - 1];
    const text = right.innerText;
    const katexCount = right.querySelectorAll('.katex').length;
    return { hasDollar: /\$[^$]+\$/.test(text), katexCount };
  });
  log('译文区无 $..$ 定界符残留（公式结构化）', !math.hasDollar, `katex=${math.katexCount}`);
  log('行内公式 KaTeX 数量 ≥45（61 inline+8 display，少量降级占位）', math.katexCount >= 45, `katex=${math.katexCount}`);
}

// ── 7. Hover 驱动同步（v2.4 模型）：hover 左栏算法块 → 滚动左栏 → 右栏把左栏当前 hover 块居中 ──
{
  const cardId = await page.evaluate(() => {
    const card0 = document.querySelector('[data-testid^="algorithm-card-"]');
    return card0 ? card0.getAttribute('data-block-id') : null;
  });
  // hover 左栏算法 region → master=left、hovered=算法块（region 已挂 data-block-id）
  await page.hover(`[data-block-id="${cardId}"] >> nth=0`);
  await page.waitForTimeout(150);
  // 滚动左栏（master 滚动事件；Chromium 滚动后更新 hover 目标 = 焦点跟随）
  await page.evaluate(() => {
    const left = document.querySelectorAll('.geminus-pane')[0];
    left.scrollTop += 260;
  });
  await page.waitForTimeout(900); // lerp 收敛
  const res = await page.evaluate(() => {
    const panes = document.querySelectorAll('.geminus-pane');
    const [left, right] = [panes[0], panes[panes.length - 1]];
    const hovered = left.querySelector('.g-block-hover');
    if (!hovered) return null;
    const id = hovered.dataset.blockId;
    const target = right.querySelector(`[data-block-id="${id}"]`);
    if (!target) return null;
    const rp = right.getBoundingClientRect();
    const tr = target.getBoundingClientRect();
    const lp = left.getBoundingClientRect();
    const lr = hovered.getBoundingClientRect();
    return {
      id,
      rightCenterDist: Math.abs(tr.top + tr.height / 2 - (rp.top + rp.height / 2)),
      hoveredInViewport: lr.bottom > lp.top && lr.top < lp.bottom,
    };
  });
  log(
    'Hover 追踪：右栏将左栏 hover 块居中（≤80px）',
    !!res && res.rightCenterDist <= 80,
    res ? `id=${res.id} dist=${Math.round(res.rightCenterDist)}px` : '未找到 hover 块',
  );
  log('hover 块仍在左栏视口内（追踪未失控）', !!res && res.hoveredInViewport);
}

// ── 8. 控件锚定：hover 公式块 → 按钮在块内右上角 ──
{
  const btn = await page.evaluate(() => {
    const btn = document.querySelector('[data-testid^="copy-latex-"]');
    if (!btn) return null;
    const block = btn.closest('[data-block-id]');
    const bs = block.getBoundingClientRect();
    const r = btn.getBoundingClientRect();
    const pane = block.closest('.geminus-pane');
    // 视口坐标系：按钮右缘 ≤ 面板内容区右缘（rect.right − 滚动条宽）
    const scrollbar = pane ? pane.offsetWidth - pane.clientWidth : 0;
    const paneRight = pane ? pane.getBoundingClientRect().right - scrollbar : Infinity;
    return {
      inBlock: r.top >= bs.top - 1 && r.right <= bs.right + 1,
      notOverScrollbar: r.right <= paneRight + 1,
    };
  });
  log('复制 LaTeX 按钮位于公式块内', !!btn && btn.inBlock);
  log('按钮不覆盖滚动条', !!btn && btn.notOverScrollbar);
}

// ── 9. 翻译链路冒烟（真实 LLM，b2 合并 chunk + 算法行级） ──
{
  await page.getByTestId('translate-btn').click();
  await page
    .waitForFunction(
      () => document.querySelector('[data-testid="progress-text"]')?.textContent?.includes('100%'),
      { timeout: 900000 },
    )
    .catch(() => {});
  const stat = await page.evaluate(() => document.querySelector('[data-testid="progress-text"]')?.textContent ?? '');
  const zhChars = await page.evaluate(() => {
    const panes = document.querySelectorAll('.geminus-pane');
    const right = panes[panes.length - 1];
    return (right.innerText.match(/[\u4e00-\u9fff]/g) ?? []).length;
  });
  log('b2 翻译完成（100%）', stat.includes('100%'), stat);
  log('译文含 ≥500 中文字符', zhChars >= 500, `zh=${zhChars}`);
  // 覆盖度在翻译后仍守恒（IR 未变）
  const r2 = await fetch(`${BASE}/api/debug/coverage?doc_id=${docId}`).then((x) => x.json());
  log('翻译后覆盖度仍 violations=0', r2.violations?.length === 0, `char=${r2.char_coverage?.toFixed(4)}`);
}

// ── 10. 截图存档 ──
{
  await page.screenshot({ path: `${ROOT}/docs/screenshots/v2.2/01-b2-geometry.png`, fullPage: false });
  await page.evaluate(() => {
    const card = document.querySelector('[data-testid^="algorithm-card-"]');
    card?.scrollIntoView({ block: 'center' });
  });
  await page.waitForTimeout(600);
  await page.screenshot({ path: `${ROOT}/docs/screenshots/v2.2/02-algorithm-card.png` });
  await page.evaluate(() => {
    const panes = document.querySelectorAll('.geminus-pane');
    const right = panes[panes.length - 1];
    for (const p of right.querySelectorAll('[data-block-id]')) {
      if ((p.textContent ?? '').trim().startsWith('Figure 1')) {
        p.scrollIntoView({ block: 'center' });
        break;
      }
    }
  });
  await page.waitForTimeout(600);
  await page.screenshot({ path: `${ROOT}/docs/screenshots/v2.2/03-figure-caption.png` });
  log('截图存档 docs/screenshots/v2.2/', true);
}

// ── 11. DOCX 回归（rich.docx 文本模式） ──
{
  await openDocAndWait(`${FIX}/rich.docx`, 'text');
  await page.getByTestId('translate-btn').click();
  await page
    .waitForFunction(
      () => document.querySelector('[data-testid="progress-text"]')?.textContent?.includes('100%'),
      { timeout: 300000 },
    )
    .catch(() => {});
  const zh = await page.evaluate(() => {
    const panes = document.querySelectorAll('.geminus-pane');
    return (panes[panes.length - 1].innerText.match(/[\u4e00-\u9fff]/g) ?? []).length;
  });
  log('DOCX 回归（rich.docx 翻译链路）', zh > 100, `zh=${zh}`);
}

await browser.close();
const failed = results.filter((r) => !r.ok);
console.log(`\n═══ v2.2 E2E：${results.length - failed.length}/${results.length} ═══`);
for (const f of failed) console.log(`❌ ${f.name} — ${f.detail}`);
process.exit(failed.length ? 1 : 0);
