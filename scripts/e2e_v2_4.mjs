/**
 * 浏览器端到端验证（v2.4）：权限收口（require_login）/ 历史记录 / Hover 联动 /
 * Click 居中 / 滚动锁 + 锚点追踪。
 * 自起专项服务器：GEMINUS_REQUIRE_LOGIN=1，端口 39322，db/uploads 隔离到 tempdir，
 * MINERU_API_KEY 置空（全部走 builtin 解析，零云端调用）。
 * 前置：cargo build --release -p geminus-server；(cd app && pnpm build)；
 *       config/users.json 与 .secrets/preset_users.txt 存在（gen_users.py 产物）。
 * 运行：NODE_PATH=$(npm root -g) node scripts/e2e_v2_4.mjs
 */
import { createRequire } from 'node:module';
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const ROOT = new URL('..', import.meta.url).pathname;
const PORT = 39322;
const BASE = `http://127.0.0.1:${PORT}`;
const FIX = `${ROOT}/test/fixtures`;

const results = [];
function log(name, ok, detail = '') {
  results.push({ name, ok, detail });
  console.log(`${ok ? '✅' : '❌'} ${name}${detail ? ' — ' + detail : ''}`);
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ── 0. 专项服务器拉起（require_login=1，状态目录隔离） ──
const stateDir = mkdtempSync(path.join(tmpdir(), 'geminus_v24_'));
const server = spawn(`${ROOT}/target/release/geminus-server`, [], {
  cwd: ROOT,
  env: {
    ...process.env,
    GEMINUS_BIND: `127.0.0.1:${PORT}`,
    GEMINUS_STATIC_DIR: 'app/dist',
    GEMINUS_REQUIRE_LOGIN: '1',
    GEMINUS_ALLOW_LOCAL_PATH: '0',
    GEMINUS_CACHE_PATH: path.join(stateDir, 'cache.db'),
    GEMINUS_AUTH_DB: path.join(stateDir, 'quota.db'),
    GEMINUS_HISTORY_DB: path.join(stateDir, 'history.db'),
    GEMINUS_UPLOADS_DIR: path.join(stateDir, 'uploads'),
    GEMINUS_MINERU_CACHE_DIR: path.join(stateDir, 'mineru_cache'),
    MINERU_API_KEY: '',
  },
  stdio: ['ignore', 'pipe', 'pipe'],
});
server.stderr.on('data', (d) => process.env.E2E_VERBOSE && process.stderr.write(d));

async function waitHealthy() {
  for (let i = 0; i < 60; i++) {
    try {
      const r = await fetch(`${BASE}/api/health`);
      if (r.ok) return true;
    } catch {}
    await sleep(250);
  }
  return false;
}

// 凭据：.secrets/preset_users.txt 首行（tab 分隔 user/pass）
function firstCredential() {
  try {
    const line = readFileSync(`${ROOT}/.secrets/preset_users.txt`, 'utf8').trim().split('\n')[0];
    const [u, p] = line.split('\t');
    return { user: u.trim(), pass: p.trim() };
  } catch {
    return null;
  }
}

try {
  const healthy = await waitHealthy();
  log('专项服务器启动（GEMINUS_REQUIRE_LOGIN=1）', healthy, `port=${PORT} state=${stateDir}`);
  if (!healthy) throw new Error('服务器未就绪');

  const me = await fetch(`${BASE}/api/auth/me`).then((r) => r.json());
  log('me 返回 require_login=true + login_enabled=true',
    me.require_login === true && me.login_enabled === true, JSON.stringify(me));

  const cred = firstCredential();
  log('预设用户凭据可用', !!cred, cred ? `user=${cred.user}` : '.secrets/preset_users.txt 缺失');

  const browser = await chromium.launch({
    headless: true,
    executablePath: `${process.env.HOME}/.cache/ms-playwright/chromium-1223/chrome-linux64/chrome`,
  });
  const page = await browser.newPage({ viewport: { width: 1600, height: 900 } });

  // ── 1. 匿名零点击（v2.6）：上传 simple.pdf → 200 外壳（无 IR）→ 前端引导登录框自动弹出 ──
  {
    let parseStatus = null;
    const onResponse = (res) => { if (res.url().includes('/api/parse')) parseStatus = res.status(); };
    page.on('response', onResponse);
    await page.goto(BASE);
    await page.setInputFiles('[data-testid="upload-input"]', `${FIX}/pdf/simple.pdf`);
    const dialogVisible = await page.getByTestId('login-overlay').waitFor({ timeout: 5000 }).then(() => true).catch(() => false);
    log('匿名上传零点击 → 自动弹登录框', dialogVisible);
    // v2.5 服务端模型：parse 收稿返回 200 外壳（IR 门禁在 /api/documents 与 /api/translate，见 §2 的 401 断言）
    log('匿名 /api/parse → 200 外壳（IR 登录后交付）', parseStatus === 200, `status=${parseStatus}`);
    page.off('response', onResponse);
  }

  // ── 2. 匿名 API 门禁（后端）：上传外壳化 + 查看/翻译 401 ──
  {
    const form = new FormData();
    const bytes = readFileSync(`${FIX}/pdf/simple.pdf`);
    form.append('file', new Blob([bytes], { type: 'application/pdf' }), 'simple.pdf');
    form.append('parser', 'builtin');
    const r = await fetch(`${BASE}/api/parse`, { method: 'POST', body: form });
    const body = await r.json();
    log('匿名上传 → 200 外壳（含 doc_id）', r.status === 200 && !!body.id, `status=${r.status}`);
    log('匿名上传不返回 IR（无 blocks）', !body.blocks && !body.resources);
    const g1 = await fetch(`${BASE}/api/documents/${body.id}`);
    log('匿名 GET /api/documents → 401', g1.status === 401, `status=${g1.status}`);
    const t1 = await fetch(`${BASE}/api/translate`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ doc_id: body.id }),
    });
    log('匿名 POST /api/translate → 401', t1.status === 401, `status=${t1.status}`);
    const h0 = await fetch(`${BASE}/api/history`);
    log('匿名 GET /api/history → 401', h0.status === 401, `status=${h0.status}`);
  }

  // ── 3. 登录 + 重新选择文件（零点击自动续跑：解析+翻译自动开始）→ viewer 出现 ──
  {
    await page.getByTestId('login-username').fill(cred.user);
    await page.getByTestId('login-password').fill(cred.pass);
    await page.getByTestId('login-submit').click();
    await page.getByTestId('auth-username').waitFor({ timeout: 5000 });
    log('登录成功（用户名徽章出现）', true, `user=${cred.user}`);
    // v2.6：零点击无 pending 续跑——登录后重新选择文件即自动打开
    await page.setInputFiles('[data-testid="upload-input"]', `${FIX}/pdf/simple.pdf`);
    const viewer = await Promise.race([
      page.getByTestId('pdf-compare-view').waitFor({ timeout: 30000 }).then(() => 'pdf'),
      page.getByTestId('document-viewer').waitFor({ timeout: 30000 }).then(() => 'text'),
    ]).catch(() => null);
    log('登录后自动续跑打开 → 阅读器出现', !!viewer, `mode=${viewer}`);
    if (!viewer) throw new Error('阅读器未出现');
    await sleep(800); // 渲染稳定
  }

  // ── 4. 历史：面板出现条目 + 点击回载（/api/documents 请求） ──
  {
    let docFetch = null;
    page.on('request', (req) => {
      const m = req.url().match(/\/api\/documents\/([0-9a-f-]{36})/);
      if (m) docFetch = m[1];
    });
    await page.getByTestId('history-btn').click();
    await page.getByTestId('history-panel').waitFor({ timeout: 5000 });
    const itemCount = await page.locator('[data-testid^="history-item-"]').count();
    log('历史面板出现且 ≥1 条', itemCount >= 1, `items=${itemCount}`);
    const firstName = await page.locator('[data-testid^="history-item-"]').first().textContent();
    log('历史条目含文件名 simple.pdf', /simple\.pdf/i.test(firstName ?? ''), (firstName ?? '').slice(0, 60));
    await page.locator('[data-testid^="history-item-"]').first().click();
    const reloaded = await Promise.race([
      page.getByTestId('pdf-compare-view').waitFor({ timeout: 15000 }).then(() => 'pdf'),
      page.getByTestId('document-viewer').waitFor({ timeout: 15000 }).then(() => 'text'),
    ]).catch(() => null);
    log('点击历史条目 → 回载阅读器（GET /api/documents）', !!reloaded && !!docFetch, `doc=${docFetch}`);
    await sleep(800);
  }

  // ── 4.5 换长文档（rich.docx）：交互断言需要可滚动内容 ──
  {
    await page.setInputFiles('[data-testid="upload-input"]', `${FIX}/rich.docx`);
    // viewer testid 在文档切换间不卸载 → 轮询左栏真正可滚动为止
    let scrollable = 0;
    for (let i = 0; i < 40; i++) {
      scrollable = await page.evaluate(() => {
        const left = document.querySelectorAll('.geminus-pane')[0];
        return left ? left.scrollHeight - left.clientHeight : 0;
      });
      if (scrollable > 200) break;
      await sleep(250);
    }
    log('长文档打开且左栏可滚动（交互前置）', scrollable > 200, `scrollable=${Math.round(scrollable)}px`);
    await sleep(500);
  }

  // ── 5. Hover 联动：左栏 hover 块 → 双侧 .g-block-hover ──
  {
    // 取右栏首个块 id（左右同 id）；左栏对应元素可能是文本块或 PDF region
    const firstId = await page.evaluate(() => {
      const panes = document.querySelectorAll('.geminus-pane');
      const right = panes[panes.length - 1];
      return right.querySelector('[data-block-id]')?.dataset.blockId ?? null;
    });
    await page.hover(`[data-block-id="${firstId}"] >> nth=0`); // nth=0 取左栏
    await sleep(250);
    const both = await page.evaluate((id) => {
      const panes = document.querySelectorAll('.geminus-pane');
      const [left, right] = [panes[0], panes[panes.length - 1]];
      return {
        left: left.querySelector(`[data-block-id="${id}"]`)?.classList.contains('g-block-hover') ?? false,
        right: right.querySelector(`[data-block-id="${id}"]`)?.classList.contains('g-block-hover') ?? false,
      };
    }, firstId);
    log('Hover 联动：双侧同 id 块挂 .g-block-hover', both.left && both.right, JSON.stringify(both));
    // 移出 pane → 类清除
    await page.mouse.move(4, 4);
    await sleep(250);
    const cleared = await page.evaluate(() => document.querySelectorAll('.g-block-hover').length);
    log('鼠标移出面板 → hover 类清除', cleared === 0, `remaining=${cleared}`);
  }

  // ── 6. Click 居中：点右栏中部块 → 双侧块中心对齐视口中心 ──
  {
    const targetId = await page.evaluate(() => {
      const panes = document.querySelectorAll('.geminus-pane');
      const right = panes[panes.length - 1];
      const rp = right.getBoundingClientRect();
      let best = null; let bestDist = Infinity;
      for (const el of right.querySelectorAll('[data-block-id]')) {
        const r = el.getBoundingClientRect();
        const d = Math.abs(r.top + r.height / 2 - (rp.top + rp.height / 2));
        if (d < bestDist) { bestDist = d; best = el.dataset.blockId; }
      }
      return best;
    });
    await page.click(`[data-block-id="${targetId}"] >> nth=-1`); // nth=-1 取右栏
    await sleep(1000); // 双侧 smooth 滚动
    const dists = await page.evaluate((id) => {
      const panes = document.querySelectorAll('.geminus-pane');
      const [left, right] = [panes[0], panes[panes.length - 1]];
      const centerDist = (pane) => {
        const el = pane.querySelector(`[data-block-id="${id}"]`);
        if (!el) return null;
        const pr = pane.getBoundingClientRect();
        const er = el.getBoundingClientRect();
        return Math.abs(er.top + er.height / 2 - (pr.top + pr.height / 2));
      };
      return { left: centerDist(left), right: centerDist(right) };
    }, targetId);
    log('Click 居中：双侧目标块中心距视口中心 <80px',
      dists.left !== null && dists.right !== null && dists.left < 80 && dists.right < 80,
      `left=${Math.round(dists.left ?? -1)}px right=${Math.round(dists.right ?? -1)}px`);
  }

  // ── 7. 滚动锁 + 锚点追踪：hover 左栏 → 滚动 → 右栏把左栏 hover 块居中且左栏无回滚 ──
  {
    // hover 左栏中部某块
    const hoverId = await page.evaluate(() => {
      const panes = document.querySelectorAll('.geminus-pane');
      const left = panes[0];
      const lp = left.getBoundingClientRect();
      let best = null; let bestDist = Infinity;
      for (const el of left.querySelectorAll('[data-block-id]')) {
        const r = el.getBoundingClientRect();
        const d = Math.abs(r.top + r.height / 2 - (lp.top + lp.height / 2));
        if (d < bestDist) { bestDist = d; best = el.dataset.blockId; }
      }
      return best;
    });
    await page.hover(`[data-block-id="${hoverId}"] >> nth=0`);
    await sleep(200);
    const scrollSet = await page.evaluate(() => {
      const left = document.querySelectorAll('.geminus-pane')[0];
      left.scrollTop += 260;
      return left.scrollTop;
    });
    await sleep(150);
    const leftAfter100 = await page.evaluate(() => document.querySelectorAll('.geminus-pane')[0].scrollTop);
    await sleep(900); // lerp 收敛
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
      return {
        id,
        rightCenterDist: Math.abs(tr.top + tr.height / 2 - (rp.top + rp.height / 2)),
        leftStable: Math.abs(left.scrollTop - document.querySelectorAll('.geminus-pane')[0].scrollTop) < 2,
      };
    });
    log('锚点追踪：右栏将左栏当前 hover 块居中（≤80px）',
      !!res && res.rightCenterDist <= 80, res ? `id=${res.id} dist=${Math.round(res.rightCenterDist)}px` : '未找到 hover 块');
    const leftFinal = await page.evaluate(() => document.querySelectorAll('.geminus-pane')[0].scrollTop);
    log('滚动锁：master（左栏）scrollTop 稳定无回滚', Math.abs(leftFinal - scrollSet) <= 2 && Math.abs(leftAfter100 - scrollSet) <= 2,
      `set=${scrollSet} final=${Math.round(leftFinal)}`);
  }

  await browser.close();
} catch (e) {
  log('E2E 执行异常', false, String(e));
} finally {
  server.kill('SIGTERM');
  await sleep(300);
  server.kill('SIGKILL');
}

const failed = results.filter((r) => !r.ok).length;
console.log(`\n结果：${results.length - failed} 通过 / ${failed} 失败`);
process.exit(failed ? 1 : 0);
