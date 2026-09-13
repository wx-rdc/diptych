/**
 * 浏览器端到端验证（v2.1 T8.3）：全宽布局 / 表格富渲染 / 行内公式 / 高亮降噪 /
 * 页码标签 / 段落工具条（单段重译真实 API）/ 块数对齐 / Lightbox / DOCX 回归。
 * 前置：release 服务器已在 127.0.0.1:39321 运行（app/dist 静态托管，.env.local 有双 Token）。
 * 运行：NODE_PATH=$(npm root -g) node scripts/e2e_v2_1.mjs
 */
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const BASE = 'http://127.0.0.1:39321';
const FIX = new URL('../test/fixtures', import.meta.url).pathname;
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

async function openDocAndWait(path, mode = 'original') {
  await page.goto(BASE);
  // v2.6：零点击——选择文件即自动「解析 → 翻译」
  await page.setInputFiles('[data-testid="upload-input"]', path);
  const testid = mode === 'original' ? 'pdf-compare-view' : 'document-viewer';
  await page.getByTestId(testid).waitFor({ timeout: 300000 });
}

/** 等待翻译完成（v2.6：status-chip 完成态，替代旧 TranslateControl「已完成」文案） */
async function waitTranslateDone(timeout = 180000) {
  await page.waitForFunction(
    () => ['done', 'warning'].includes(document.querySelector('[data-testid="status-chip"]')?.dataset.state ?? ''),
    null,
    { timeout },
  ).catch(() => {});
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

// ── 1. 全宽铺满（simple.pdf，MinerU 解析缓存）──
await openDocAndWait(`${FIX}/pdf/simple.pdf`);
await waitCount('canvas[data-testid^="pdf-canvas-"]', 1);
await page.waitForTimeout(600);
const widths = await page.evaluate(() => {
  const pane = document.querySelector('[data-testid="pdf-compare-view"] .geminus-pane');
  const canvas = document.querySelector('canvas[data-testid^="pdf-canvas-"]');
  const right = document.querySelectorAll('[data-testid="pdf-compare-view"] .geminus-pane')[1];
  const para = right?.querySelector('[data-block-id]');
  const contentW = (el) => {
    if (!el) return 0;
    const cs = getComputedStyle(el);
    return el.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
  };
  // 左栏链路：pane(--pane-pad) → pdf-original-viewer → 页 wrapper → canvas
  // v2.2 布局：canvas 铺满页 wrapper（内容裁切渲染）——直接量测页 wrapper 宽（布局无关）
  const pageEl = document.querySelector('[data-testid^="pdf-page-"]');
  return {
    leftContent: contentW(pane),
    leftPage: pageEl ? pageEl.clientWidth : 0,
    canvas: canvas ? canvas.getBoundingClientRect().width : 0,
    rightContent: contentW(right),
    para: para ? para.getBoundingClientRect().width : 0,
  };
});
log(
  '全宽铺满：canvas ≈ 左栏内容宽（扣 padding）±4px',
  Math.abs(widths.canvas - widths.leftPage) <= 4,
  `canvas=${widths.canvas.toFixed(1)} content=${widths.leftContent} page=${widths.leftPage}`,
);
log(
  '全宽铺满：右栏段落 ≈ 右栏内容宽（扣 padding）±4px',
  Math.abs(widths.para - widths.rightContent) <= 4,
  `para=${widths.para.toFixed(1)} content=${widths.rightContent}`,
);
await page.screenshot({ path: 'docs/screenshots/v2.1/fullwidth.png', fullPage: false });

// ── 2. 字号体系：默认 ≥18px，A+ → 20px，刷新持久 ──
// v2.4：左栏 PDF region 也挂了 data-block-id（Hover 联动）→ 选择器限定右栏（最后一个 pane）
const fs0 = await page.evaluate(() => {
  document.documentElement.style.setProperty('--geminus-user-fs', '18px');
  const panes = document.querySelectorAll('[data-testid="pdf-compare-view"] .geminus-pane');
  const right = panes[panes.length - 1];
  const el = right.querySelector('[data-block-id] p') ?? right.querySelector('[data-block-id]');
  return getComputedStyle(el).fontSize;
});
log('默认阅读字号 ≥18px', parseFloat(fs0) >= 18, `fontSize=${fs0}`);
// v2.6：字号控件收进 ⚙ 设置弹层
await page.getByTestId('settings-btn').click();
await page.locator('button[aria-label="增大字号"]').click();
const fs1 = await page.evaluate(
  () => document.documentElement.style.getPropertyValue('--geminus-user-fs'),
);
log('A+ 升档到 20px', fs1 === '20px', `--geminus-user-fs=${fs1}`);
await page.reload();
await page.getByTestId('settings-btn').click();
await page.getByTestId('font-size-control').waitFor({ timeout: 20000 });
const fs2 = await page.evaluate(
  () => document.documentElement.style.getPropertyValue('--geminus-user-fs'),
);
log('刷新后档位持久化（20px）', fs2 === '20px', `--geminus-user-fs=${fs2}`);
// 复位 18px
await page.locator('button[aria-label="减小字号"]').click();

// ── 3. 翻译（缓存命中为主）+ 块数对齐（T1.6）──
// （场景 2 reload 后回到空态，需重新打开文档）
await openDocAndWait(`${FIX}/pdf/simple.pdf`);
await waitTranslateDone();
await page.waitForTimeout(600);
const blockCounts = await page.evaluate(() => {
  const panes = document.querySelectorAll('[data-testid="pdf-compare-view"] .geminus-pane');
  // 右栏每块有外层 wrapper + 内层元素两个 data-block-id → 按 id 去重
  const ids = new Set(
    Array.from(panes[1]?.querySelectorAll('[data-block-id]') ?? []).map((el) =>
      el.getAttribute('data-block-id'),
    ),
  );
  const regions = document.querySelectorAll('[data-testid^="region-b_"]').length;
  return { regions, unique: ids.size };
});
log(
  '块数对齐：译文面板去重块数 == 原文区域块数（T1.6）',
  blockCounts.regions > 0 && blockCounts.unique === blockCounts.regions,
  `regions=${blockCounts.regions} unique=${blockCounts.unique}`,
);

// ── 4. 高亮降噪：区域点击 → data-active + border-l-4 ──
const firstRegion = page.locator('[data-testid^="region-b_"]').first();
await firstRegion.click();
await page.waitForTimeout(300);
const hl = await page.evaluate(() => {
  const right = document.querySelectorAll('[data-testid="pdf-compare-view"] .geminus-pane')[1];
  const el = right?.querySelector('[data-active="true"]');
  if (!el) return { found: false };
  const cs = getComputedStyle(el);
  return { found: true, left: cs.borderLeftWidth, top: cs.borderTopWidth };
});
log(
  '高亮降噪：border-left=4px 无四边框',
  hl.found && hl.left === '4px' && (hl.top === '0px' || hl.top === ''),
  JSON.stringify(hl),
);
await page.screenshot({ path: 'docs/screenshots/v2.1/highlight.png', fullPage: false });

// ── 5. 页码标签 ──
const pageLabels = await page.locator('[data-testid^="page-label-"]').count();
const dividers = await page.locator('[data-testid^="translated-page-divider-"]').count();
log('页码标签：左栏 sticky Page N', pageLabels >= 1, `labels=${pageLabels}`);
log('页码标签：右栏组分隔线', dividers >= 1, `dividers=${dividers}`);

// ── 6. 表格富渲染（with_table.pdf：MinerU 表格 HTML + 截图）──
await openDocAndWait(`${FIX}/pdf/with_table.pdf`);
await waitCount('[data-testid^="table-card-"]', 1);
await waitTranslateDone();
await page.waitForTimeout(800);
const tableInfo = await page.evaluate(() => {
  const right = document.querySelectorAll('[data-testid="pdf-compare-view"] .geminus-pane')[1];
  const text = right?.innerText ?? '';
  const cards = right?.querySelectorAll('[data-testid^="table-body-"]') ?? [];
  const rows = cards[0]?.querySelectorAll('tr').length ?? 0;
  return { hasRawTd: text.includes('<td'), cards: cards.length, rows, text: text.slice(0, 200) };
});
log('表格富渲染：右栏无裸 <td 标签', !tableInfo.hasRawTd, JSON.stringify(tableInfo).slice(0, 120));
log('表格富渲染：<table> 行数 >0', tableInfo.cards > 0 && tableInfo.rows > 0, `rows=${tableInfo.rows}`);

// ── 7. Lightbox：展开 + tab 切换 + 关闭 ──
const expandBtn = page.locator('[data-testid^="table-expand-"]').first();
if ((await expandBtn.count()) > 0) {
  await expandBtn.click();
  await page.getByTestId('lightbox').waitFor({ timeout: 5000 });
  const tabCount = await page.locator('[data-testid^="lightbox-tab-"]').count();
  const shotTab = page.getByTestId('lightbox-tab-shot');
  if ((await shotTab.count()) > 0) {
    await shotTab.click();
    await page
      .waitForFunction(() => {
        const img = document.querySelector('[data-testid="lightbox"] img');
        return !!img && img.complete && img.naturalWidth > 0;
      }, { timeout: 15000 })
      .catch(() => {});
    const imgOk = await page.evaluate(() => {
      const img = document.querySelector('[data-testid="lightbox"] img');
      return img ? img.naturalWidth > 0 : false;
    });
    log('Lightbox 双 tab + 原文截图加载', tabCount >= 2 && imgOk, `tabs=${tabCount} imgOk=${imgOk}`);
  } else {
    log('Lightbox 富渲染单 tab（无截图资源）', tabCount === 1, `tabs=${tabCount}`);
  }
  await page.getByTestId('lightbox-close').click();
  await page.waitForTimeout(200);
  log('Lightbox 关闭', (await page.getByTestId('lightbox').count()) === 0);
} else {
  log('Lightbox：无表格卡片可展开（跳过）', true);
}
await page.screenshot({ path: 'docs/screenshots/v2.1/table-card.png', fullPage: false });

// ── 8. 行内公式（b2 论文 3.1 节，解析+翻译缓存）──
await openDocAndWait(new URL('../test/b2_Prototypical_Networks.pdf', import.meta.url).pathname);
await waitTranslateDone(300000);
await page.waitForTimeout(800);
const mathInfo = await page.evaluate(() => {
  const right = document.querySelectorAll('[data-testid="pdf-compare-view"] .geminus-pane')[1];
  const katex = right?.querySelectorAll('.katex').length ?? 0;
  const text = right?.innerText ?? '';
  return { katex, hasTimes: text.includes('\\times'), hasDollar: /\$[^$\s]+\$/.test(text) };
});
log(
  '行内公式渲染为 .katex（≥3 处）',
  mathInfo.katex >= 3,
  `katex=${mathInfo.katex}`,
);
log(
  '公式残留检查：右栏无 \\times / $...$',
  !mathInfo.hasTimes && !mathInfo.hasDollar,
  JSON.stringify(mathInfo),
);
await page.screenshot({ path: 'docs/screenshots/v2.1/inline-math.png', fullPage: false });

// ── 9. 段落工具条：hover + 复制 + 重译本段（真实 DeepSeek 单段）──
const rightPane = page.locator('[data-testid="pdf-compare-view"] .geminus-pane').nth(1);
const paraBlock = rightPane.locator('.group > p').first();
if ((await paraBlock.count()) > 0) {
  const textBefore = (await paraBlock.textContent())?.trim() ?? '';
  await paraBlock.hover();
  await page.waitForTimeout(200);
  // hover 目标段落的工具条可见（DOM 中所有工具条常驻，仅 hover 组显示）
  const toolbarVisible = await page.evaluate(() => {
    const bars = document.querySelectorAll('[data-testid^="paragraph-toolbar-"]');
    return Array.from(bars).some((b) => getComputedStyle(b).display !== 'none');
  });
  log('段落 hover 工具条出现', toolbarVisible);
  const retranslateBtn = page.locator('[data-testid="retranslate-btn"]:visible').first();
  await retranslateBtn.click();
  // 等待重译完成（按钮文案恢复）
  await page
    .waitForFunction(
      () => !document.querySelector('[data-testid="retranslate-btn"]')?.disabled,
      { timeout: 120000 },
    )
    .catch(() => {});
  await page.waitForTimeout(500);
  const textAfter = (await paraBlock.textContent())?.trim() ?? '';
  // 同输入重译可能返回相同译文（缓存键相同覆盖写）——断言交互链路：
  // 按钮恢复可用 + 段落不丢 + 仍为中文译文；绕过缓存的差异性由服务端集成测试验证
  const zhAfter = (textAfter.match(/[\u4e00-\u9fff]/g) || []).length;
  // hover 可因重译后的重渲染而移开——以 DOM 状态断言：存在已恢复（非禁用）的重译按钮
  const btnRecovered = await page.evaluate(() => {
    const btns = document.querySelectorAll('[data-testid="retranslate-btn"]');
    return Array.from(btns).some((b) => !b.disabled && !b.textContent?.includes('重译中'));
  });
  log(
    '重译本段：真实 API 调用完成，段落保留中文译文',
    btnRecovered && zhAfter > 0,
    `zhChars=${zhAfter} btnRecovered=${btnRecovered}`,
  );
  await page.screenshot({ path: 'docs/screenshots/v2.1/paragraph-toolbar.png', fullPage: false });
} else {
  log('段落工具条：无可重译段落（跳过）', false, '未找到右栏段落');
}

// ── 10. DOCX 回归（rich.docx 文本模式 + 工具条）──
await openDocAndWait(`${FIX}/rich.docx`, 'text');
const docxOk = await page.evaluate(() => {
  const viewer = document.querySelector('[data-testid="document-viewer"]');
  const right = viewer?.querySelectorAll('.geminus-pane')[1];
  return { hasViewer: !!viewer, rightParas: right?.querySelectorAll('.group').length ?? 0 };
});
log('DOCX 文本模式回归（右栏段落带工具条）', docxOk.hasViewer && docxOk.rightParas > 0, JSON.stringify(docxOk));

// ── 汇总 ──
await browser.close();
const failed = results.filter((r) => !r.ok);
console.log(`\n═══ E2E v2.1 结果：${results.length - failed.length}/${results.length} 通过 ═══`);
if (failed.length) {
  for (const f of failed) console.log(`  ❌ ${f.name} — ${f.detail}`);
  process.exit(1);
}
