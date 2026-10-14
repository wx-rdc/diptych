# Geminus Slim Toolbar（工具栏与信息栏重设计）实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 按 `docs/_local/ui_design_0913.md` 重构前端工具栏——48px 单行顶栏（7 区）、移除底部信息栏与栏头、三个 Popover（状态/设置/配额）+ Toast 收纳低频信息，并同步更新全部 E2E 脚本后发布。

**架构：** 纯前端改造（React 18 + TS + Tailwind，vitest 单测）。新增 `Popover` 原语 + `toolbar/` 子目录（ToolbarSlim / StatusChip / SettingsPopover / QuotaChip）+ `Toast`；`Pane` 去栏头改极淡浮标；删除 `Toolbar` / `StatusBar` / `TranslateControl` / `QuotaIndicator`（功能并入新组件）；App.tsx 重接线（停止/继续/重译/重试失败段迁入 Status Popover）。后端零改动。

**技术栈：** React 18、TypeScript、Tailwind CSS 3、vitest + @testing-library/react、Playwright（E2E 脚本模式）。

**关键背景（探索结论，实施者必读）：**

1. **E2E 已过期**：v2.4 零点击移除了 `open-btn`/`translate-btn`，但 `scripts/e2e_v2.mjs`、`e2e_v2_1.mjs`、`e2e_v2_2.mjs`、`e2e_v2_3.mjs`、`e2e_v2_4.mjs` 仍点击它们——当前 main 上这些脚本本就跑不绿。本计划任务 9 一并适配（零点击 + 新选择器）。
2. `file-path-input`（本地路径开发分支）从顶栏挪进 Settings Popover 的「开发」小节（**无条件渲染**，E2E/单测仍可用 Enter 触发流水线；E2E 主路径改用 `upload-input` 上传，更贴近真实用户）。
3. 「重试失败段」用现成 API `retranslateParagraph(docId, blockId, src, tgt)`（`app/src/services/backend.ts:256`）逐段重译 `fallbackIds`，与「重新翻译全文」（整跑 `translateDoc`，缓存秒回）语义区分。
4. 游客/登录均为 finite 额度（5/24h 与 1000/24h，`GET /api/quota`）；本地直连 `unlimited:true` 时配额 Chip 隐藏。
5. 保留 testid：`upload-btn`/`upload-input`/`login-btn`/`auth-username`/`history-btn`/`logout-btn`/`render-mode-toggle`/`mode-original-btn`/`mode-text-btn`/`font-size-control`/`font-size-value`/`file-path-input`/`quota-indicator`/`file-path-input`/`login-overlay`。移除 testid：`anon-hint`/`progress-bar`/`progress-text`/`status-error`/`translate-control`/`translate-*`/`parse-progress`。
6. `doc.title`（后端标题，截图中的"哈希串"）不再渲染于顶栏与栏头；顶栏文档名用 `filePath || doc.title`。
7. 发布走项目技能 geminus-deploy（`deploy/publish.sh <tag> --public-base=http://x.wx-rdc.top`）。

**文件结构（创建/修改/删除总览）：**

| 动作 | 路径 | 职责 |
|------|------|------|
| 创建 | `app/src/components/Popover.tsx` | Popover 原语：触发器+面板、Esc/点击外部关闭 |
| 创建 | `app/src/components/toolbar/StatusChip.tsx` | 状态 Chip（⟳/✓/⚠/⏸/解析中）+ 状态 Popover（计数/停止/继续/重试失败/重译） |
| 创建 | `app/src/components/toolbar/SettingsPopover.tsx` | ⚙ 设置：字号 A−/A+、语言对、开发路径输入 |
| 创建 | `app/src/components/toolbar/QuotaChip.tsx` | ◔ 配额 Chip + 配额 Popover（替代 QuotaIndicator） |
| 创建 | `app/src/components/toolbar/ToolbarSlim.tsx` | 48px 单行顶栏组装 + 2px 顶栏下缘进度条 + `progressLabel` 迁入 |
| 创建 | `app/src/components/Toast.tsx` | 瞬时错误 Toast（自动消失） |
| 修改 | `app/src/App.tsx` | 接线新组件；删 StatusBar/TranslateControl；新增 retryFailed/stop 回调与 Toast |
| 修改 | `app/src/components/Pane.tsx` | 移除栏头 → 极淡浮标 label（不占布局高度） |
| 修改 | `app/src/components/DocumentViewer.tsx`、`PdfCompareView.tsx` | Pane 新 props；类型徽章移至顶栏 |
| 删除 | `app/src/components/Toolbar.tsx`、`StatusBar.tsx`、`TranslateControl.tsx`、`QuotaIndicator.tsx` | 旧组件 |
| 删除 | `app/src/components/__tests__/TranslateControl.test.tsx`、`QuotaIndicator.test.tsx` | 由 StatusChip/QuotaChip 测试替代 |
| 修改 | `app/src/__tests__/App.test.tsx`、`PdfCompareView.test.tsx`（如引用栏头） | 新选择器 |
| 修改 | `scripts/e2e_v2.mjs`、`e2e_v2_1.mjs`、`e2e_v2_2.mjs`、`e2e_v2_3.mjs`、`e2e_v2_4.mjs` | 零点击 + 新选择器适配 |
| 修改 | `docs/_local/IMPLEMENTATION_LOG.md` | 完成情况记录 |

---

### 任务 0：基线确认

- [ ] **步骤 0.1：跑当前前端单测记录基线**

运行：`cd /home/zhu/work/projects/geminus/app && pnpm test`
预期：全绿（约 133+ 用例）。若有红，先记录（不修，本计划外）。

- [ ] **步骤 0.2：确认分支干净**

运行：`git status --short`
预期：clean（main）。

---

### 任务 1：Popover 原语（TDD）

**文件：**
- 创建：`app/src/components/Popover.tsx`
- 测试：`app/src/components/__tests__/Popover.test.tsx`

- [ ] **步骤 1.1：编写失败的测试**

```tsx
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import Popover from '../Popover';

afterEach(cleanup);

describe('Popover 原语', () => {
  it('点击触发器开面板；再点关闭', () => {
    render(
      <Popover
        panelTestId="p1"
        trigger={({ toggle }) => <button onClick={toggle} data-testid="trigger">触发</button>}
        panel={(close) => <button onClick={close} data-testid="act">动作</button>}
      />,
    );
    expect(screen.queryByTestId('p1')).toBeNull();
    fireEvent.click(screen.getByTestId('trigger'));
    expect(screen.getByTestId('p1')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('trigger'));
    expect(screen.queryByTestId('p1')).toBeNull();
  });

  it('面板动作 close() 关闭面板', () => {
    render(
      <Popover
        panelTestId="p2"
        trigger={({ toggle }) => <button onClick={toggle}>t</button>}
        panel={(close) => <button onClick={close} data-testid="act">x</button>}
      />,
    );
    fireEvent.click(screen.getByText('t'));
    fireEvent.click(screen.getByTestId('act'));
    expect(screen.queryByTestId('p2')).toBeNull();
  });

  it('Esc 关闭；点击面板外部关闭', () => {
    render(
      <Popover
        panelTestId="p3"
        trigger={({ toggle }) => <button onClick={toggle}>t</button>}
        panel={() => <span>content</span>}
      />,
    );
    fireEvent.click(screen.getByText('t'));
    expect(screen.getByTestId('p3')).toBeInTheDocument();
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByTestId('p3')).toBeNull();
    // 外部点击
    fireEvent.click(screen.getByText('t'));
    fireEvent.mouseDown(document.body);
    expect(screen.queryByTestId('p3')).toBeNull();
  });
});
```

- [ ] **步骤 1.2：运行验证失败**

运行：`cd app && pnpm vitest run src/components/__tests__/Popover.test.tsx`
预期：FAIL（模块不存在）。

- [ ] **步骤 1.3：实现**

```tsx
import { useEffect, useRef, useState, type ReactNode } from 'react';

interface Props {
  /** 触发器：注入 isOpen 与 toggle */
  trigger: (o: { isOpen: boolean; toggle: () => void }) => ReactNode;
  /** 面板内容：注入 close() 供动作按钮收起 */
  panel: (close: () => void) => ReactNode;
  /** 面板容器 testid（断言用） */
  panelTestId: string;
  /** 弹层水平对齐（默认右对齐——顶栏右侧控件） */
  align?: 'left' | 'right';
  /** 面板宽度等 class 覆写（默认 w-64） */
  panelClassName?: string;
}

/** 轻量弹出层（ui_design_0913 §四）：Esc / 点击外部关闭；无依赖、受控于内部 state */
export default function Popover({ trigger, panel, panelTestId, align = 'right', panelClassName = 'w-64' }: Props) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  return (
    <div className="relative" ref={rootRef}>
      {trigger({ isOpen: open, toggle: () => setOpen((v) => !v) })}
      {open ? (
        <div
          role="dialog"
          data-testid={panelTestId}
          className={`absolute top-full z-40 mt-1.5 ${panelClassName} rounded-lg border border-slate-200 bg-white p-3 text-xs text-slate-600 shadow-lg ${align === 'right' ? 'right-0' : 'left-0'}`}
        >
          {panel(() => setOpen(false))}
        </div>
      ) : null}
    </div>
  );
}
```

- [ ] **步骤 1.4：运行验证通过**

运行：`cd app && pnpm vitest run src/components/__tests__/Popover.test.tsx`
预期：3 PASS。

- [ ] **步骤 1.5：Commit**

```bash
git add app/src/components/Popover.tsx app/src/components/__tests__/Popover.test.tsx
git commit -m "feat(ui): Popover 原语——Esc/外部点击关闭的轻量弹出层（slim toolbar T1 前置）"
```

---

### 任务 2：StatusChip（状态单一出口）

**文件：**
- 创建：`app/src/components/toolbar/StatusChip.tsx`
- 测试：`app/src/components/__tests__/StatusChip.test.tsx`

- [ ] **步骤 2.1：编写失败的测试**

```tsx
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import StatusChip from '../../toolbar/StatusChip';

const base = {
  hasDoc: true,
  parseLabel: null,
  translating: false,
  paused: false,
  done: false,
  stats: { done: 0, cached: 0, errors: 0 },
  current: 0,
  total: 0,
  onStop: vi.fn(),
  onContinue: vi.fn(),
  onRetryFailed: vi.fn(),
  onRetranslate: vi.fn(),
};

afterEach(cleanup);

function chip(p: Partial<typeof base> = {}) {
  const props = { ...base, ...p };
  render(<StatusChip {...props} />);
  return props;
}

describe('StatusChip（状态单一出口）', () => {
  it('空闲且无文档 → 不渲染', () => {
    const { container } = render(<StatusChip {...base} hasDoc={false} />);
    expect(container.firstChild).toBeNull();
  });

  it('翻译中：⟳ current/total；Popover 有 [⏹ 停止]', () => {
    chip({ translating: true, current: 32, total: 46 });
    const el = screen.getByTestId('status-chip');
    expect(el.dataset.state).toBe('translating');
    expect(el.textContent).toContain('32/46');
    fireEvent.click(el);
    expect(screen.getByTestId('status-popover')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('status-stop-btn'));
    expect(base.onStop).toHaveBeenCalledOnce();
  });

  it('解析中：⟳ 解析中 + title 带完整解析文案；Popover 显示解析文案', () => {
    chip({ hasDoc: false, parseLabel: '云端解析 1/2 文件…' });
    const el = screen.getByTestId('status-chip');
    expect(el.dataset.state).toBe('parsing');
    expect(el.textContent).toContain('解析中');
    expect(el.getAttribute('title')).toContain('云端解析 1/2');
    fireEvent.click(el);
    expect(screen.getByTestId('status-popover').textContent).toContain('云端解析 1/2');
  });

  it('完成无失败：✓；Popover 有 [↻ 重新翻译全文] 无重试失败', () => {
    chip({ done: true, stats: { done: 46, cached: 0, errors: 0 } });
    expect(screen.getByTestId('status-chip').dataset.state).toBe('done');
    fireEvent.click(screen.getByTestId('status-chip'));
    expect(screen.getByTestId('status-popover').textContent).toContain('完成 46');
    expect(screen.getByTestId('status-popover').textContent).not.toContain('缓存'); // 缓存=0 隐藏
    expect(screen.getByTestId('status-retranslate-btn')).toBeInTheDocument();
    expect(screen.queryByTestId('status-retry-failed-btn')).toBeNull();
  });

  it('完成有失败：⚠ N 段；Popover 显示失败计数 + [重试失败段]', () => {
    chip({ done: true, stats: { done: 45, cached: 1, errors: 1 } });
    const el = screen.getByTestId('status-chip');
    expect(el.dataset.state).toBe('warning');
    expect(el.textContent).toContain('1');
    fireEvent.click(el);
    const text = screen.getByTestId('status-popover').textContent ?? '';
    expect(text).toContain('完成 45');
    expect(text).toContain('缓存 1'); // >0 才显示
    expect(text).toContain('失败 1');
    fireEvent.click(screen.getByTestId('status-retry-failed-btn'));
    expect(base.onRetryFailed).toHaveBeenCalledOnce();
  });

  it('暂停态：⏸ 已暂停 + [▶ 继续翻译]', () => {
    chip({ paused: true });
    expect(screen.getByTestId('status-chip').dataset.state).toBe('paused');
    fireEvent.click(screen.getByTestId('status-chip'));
    fireEvent.click(screen.getByTestId('status-continue-btn'));
    expect(base.onContinue).toHaveBeenCalledOnce();
  });

  it('动作点击后面板收起', () => {
    chip({ done: true });
    fireEvent.click(screen.getByTestId('status-chip'));
    fireEvent.click(screen.getByTestId('status-retranslate-btn'));
    expect(screen.queryByTestId('status-popover')).toBeNull();
    expect(base.onRetranslate).toHaveBeenCalledOnce();
  });
});
```

- [ ] **步骤 2.2：运行验证失败**

运行：`cd app && pnpm vitest run src/components/__tests__/StatusChip.test.tsx`
预期：FAIL（模块不存在）。

- [ ] **步骤 2.3：实现**

```tsx
import Popover from '../Popover';

interface Props {
  hasDoc: boolean;
  /** 解析阶段完整文案（progressLabel 产物；null = 非解析中） */
  parseLabel: string | null;
  translating: boolean;
  paused: boolean;
  /** 本轮翻译完成（含部分失败） */
  done: boolean;
  stats: { done: number; cached: number; errors: number };
  current: number;
  total: number;
  onStop: () => void;
  onContinue: () => void;
  onRetryFailed: () => void;
  onRetranslate: () => void;
}

type State = 'parsing' | 'translating' | 'paused' | 'warning' | 'done';

/**
 * 状态单一出口（ui_design_0913 §三③/§四）：
 * Chip 承载 进度/完成/失败/暂停/解析 五态；细节与动作收进 Popover。
 * （原底部信息栏 4 个技术计数 + TranslateControl 的停止/继续/重译全部收编于此）
 */
export default function StatusChip(p: Props) {
  const state: State | null = p.translating
    ? 'translating'
    : p.paused
      ? 'paused'
      : p.done
        ? p.stats.errors > 0 ? 'warning' : 'done'
        : p.parseLabel
          ? 'parsing'
          : null;
  if (!state && !p.hasDoc) return null;
  if (!state) return null; // 有文档但空闲（切换瞬态）不占位

  const label =
    state === 'translating' ? `⟳ ${p.current}/${p.total}` :
    state === 'parsing' ? '⟳ 解析中' :
    state === 'paused' ? '⏸ 已暂停' :
    state === 'warning' ? `⚠ ${p.stats.errors} 段` :
    '✓';

  const tone =
    state === 'done' ? 'text-emerald-600 bg-emerald-50' :
    state === 'warning' || state === 'paused' ? 'text-amber-700 bg-amber-50' :
    'text-blue-600 bg-blue-50';

  const summary = [
    `完成 ${p.stats.done}`,
    ...(p.stats.cached > 0 ? [`缓存 ${p.stats.cached}`] : []),
    ...(p.stats.errors > 0 ? [`失败 ${p.stats.errors}`] : []),
  ].join(' · ');

  return (
    <Popover
      panelTestId="status-popover"
      panelClassName="w-60"
      trigger={({ toggle }) => (
        <button
          onClick={toggle}
          data-testid="status-chip"
          data-state={state}
          title={state === 'parsing' ? (p.parseLabel ?? undefined) : `翻译状态：${summary}`}
          className={`flex h-6 items-center gap-1 rounded-full px-2 text-[11px] font-medium tabular-nums hover:brightness-95 ${tone}`}
        >
          {label}
        </button>
      )}
      panel={(close) => (
        <div>
          <p className="mb-1 text-[11px] font-semibold text-slate-400">翻译状态</p>
          {state === 'parsing' ? (
            <p className="text-xs text-slate-600">{p.parseLabel}</p>
          ) : (
            <>
              <p className="mb-2 text-xs text-slate-700" data-testid="status-summary">{summary}</p>
              <div className="flex flex-wrap gap-1.5 border-t border-slate-100 pt-2">
                {state === 'translating' ? (
                  <button
                    onClick={() => { p.onStop(); close(); }}
                    data-testid="status-stop-btn"
                    className="rounded border border-red-200 bg-red-50 px-2 py-1 font-medium text-red-600 hover:bg-red-100"
                  >
                    ⏹ 停止翻译
                  </button>
                ) : null}
                {state === 'paused' ? (
                  <button
                    onClick={() => { p.onContinue(); close(); }}
                    data-testid="status-continue-btn"
                    className="rounded bg-blue-600 px-2 py-1 font-medium text-white hover:bg-blue-500"
                  >
                    ▶ 继续翻译
                  </button>
                ) : null}
                {state !== 'translating' && p.stats.errors > 0 ? (
                  <button
                    onClick={() => { p.onRetryFailed(); close(); }}
                    data-testid="status-retry-failed-btn"
                    className="rounded border border-amber-300 bg-amber-50 px-2 py-1 font-medium text-amber-700 hover:bg-amber-100"
                  >
                    ↻ 重试失败段
                  </button>
                ) : null}
                {state !== 'translating' && p.hasDoc ? (
                  <button
                    onClick={() => { p.onRetranslate(); close(); }}
                    data-testid="status-retranslate-btn"
                    className="rounded border border-slate-300 px-2 py-1 font-medium text-slate-600 hover:bg-slate-100"
                  >
                    ↻ 重新翻译全文
                  </button>
                ) : null}
              </div>
            </>
          )}
        </div>
      )}
    />
  );
}
```

- [ ] **步骤 2.4：运行验证通过**

运行：`cd app && pnpm vitest run src/components/__tests__/StatusChip.test.tsx`
预期：7 PASS。

- [ ] **步骤 2.5：Commit**

```bash
git add app/src/components/toolbar/StatusChip.tsx app/src/components/__tests__/StatusChip.test.tsx
git commit -m "feat(ui): StatusChip——状态单一出口（五态 Chip + 计数/停止/继续/重试失败/重译 Popover）"
```

---

### 任务 3：SettingsPopover（低频控件收纳）

**文件：**
- 创建：`app/src/components/toolbar/SettingsPopover.tsx`
- 测试：`app/src/components/__tests__/SettingsPopover.test.tsx`

- [ ] **步骤 3.1：编写失败的测试**

```tsx
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import SettingsPopover from '../../toolbar/SettingsPopover';

afterEach(cleanup);

const base = {
  fontSize: 18,
  onFontIncrease: vi.fn(),
  onFontDecrease: vi.fn(),
  sourceLang: 'en',
  targetLang: 'zh',
  onSourceLangChange: vi.fn(),
  onTargetLangChange: vi.fn(),
  devPath: '',
  onDevPathChange: vi.fn(),
  onParseLocal: vi.fn(),
};

function open() {
  render(<SettingsPopover {...base} />);
  fireEvent.click(screen.getByTestId('settings-btn'));
}

describe('SettingsPopover（字号/语言对/开发路径收纳）', () => {
  it('点击 ⚙ 展开面板：字号控件 + 语言对 + 提示', () => {
    open();
    expect(screen.getByTestId('settings-popover')).toBeInTheDocument();
    expect(screen.getByTestId('font-size-control')).toBeInTheDocument();
    expect(screen.getByTestId('font-size-value').textContent).toBe('18');
    expect(screen.getByLabelText('源语言')).toBeInTheDocument();
    expect(screen.getByLabelText('目标语言')).toBeInTheDocument();
    expect(screen.getByTestId('settings-popover').textContent).toContain('更改语言对');
  });

  it('字号增减回调', () => {
    open();
    fireEvent.click(screen.getByTestId('font-size-inc'));
    expect(base.onFontIncrease).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByTestId('font-size-dec'));
    expect(base.onFontDecrease).toHaveBeenCalledOnce();
  });

  it('语言对变更回调', () => {
    open();
    fireEvent.change(screen.getByLabelText('源语言'), { target: { value: 'fr' } });
    expect(base.onSourceLangChange).toHaveBeenCalledWith('fr');
    fireEvent.change(screen.getByLabelText('目标语言'), { target: { value: 'ja' } });
    expect(base.onTargetLangChange).toHaveBeenCalledWith('ja');
  });

  it('开发小节：本地路径输入 Enter 触发 onParseLocal', () => {
    open();
    const input = screen.getByTestId('file-path-input');
    fireEvent.change(input, { target: { value: '/tmp/a.pdf' } });
    expect(base.onDevPathChange).toHaveBeenCalledWith('/tmp/a.pdf');
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(base.onParseLocal).toHaveBeenCalledOnce();
  });
});
```

- [ ] **步骤 3.2：运行验证失败**

运行：`cd app && pnpm vitest run src/components/__tests__/SettingsPopover.test.tsx`
预期：FAIL（模块不存在）。

- [ ] **步骤 3.3：实现**（`LANGS` 与字号控件从旧 Toolbar 迁入）

```tsx
import Popover from '../Popover';

const LANGS = [
  { code: 'en', label: '英语' },
  { code: 'zh', label: '中文' },
  { code: 'ja', label: '日语' },
  { code: 'fr', label: '法语' },
  { code: 'de', label: '德语' },
];

interface Props {
  fontSize: number;
  onFontIncrease: () => void;
  onFontDecrease: () => void;
  sourceLang: string;
  targetLang: string;
  onSourceLangChange: (v: string) => void;
  onTargetLangChange: (v: string) => void;
  /** 开发分支：服务器本地路径（Enter 触发零点击流水线；E2E/单测共用） */
  devPath: string;
  onDevPathChange: (v: string) => void;
  onParseLocal: () => void;
}

/** ⚙ 设置 Popover（ui_design_0913 §四）：字号与语言对从顶栏下架，会话内一次设置即可 */
export default function SettingsPopover(p: Props) {
  return (
    <Popover
      panelTestId="settings-popover"
      panelClassName="w-72"
      trigger={({ isOpen, toggle }) => (
        <button
          onClick={toggle}
          data-testid="settings-btn"
          title="设置（字号 / 语言对）"
          className={'flex h-8 w-8 items-center justify-center rounded-lg text-sm text-slate-500 hover:bg-slate-100 hover:text-slate-700 ' + (isOpen ? 'bg-slate-100 text-slate-700' : '')}
        >
          ⚙
        </button>
      )}
      panel={() => (
        <div className="space-y-3">
          <section>
            <p className="mb-1.5 text-[11px] font-semibold text-slate-400">显示 · 字号</p>
            <div className="flex items-center gap-2" data-testid="font-size-control">
              <button onClick={p.onFontDecrease} data-testid="font-size-dec" aria-label="减小字号" className="h-7 w-7 rounded border border-slate-300 hover:bg-slate-100">A−</button>
              <span className="w-8 text-center text-xs tabular-nums text-slate-500" data-testid="font-size-value">{p.fontSize}</span>
              <button onClick={p.onFontIncrease} data-testid="font-size-inc" aria-label="增大字号" className="h-7 w-7 rounded border border-slate-300 hover:bg-slate-100">A＋</button>
            </div>
          </section>
          <section>
            <p className="mb-1.5 text-[11px] font-semibold text-slate-400">翻译 · 语言对</p>
            <div className="flex items-center gap-1.5">
              <select value={p.sourceLang} onChange={(e) => p.onSourceLangChange(e.target.value)} aria-label="源语言" className="h-7 flex-1 rounded border border-slate-300 px-1">
                {LANGS.map((l) => <option key={l.code} value={l.code}>{l.label}</option>)}
              </select>
              <span className="text-slate-400">→</span>
              <select value={p.targetLang} onChange={(e) => p.onTargetLangChange(e.target.value)} aria-label="目标语言" className="h-7 flex-1 rounded border border-slate-300 px-1">
                {LANGS.map((l) => <option key={l.code} value={l.code}>{l.label}</option>)}
              </select>
            </div>
            <p className="mt-1.5 text-[11px] text-slate-400">ⓘ 更改语言对后需重新翻译全文生效</p>
          </section>
          <section className="border-t border-slate-100 pt-2">
            <p className="mb-1.5 text-[11px] font-semibold text-slate-400">开发 · 服务器本地路径</p>
            <input
              type="text"
              value={p.devPath}
              onChange={(e) => p.onDevPathChange(e.target.value)}
              onKeyDown={(e) => { if (e.key === 'Enter' && p.devPath.trim()) p.onParseLocal(); }}
              placeholder="/path/to/file.pdf 回车"
              data-testid="file-path-input"
              className="h-7 w-full rounded border border-slate-300 px-2 text-xs focus:border-blue-400 focus:outline-none"
            />
          </section>
        </div>
      )}
    />
  );
}
```

- [ ] **步骤 3.4：运行验证通过**

运行：`cd app && pnpm vitest run src/components/__tests__/SettingsPopover.test.tsx`
预期：4 PASS。

- [ ] **步骤 3.5：Commit**

```bash
git add app/src/components/toolbar/SettingsPopover.tsx app/src/components/__tests__/SettingsPopover.test.tsx
git commit -m "feat(ui): SettingsPopover——字号/语言对/开发路径收进 ⚙ 弹层（顶栏下架低频控件）"
```

---

### 任务 4：QuotaChip（配额收敛）

**文件：**
- 创建：`app/src/components/toolbar/QuotaChip.tsx`
- 测试：`app/src/components/__tests__/QuotaChip.test.tsx`

- [ ] **步骤 4.1：编写失败的测试**（改写自 QuotaIndicator.test.tsx，保留 `quota-indicator` testid 语义）

```tsx
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import QuotaChip from '../../toolbar/QuotaChip';
import * as backend from '../../services/backend';

vi.mock('../../services/backend', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../services/backend')>()),
  fetchQuota: vi.fn(),
}));

const onLoginClick = vi.fn();
const onViewUsage = vi.fn();

afterEach(() => {
  vi.clearAllMocks();
  cleanup();
});

describe('QuotaChip（五碎片收敛为一个 Chip + Popover）', () => {
  it('游客：Chip 显示 ◔ N页；Popover 有剩余/上限、24h 清理提示、登录 CTA、翻译记录入口', async () => {
    vi.mocked(backend.fetchQuota).mockResolvedValue({ anonymous: true, unlimited: false, limit: 5, used: 2, remaining: 3 });
    render(<QuotaChip onLoginClick={onLoginClick} onViewUsage={onViewUsage} />);
    await waitFor(() => expect(screen.getByTestId('quota-indicator')).toBeInTheDocument());
    expect(screen.getByTestId('quota-indicator').textContent).toContain('3');
    fireEvent.click(screen.getByTestId('quota-indicator'));
    const pop = screen.getByTestId('quota-popover');
    expect(pop.textContent).toContain('游客额度');
    expect(pop.textContent).toContain('剩余 3 页');
    expect(pop.textContent).toContain('5 页');
    expect(pop.textContent).toContain('24h');
    fireEvent.click(screen.getByTestId('quota-popover-login-btn'));
    expect(onLoginClick).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByTestId('quota-indicator'));
    fireEvent.click(screen.getByTestId('quota-popover-usage-btn'));
    expect(onViewUsage).toHaveBeenCalledOnce();
  });

  it('登录用户（1000 页/24h）：显示剩余；无登录 CTA、无清理提示、无记录入口', async () => {
    vi.mocked(backend.fetchQuota).mockResolvedValue({ anonymous: false, unlimited: false, limit: 1000, used: 120, remaining: 880 });
    render(<QuotaChip onLoginClick={onLoginClick} />);
    await waitFor(() => expect(screen.getByTestId('quota-indicator')).toBeInTheDocument());
    expect(screen.getByTestId('quota-indicator').textContent).toContain('880');
    fireEvent.click(screen.getByTestId('quota-indicator'));
    const pop = screen.getByTestId('quota-popover');
    expect(pop.textContent).toContain('880');
    expect(pop.textContent).toContain('1000');
    expect(screen.queryByTestId('quota-popover-login-btn')).toBeNull();
    expect(screen.queryByTestId('quota-popover-usage-btn')).toBeNull();
    expect(pop.textContent).not.toContain('自动清理');
  });

  it('不限量（本地直连）：不渲染', async () => {
    vi.mocked(backend.fetchQuota).mockResolvedValue({ anonymous: true, unlimited: true, remaining: null });
    const { container } = render(<QuotaChip onLoginClick={onLoginClick} />);
    await waitFor(() => expect(backend.fetchQuota).toHaveBeenCalled());
    expect(container.firstChild).toBeNull();
  });

  it('查询失败：静默隐藏', async () => {
    vi.mocked(backend.fetchQuota).mockRejectedValue(new Error('offline'));
    const { container } = render(<QuotaChip onLoginClick={onLoginClick} />);
    await waitFor(() => expect(backend.fetchQuota).toHaveBeenCalled());
    expect(container.firstChild).toBeNull();
  });
});
```

- [ ] **步骤 4.2：运行验证失败**

运行：`cd app && pnpm vitest run src/components/__tests__/QuotaChip.test.tsx`
预期：FAIL（模块不存在）。

- [ ] **步骤 4.3：实现**

```tsx
import { useCallback, useEffect, useState } from 'react';
import Popover from '../Popover';
import { fetchQuota, type QuotaStatus } from '../../services/backend';

interface Props {
  onLoginClick: () => void;
  /** 游客：查看翻译记录（额度去向） */
  onViewUsage?: () => void;
}

/**
 * 配额 Chip ◔ N页（ui_design_0913 §三⑥/§四）：
 * 顶栏五个碎片文字（游客模式/免登录/剩余/24h/去向）收敛为一个 Chip；
 * unlimited（本地直连）/查询失败 → 整体隐藏。
 */
export default function QuotaChip({ onLoginClick, onViewUsage }: Props) {
  const [quota, setQuota] = useState<QuotaStatus | null>(null);

  const refresh = useCallback(() => {
    fetchQuota()
      .then((q) => setQuota(q.unlimited || q.remaining == null ? null : q))
      .catch(() => setQuota(null));
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  if (!quota) return null;
  const who = quota.anonymous ? '游客额度' : '每日额度';
  return (
    <Popover
      panelTestId="quota-popover"
      panelClassName="w-60"
      trigger={({ toggle }) => (
        <button
          onClick={toggle}
          data-testid="quota-indicator"
          title={`${who}：已用 ${quota.used ?? 0} / ${quota.limit ?? 0} 页（每 24 小时滚动恢复）`}
          className="flex h-6 items-center gap-1 rounded-full bg-slate-100 px-2 text-[11px] font-medium tabular-nums text-slate-500 hover:bg-slate-200"
        >
          ◔ {quota.remaining}页
        </button>
      )}
      panel={(close) => (
        <div>
          <p className="mb-1 text-[11px] font-semibold text-slate-400">{who}</p>
          <p className="text-xs text-slate-700">
            剩余 {quota.remaining} 页 / 每 24h {quota.limit ?? 0} 页
          </p>
          <p className="mt-0.5 text-[11px] text-slate-400">已用 {quota.used ?? 0} 页 · 滚动恢复</p>
          {quota.anonymous ? <p className="mt-0.5 text-[11px] text-slate-400">匿名文档 24h 后自动清理</p> : null}
          <div className="mt-2 flex flex-col gap-1.5 border-t border-slate-100 pt-2">
            {quota.anonymous && onViewUsage ? (
              <button
                onClick={() => { onViewUsage(); close(); }}
                data-testid="quota-popover-usage-btn"
                className="rounded border border-slate-300 px-2 py-1 font-medium text-slate-600 hover:bg-slate-100"
              >
                🕘 查看翻译记录
              </button>
            ) : null}
            {quota.anonymous ? (
              <button
                onClick={() => { onLoginClick(); close(); }}
                data-testid="quota-popover-login-btn"
                className="rounded bg-blue-600 px-2 py-1 font-medium text-white hover:bg-blue-500"
              >
                登录解锁每日 1000 页
              </button>
            ) : null}
          </div>
        </div>
      )}
    />
  );
}
```

- [ ] **步骤 4.4：运行验证通过**

运行：`cd app && pnpm vitest run src/components/__tests__/QuotaChip.test.tsx`
预期：4 PASS。

- [ ] **步骤 4.5：Commit**

```bash
git add app/src/components/toolbar/QuotaChip.tsx app/src/components/__tests__/QuotaChip.test.tsx
git commit -m "feat(ui): QuotaChip——配额五碎片收敛为一个 ◔ Chip + Popover（登录 CTA/记录入口）"
```

---

### 任务 5：ToolbarSlim 组装（48px 单行七区 + 2px 进度条）

**文件：**
- 创建：`app/src/components/toolbar/ToolbarSlim.tsx`
- 测试：`app/src/components/__tests__/ToolbarSlim.test.tsx`

- [ ] **步骤 5.1：编写失败的测试**

```tsx
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import ToolbarSlim, { progressLabel } from '../../toolbar/ToolbarSlim';
import type { ParseProgress } from '../../types';

vi.mock('../../services/backend', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../services/backend')>()),
  fetchQuota: vi.fn().mockResolvedValue({ anonymous: true, unlimited: true, remaining: null }),
}));

afterEach(cleanup);

const base = {
  docTitle: null as string | null,
  fileType: null as 'docx' | 'pdf' | null,
  onPickFile: vi.fn(),
  parseLabel: null as string | null,
  translating: false,
  paused: false,
  done: false,
  stats: { done: 0, cached: 0, errors: 0 },
  progressCurrent: 0,
  progressTotal: 0,
  onStopTranslate: vi.fn(),
  onContinueTranslate: vi.fn(),
  onRetryFailed: vi.fn(),
  onRetranslate: vi.fn(),
  renderMode: 'original' as const,
  onRenderModeChange: vi.fn(),
  canToggleRenderMode: false,
  fontSize: 18,
  onFontIncrease: vi.fn(),
  onFontDecrease: vi.fn(),
  sourceLang: 'en',
  targetLang: 'zh',
  onSourceLangChange: vi.fn(),
  onTargetLangChange: vi.fn(),
  devPath: '',
  onDevPathChange: vi.fn(),
  onParseLocal: vi.fn(),
  username: null as string | null,
  onLoginClick: vi.fn(),
  onLogoutClick: vi.fn(),
};

describe('ToolbarSlim（48px 单行七区）', () => {
  it('默认渲染：Logo/wordmark、＋打开主按钮、⚙、登录；无哈希 ID/游客模式碎片/字号/语言对常驻', () => {
    render(<ToolbarSlim {...base} />);
    expect(screen.getByTestId('upload-btn')).toBeInTheDocument();
    expect(screen.getByTestId('upload-input')).toBeInTheDocument();
    expect(screen.getByTestId('settings-btn')).toBeInTheDocument();
    expect(screen.getByTestId('login-btn')).toBeInTheDocument();
    // 收纳验证：常驻区不再直接出现语言对/字号控件
    expect(screen.queryByTestId('font-size-control')).toBeNull();
    expect(screen.queryByLabelText('源语言')).toBeNull();
    expect(screen.queryByTestId('anon-hint')).toBeNull();
  });

  it('文档打开：文档名 + 类型徽章；分段控件按需出现', () => {
    render(<ToolbarSlim {...base} docTitle="01_ViT_Original_ViT.pdf" fileType="pdf" canToggleRenderMode />);
    expect(screen.getByText('01_ViT_Original_ViT.pdf')).toBeInTheDocument();
    expect(screen.getByTestId('file-type-badge').textContent).toBe('PDF');
    expect(screen.getByTestId('render-mode-toggle')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('mode-text-btn'));
    expect(base.onRenderModeChange).toHaveBeenCalledWith('text');
  });

  it('无 pdf_meta：分段控件不出现', () => {
    render(<ToolbarSlim {...base} canToggleRenderMode={false} />);
    expect(screen.queryByTestId('render-mode-toggle')).toBeNull();
  });

  it('翻译中：顶栏下缘 2px 进度条按比例；未翻译时隐藏', () => {
    const { rerender } = render(<ToolbarSlim {...base} translating progressCurrent={23} progressTotal={46} />);
    const bar = screen.getByTestId('top-progress-bar');
    expect(bar.style.width).toBe('50%');
    rerender(<ToolbarSlim {...base} translating={false} progressCurrent={46} progressTotal={46} />);
    expect(screen.queryByTestId('top-progress-bar')).toBeNull();
  });

  it('登录态：用户名 + 历史 + 退出 + 配额 Chip 挂载点', async () => {
    render(<ToolbarSlim {...base} username="alice" onHistoryClick={vi.fn()} />);
    expect(screen.getByTestId('auth-username').textContent).toContain('alice');
    expect(screen.getByTestId('history-btn')).toBeInTheDocument();
    expect(screen.getByTestId('logout-btn')).toBeInTheDocument();
  });

  it('progressLabel：各解析阶段文案', () => {
    const p = (stage: ParseProgress['stage']) => ({ stage }) as ParseProgress;
    expect(progressLabel(p('merging'))).toContain('合并');
    expect(progressLabel(p('fallback'))).toContain('回退');
    expect(progressLabel(p('cached'))).toContain('缓存');
  });
});
```

- [ ] **步骤 5.2：运行验证失败**

运行：`cd app && pnpm vitest run src/components/__tests__/ToolbarSlim.test.tsx`
预期：FAIL（模块不存在）。

- [ ] **步骤 5.3：实现**

```tsx
import { useRef } from 'react';
import type { ParseProgress, RenderMode } from '../../types';
import StatusChip from './StatusChip';
import SettingsPopover from './SettingsPopover';
import QuotaChip from './QuotaChip';

/** MinerU 解析进度文案（自旧 Toolbar 迁入，语义不变） */
export function progressLabel(p: ParseProgress): string {
  switch (p.stage) {
    case 'splitting':
      return `拆分为 ${p.chunks} 个片段…`;
    case 'uploading':
      return `上传片段 ${p.current ?? 0}/${p.total ?? '?'}…`;
    case 'parsing':
      return `云端解析 ${p.done_files ?? 0}/${p.total_files ?? '?'} 文件${
        p.pages_total ? `（${p.pages_done}/${p.pages_total} 页）` : ''
      }…`;
    case 'merging':
      return '合并解析结果…';
    case 'fallback':
      return '已回退内置解析';
    case 'cached':
      return '解析缓存命中';
  }
}

interface Props {
  docTitle: string | null;
  fileType: 'docx' | 'pdf' | null;
  onPickFile: (file: File) => void;
  // ③ 状态
  parseLabel: string | null;
  translating: boolean;
  paused: boolean;
  done: boolean;
  stats: { done: number; cached: number; errors: number };
  progressCurrent: number;
  progressTotal: number;
  onStopTranslate: () => void;
  onContinueTranslate: () => void;
  onRetryFailed: () => void;
  onRetranslate: () => void;
  // ④ 视图
  renderMode: RenderMode;
  onRenderModeChange: (m: RenderMode) => void;
  canToggleRenderMode: boolean;
  // ⑤ 设置
  fontSize: number;
  onFontIncrease: () => void;
  onFontDecrease: () => void;
  sourceLang: string;
  targetLang: string;
  onSourceLangChange: (v: string) => void;
  onTargetLangChange: (v: string) => void;
  devPath: string;
  onDevPathChange: (v: string) => void;
  onParseLocal: () => void;
  // ⑥⑦ 配额/账户
  username: string | null;
  onLoginClick: () => void;
  onLogoutClick: () => void;
  onHistoryClick?: () => void;
  onViewUsage?: () => void;
}

/**
 * 单行工具栏（ui_design_0913 §三）：48px 高、三区分治（身份/文档 ｜ 主操作/状态 ｜ 视图/账户），
 * 区际留白分隔；翻译进行时下缘 2px 主色进度条。
 */
export default function ToolbarSlim(p: Props) {
  const fileInputRef = useRef<HTMLInputElement>(null);
  const pct =
    p.translating && p.progressTotal > 0
      ? Math.round((p.progressCurrent / p.progressTotal) * 100)
      : null;

  return (
    <header className="relative flex h-12 shrink-0 items-center gap-3 border-b border-slate-200 bg-white px-3">
      {/* ① 身份/文档 */}
      <span className="shrink-0 text-base leading-none" title="geminus">🦊</span>
      {p.docTitle ? (
        <span className="min-w-0 max-w-[16rem] truncate text-sm text-slate-700" title={p.docTitle}>
          {p.docTitle}
          {p.fileType ? (
            <span
              data-testid="file-type-badge"
              className="ml-1.5 rounded bg-slate-100 px-1 py-0.5 align-middle text-[10px] font-bold tracking-wide text-slate-500"
            >
              {p.fileType.toUpperCase()}
            </span>
          ) : null}
        </span>
      ) : (
        <span className="text-sm font-semibold text-slate-800">geminus</span>
      )}

      {/* ② 主操作：唯一实心主色按钮 */}
      <input
        type="file"
        accept=".docx,.pdf"
        ref={fileInputRef}
        className="hidden"
        data-testid="upload-input"
        onChange={(e) => {
          const f = e.target.files?.[0];
          if (f) p.onPickFile(f);
          e.target.value = ''; // 允许重复选择同一文件
        }}
      />
      <button
        onClick={() => fileInputRef.current?.click()}
        data-testid="upload-btn"
        title="打开 .docx / .pdf 文件，自动解析并翻译"
        className="h-8 shrink-0 rounded-lg bg-blue-600 px-3 text-xs font-medium text-white hover:bg-blue-500"
      >
        ＋ 打开
      </button>

      {/* ③ 状态 */}
      <StatusChip
        hasDoc={!!p.docTitle}
        parseLabel={p.parseLabel}
        translating={p.translating}
        paused={p.paused}
        done={p.done}
        stats={p.stats}
        current={p.progressCurrent}
        total={p.progressTotal}
        onStop={p.onStopTranslate}
        onContinue={p.onContinueTranslate}
        onRetryFailed={p.onRetryFailed}
        onRetranslate={p.onRetranslate}
      />

      <div className="flex-1" />

      {/* ④ 视图：分段控件（原始│对照） */}
      {p.canToggleRenderMode ? (
        <div className="flex h-8 shrink-0 items-center rounded-lg bg-slate-100 p-0.5 text-xs" data-testid="render-mode-toggle">
          <button
            onClick={() => p.onRenderModeChange('original')}
            data-testid="mode-original-btn"
            className={'h-7 rounded-md px-2.5 ' + (p.renderMode === 'original' ? 'bg-white font-medium text-blue-700 shadow-sm' : 'text-slate-500 hover:text-slate-700')}
          >
            原始
          </button>
          <button
            onClick={() => p.onRenderModeChange('text')}
            data-testid="mode-text-btn"
            className={'h-7 rounded-md px-2.5 ' + (p.renderMode === 'text' ? 'bg-white font-medium text-blue-700 shadow-sm' : 'text-slate-500 hover:text-slate-700')}
          >
            对照
          </button>
        </div>
      ) : null}

      {/* ⑤ 设置 */}
      <SettingsPopover
        fontSize={p.fontSize}
        onFontIncrease={p.onFontIncrease}
        onFontDecrease={p.onFontDecrease}
        sourceLang={p.sourceLang}
        targetLang={p.targetLang}
        onSourceLangChange={p.onSourceLangChange}
        onTargetLangChange={p.onTargetLangChange}
        devPath={p.devPath}
        onDevPathChange={p.onDevPathChange}
        onParseLocal={p.onParseLocal}
      />

      {/* ⑥ 配额 */}
      <QuotaChip onLoginClick={p.onLoginClick} onViewUsage={p.onViewUsage} />

      {/* ⑦ 账户 */}
      {p.username ? (
        <div className="flex shrink-0 items-center gap-1" data-testid="auth-area">
          <span
            className="max-w-[10rem] truncate rounded-full bg-emerald-50 px-2.5 py-1 text-xs text-emerald-700"
            title={p.username}
            data-testid="auth-username"
          >
            👤 {p.username}
          </span>
          <button
            onClick={p.onHistoryClick}
            className="flex h-8 w-8 items-center justify-center rounded-lg text-sm text-slate-500 hover:bg-slate-100 hover:text-slate-700"
            data-testid="history-btn"
            title="我的文档"
          >
            🕘
          </button>
          <button
            onClick={p.onLogoutClick}
            className="rounded-lg px-2 py-1 text-xs text-slate-500 hover:bg-slate-100 hover:text-slate-700"
            data-testid="logout-btn"
          >
            退出
          </button>
        </div>
      ) : (
        <div className="flex shrink-0 items-center gap-1" data-testid="auth-area">
          <button
            onClick={p.onLoginClick}
            className="rounded-lg border border-blue-300 bg-blue-50 px-2.5 py-1.5 text-xs font-medium text-blue-700 hover:bg-blue-100"
            data-testid="login-btn"
          >
            登录
          </button>
        </div>
      )}

      {/* 翻译进行时：顶栏下缘 2px 进度条（完成即隐藏） */}
      {pct != null ? (
        <div
          data-testid="top-progress-bar"
          className="absolute inset-y-auto bottom-0 left-0 h-0.5 bg-blue-600 transition-all duration-300"
          style={{ width: `${pct}%` }}
        />
      ) : null}
    </header>
  );
}
```

- [ ] **步骤 5.4：运行验证通过**

运行：`cd app && pnpm vitest run src/components/__tests__/ToolbarSlim.test.tsx`
预期：6 PASS。

- [ ] **步骤 5.5：Commit**

```bash
git add app/src/components/toolbar/ToolbarSlim.tsx app/src/components/__tests__/ToolbarSlim.test.tsx
git commit -m "feat(ui): ToolbarSlim——48px 单行七区顶栏 + 分段控件 + 2px 顶栏下缘进度条（T1）"
```

---

### 任务 6：Toast（瞬时错误出口）

**文件：**
- 创建：`app/src/components/Toast.tsx`
- 测试：`app/src/components/__tests__/Toast.test.tsx`

- [ ] **步骤 6.1：编写失败的测试**

```tsx
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, act } from '@testing-library/react';
import Toast from '../Toast';

beforeEach(() => vi.useFakeTimers());
afterEach(() => { vi.runOnlyPendingTimers(); vi.useRealTimers(); cleanup(); });

describe('Toast（瞬时错误出口）', () => {
  it('渲染错误文案 + 手动关闭回调', () => {
    const onDismiss = vi.fn();
    render(<Toast message="连接中断" onDismiss={onDismiss} />);
    expect(screen.getByTestId('toast').textContent).toContain('连接中断');
    fireEvent.click(screen.getByTestId('toast-dismiss'));
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it('6s 后自动消失', () => {
    const onDismiss = vi.fn();
    render(<Toast message="x" onDismiss={onDismiss} />);
    act(() => { vi.advanceTimersByTime(6000); });
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it('message 为 null 不渲染', () => {
    const { container } = render(<Toast message={null} onDismiss={() => {}} />);
    expect(container.firstChild).toBeNull();
  });
});
```

- [ ] **步骤 6.2：运行验证失败**

运行：`cd app && pnpm vitest run src/components/__tests__/Toast.test.tsx`
预期：FAIL。

- [ ] **步骤 6.3：实现**

```tsx
import { useEffect } from 'react';

interface Props {
  message: string | null;
  onDismiss: () => void;
  /** 自动消失时长 */
  duration?: number;
}

/** 瞬时错误 Toast（ui_design_0913 §三：底部栏移除后错误信息的出口） */
export default function Toast({ message, onDismiss, duration = 6000 }: Props) {
  useEffect(() => {
    if (message == null) return;
    const t = window.setTimeout(onDismiss, duration);
    return () => window.clearTimeout(t);
  }, [message, duration, onDismiss]);

  if (message == null) return null;
  return (
    <div
      data-testid="toast"
      role="alert"
      className="fixed left-1/2 top-14 z-50 flex max-w-md -translate-x-1/2 items-center gap-2 rounded-lg bg-red-600 px-3 py-2 text-xs font-medium text-white shadow-lg"
    >
      <span className="truncate" title={message}>{message}</span>
      <button onClick={onDismiss} data-testid="toast-dismiss" className="shrink-0 rounded px-1 text-base leading-none hover:bg-red-500" aria-label="关闭">
        ×
      </button>
    </div>
  );
}
```

- [ ] **步骤 6.4：运行验证通过**

运行：`cd app && pnpm vitest run src/components/__tests__/Toast.test.tsx`
预期：3 PASS。

- [ ] **步骤 6.5：Commit**

```bash
git add app/src/components/Toast.tsx app/src/components/__tests__/Toast.test.tsx
git commit -m "feat(ui): Toast——瞬时错误提示（替代底栏 status-error）"
```

---

### 任务 7：App 重接线 + 旧组件删除（T2 落地）

**文件：**
- 修改：`app/src/App.tsx`
- 修改：`app/src/__tests__/App.test.tsx`
- 删除：`app/src/components/Toolbar.tsx`、`StatusBar.tsx`、`TranslateControl.tsx`、`QuotaIndicator.tsx`、`__tests__/TranslateControl.test.tsx`、`__tests__/QuotaIndicator.test.tsx`

- [ ] **步骤 7.1：先改 App.test.tsx（红灯）**

对 `app/src/__tests__/App.test.tsx`：

1. 新增本地 helper（替代直接抓顶栏 input）：
```ts
/** 经 ⚙ 设置弹层打开本地路径输入（开发分支迁入 Popover 后的统一入口） */
function openSettingsPathInput() {
  fireEvent.click(screen.getByTestId('settings-btn'));
  return screen.getByTestId('file-path-input');
}
```
2. 全部 `screen.getByTestId('file-path-input')`（openDoc 与各内联处）改为 `openSettingsPathInput()`。
3. 用例「零点击自动翻译」断言更新：
```ts
    // 状态收敛进 StatusChip（点击展开 Popover 断言计数）
    const chip = screen.getByTestId('status-chip');
    await waitFor(() => expect(chip.dataset.state).toBe('done'));
    fireEvent.click(chip);
    expect(screen.getByTestId('status-popover').textContent).toContain('完成 1');
    expect(screen.getByTestId('status-popover').textContent).toContain('缓存 1');
    expect(translateDoc).toHaveBeenCalledWith('d1', 'en', 'zh', expect.any(Function));
```
（删除 `✅ 完成 1` / `💾 缓存命中 1` / `progress-text` 三行旧断言。）
4. 用例「匿名状态」：删除 `anon-hint` 断言行（碎片文字已收敛）。
5. 用例「翻译取消」：
```ts
    await waitFor(() => expect(screen.getByTestId('status-chip').dataset.state).toBe('paused'));
    fireEvent.click(screen.getByTestId('status-chip'));
    fireEvent.click(screen.getByTestId('status-continue-btn'));
```
（`translate-control`/`translate-continue-btn` 不复存在；继续按钮断言改为存在即可——原 onContinue 由 App 内 runTranslate 承接，不断言回调。）

- [ ] **步骤 7.2：运行验证失败**

运行：`cd app && pnpm vitest run src/__tests__/App.test.tsx`
预期：FAIL（settings-btn 不存在等）。

- [ ] **步骤 7.3：改 App.tsx**

改动点（对照现有 `app/src/App.tsx`）：

1. import 区：删除 `Toolbar`/`StatusBar`/`TranslateControl` 行，新增：
```ts
import ToolbarSlim, { progressLabel } from './components/toolbar/ToolbarSlim';
import Toast from './components/Toast';
import { cancelTranslate, retranslateParagraph } from './services/backend';
```
2. 新增回调（放 `handleRetranslated` 之后）：
```ts
  // slim toolbar：停止翻译（原 TranslateControl 内逻辑上移）
  const handleStopTranslate = useCallback(async () => {
    if (!doc) return;
    try {
      await cancelTranslate(doc.id);
    } catch {
      /* 状态切换由 SSE cancelled 事件驱动 */
    }
  }, [doc]);

  // slim toolbar：仅重译失败回退的段（retranslateParagraph 逐段；区别于整跑 translateDoc）
  const handleRetryFailed = useCallback(async () => {
    if (!doc) return;
    for (const id of fallbackIds) {
      try {
        const pair = await retranslateParagraph(doc.id, id, sourceLang, targetLang);
        const orig = doc.blocks.find((b) => b.id === id);
        handleRetranslated({ blockId: pair.block_id, block: pair.translated ?? orig ?? null, fallback: pair.fallback ?? false });
        setStats((s) => ({ ...s, errors: Math.max(0, s.errors - 1), done: s.done + 1 }));
      } catch {
        /* 失败保留原状，可再次重试 */
      }
    }
  }, [doc, fallbackIds, sourceLang, targetLang, handleRetranslated]);
```
（`TranslatedBlock.translated` 类型为 `Block | null`；`handleRetranslated` 的 `block` 参数当前签名是 `Block`——将 `App.tsx` 中该回调参数类型放宽为 `Block | null` 并在 setTranslatedMap 直接写入即可，与现有 `finalMap: Record<string, Block | null>` 一致。）
3. `<Toolbar …20 个旧 props/>` 整体替换为：
```tsx
      <ToolbarSlim
        docTitle={doc ? filePath || doc.title : null}
        fileType={doc ? fileType : null}
        onPickFile={handlePickFile}
        parseLabel={pipeline.state.parseProgress ? progressLabel(pipeline.state.parseProgress) : null}
        translating={translating}
        paused={paused}
        done={lastDone}
        stats={stats}
        progressCurrent={progress?.current ?? 0}
        progressTotal={progress?.total ?? 0}
        onStopTranslate={handleStopTranslate}
        onContinueTranslate={() => doc && runTranslate(doc)}
        onRetryFailed={handleRetryFailed}
        onRetranslate={() => doc && runTranslate(doc)}
        renderMode={renderMode}
        onRenderModeChange={setRenderMode}
        canToggleRenderMode={!!doc?.pdf_meta && fileType === 'pdf'}
        fontSize={fontSize}
        onFontIncrease={fontIncrease}
        onFontDecrease={fontDecrease}
        sourceLang={sourceLang}
        targetLang={targetLang}
        onSourceLangChange={setSourceLang}
        onTargetLangChange={setTargetLang}
        devPath={filePath}
        onDevPathChange={handleFilePathChange}
        onParseLocal={handleParseLocal}
        username={username}
        onLoginClick={() => openLogin()}
        onLogoutClick={handleLogout}
        onHistoryClick={username ? openHistory : undefined}
        onViewUsage={!username ? () => setGuestDocsOpen(true) : undefined}
      />
```
4. 删除 `<StatusBar … />` 与整个 `{doc ? <TranslateControl …/> : null}` 块。
5. 在 `<ToolbarSlim/>` 之后加错误 Toast：
```tsx
      <Toast message={error} onDismiss={() => setError(null)} />
```
6. `fileType` 状态在 applyDoc 已维护，无需新增。

- [ ] **步骤 7.4：删除旧组件与其测试**

```bash
git rm app/src/components/Toolbar.tsx app/src/components/StatusBar.tsx \
       app/src/components/TranslateControl.tsx app/src/components/QuotaIndicator.tsx \
       app/src/components/__tests__/TranslateControl.test.tsx app/src/components/__tests__/QuotaIndicator.test.tsx
```

- [ ] **步骤 7.5：全量单测 + 类型检查**

运行：`cd app && pnpm test && pnpm build`
预期：全绿 + tsc 无错（如 PdfCompareView.test 引用栏头文案此时应仍绿——Pane 尚未改；若引用旧 Toolbar 则按报错最小修）。

- [ ] **步骤 7.6：Commit**

```bash
git add -A app/src
git commit -m "feat(ui): App 接线 ToolbarSlim/Toast——删除 StatusBar/TranslateControl/旧 Toolbar（T2 底栏移除与状态迁移）"
```

---

### 任务 8：Pane 栏头极简（T3 前半）

**文件：**
- 修改：`app/src/components/Pane.tsx`
- 修改：`app/src/components/DocumentViewer.tsx`、`app/src/components/PdfCompareView.tsx`
- 测试：`app/src/components/__tests__/PdfCompareView.test.tsx`、`DocumentViewer` 相关（如有栏头断言）

- [ ] **步骤 8.1：先改测试（红灯）**——在 `PdfCompareView.test.tsx`（及受影响测试）里，把对栏头（`原文（原始渲染）`、`翻译后`、`file-type-badge` in pane）的断言改为浮标断言：
```ts
expect(screen.getAllByTestId('pane-label').map((e) => e.textContent)).toEqual(['原文', '译文']);
```
（若现有测试无栏头断言，则补一条上述浮标断言进任一现有用例即可。）

- [ ] **步骤 8.2：运行验证失败**

运行：`cd app && pnpm vitest run src/components/__tests__/PdfCompareView.test.tsx`
预期：FAIL（pane-label 不存在）。

- [ ] **步骤 8.3：改 Pane.tsx（整文件替换为）**

```tsx
import type { ReactNode, RefObject } from 'react';

interface Props {
  /** 极淡浮标（左上角悬浮，不占布局高度；ui_design_0913 §三 栏头极简化） */
  label?: string;
  containerRef?: RefObject<HTMLDivElement>;
  children: ReactNode;
}

/** 单侧滚动面板（原文/译文共用）；栏头已移除，双栏以 1px 分隔线区隔，语义由顶栏分段控件表达 */
export default function Pane({ label, containerRef, children }: Props) {
  return (
    <div className="relative flex h-full min-w-0 flex-1 flex-col bg-white">
      {label ? (
        <span
          data-testid="pane-label"
          className="pointer-events-none absolute left-3 top-2 z-10 select-none text-[11px] font-medium text-slate-300"
        >
          {label}
        </span>
      ) : null}
      <div ref={containerRef} className="geminus-pane flex-1 overflow-y-auto py-3" style={{ paddingInline: 'var(--pane-pad)' }}>
        {children}
      </div>
    </div>
  );
}
```

- [ ] **步骤 8.4：改调用方**

`DocumentViewer.tsx`：`<Pane title="原文" subtitle={doc.title} badge={badge} containerRef={leftRef}>` → `<Pane label="原文" containerRef={leftRef}>`；右侧同理 `<Pane label="译文" …>`；删除 `const badge = …` 行。
`PdfCompareView.tsx`：`<Pane title="原文（原始渲染）" subtitle={doc.title} badge="PDF" …>` → `<Pane label="原文" …>`；右侧 `title="译文" subtitle="翻译后"` → `label="译文"`。

- [ ] **步骤 8.5：全量单测 + 构建**

运行：`cd app && pnpm test && pnpm build`
预期：全绿（其他测试如引用旧 Pane props 按报错同步最小修）。

- [ ] **步骤 8.6：Commit**

```bash
git add -A app/src
git commit -m "feat(ui): Pane 栏头移除——极淡浮标 原文/译文（T3 栏头极简，垂直空间归还阅读区）"
```

---

### 任务 9：E2E 五脚本适配（零点击 + 新选择器）

**文件：**
- 修改：`scripts/e2e_v2.mjs`、`scripts/e2e_v2_1.mjs`、`scripts/e2e_v2_2.mjs`、`scripts/e2e_v2_3.mjs`、`scripts/e2e_v2_4.mjs`

统一改法（各脚本同名 helper / 调用点）：

- [ ] **步骤 9.1：e2e_v2.mjs**

1. `openDocAndWait`：删除 `open-btn` 点击，改上传（零点击自动解析+翻译）：
```js
async function openDocAndWait(path, mode = 'original') {
  await page.goto(BASE);
  await page.setInputFiles('[data-testid="upload-input"]', path);
  const testid = mode === 'original' ? 'pdf-compare-view' : 'document-viewer';
  await page.getByTestId(testid).waitFor({ timeout: 300000 });
}
```
2. 新增等待完成的 helper 并替换两处 `translate-btn` click + `100%` wait：
```js
async function waitTranslateDone(timeout = 180000) {
  await page.waitForFunction(
    () => document.querySelector('[data-testid="status-chip"]')?.dataset.state === 'done'
      || document.querySelector('[data-testid="status-chip"]')?.dataset.state === 'warning',
    null, { timeout },
  );
}
```
（§2：openDoc 后 `await waitTranslateDone();` 再数中文字符；§7 DOCX 同理。）
3. §4 模式切换按钮 testid 不变（`mode-text-btn`/`mode-original-btn` 保留）。
4. 顶部注释补一行：v2.6 slim toolbar 适配说明。

- [ ] **步骤 9.2：e2e_v2_1.mjs**

1. `openDoc` helper 同 9.1.1 改法（`open-btn` → upload 零点击）。
2. 三处 `translate-btn` click + `100%` 等待 → `await waitTranslateDone()`（helper 同 9.1.2，加入本文件）。
3. `font-size-control`（97 行）断言保留——控件在 Settings Popover 内，先 `await page.getByTestId('settings-btn').click();` 再断言/操作，随后 Esc 关闭。
4. 段落工具条 `retranslate-btn` 断言不变。

- [ ] **步骤 9.3：e2e_v2_2.mjs**

1. `openDoc` helper 同上改上传。
2. 两处 `translate-btn`（267/314 行）→ `waitTranslateDone()`。
3. `progress-text` 断言（270-274、317 行）改为读 Chip：
```js
const chipText = await page.evaluate(() => document.querySelector('[data-testid="status-chip"]')?.textContent ?? '');
```
完成断言用 `dataset.state`（`done`/`warning`），计数断言改为点开 Popover 读 `status-summary`。

- [ ] **步骤 9.4：e2e_v2_3.mjs**

1. `openDoc`（57 行 `open-btn`）→ 上传零点击；其余断言（canvas/文献/书眉/sup/表格 KaTeX/横幅）不动。

- [ ] **步骤 9.5：e2e_v2_4.mjs**

1. §1：`setInputFiles` 后删除 `open-btn` click——零点击自动发起 parse → 401 → 登录框自动弹出；断言改为 `login-overlay` 出现，且 `/api/parse` 请求**已发出但被 401**（拦截在后端）：
```js
let parseStatus = null;
const onResponse = (res) => { if (res.url().includes('/api/parse')) parseStatus = res.status(); };
page.on('response', onResponse);
await page.goto(BASE);
await page.setInputFiles('[data-testid="upload-input"]', `${FIX}/pdf/simple.pdf`);
const dialogVisible = await page.getByTestId('login-overlay').waitFor({ timeout: 5000 }).then(() => true).catch(() => false);
log('匿名上传零点击 → 自动弹登录框', dialogVisible);
log('匿名 /api/parse → 401（服务端门禁）', parseStatus === 401, `status=${parseStatus}`);
page.off('response', onResponse);
```
2. §3：登录成功后**无 pending 续跑**（零点击移除该机制）——登录后手动重选文件触发零点击：
```js
await page.getByTestId('auth-username').waitFor({ timeout: 5000 });
log('登录成功（用户名徽章出现）', true, `user=${cred.user}`);
await page.setInputFiles('[data-testid="upload-input"]', `${FIX}/pdf/simple.pdf`);
```
（后续 viewer 等待逻辑不变。）
3. §4.5（168-169 行）：删除 `open-btn` click，`setInputFiles` 后直接轮询滚动高度。
4. §5/§6/§7 交互断言全部不动（`.geminus-pane`/`data-block-id` 未变）。

- [ ] **步骤 9.6：Commit（E2E 适配先行落盘，任务 10 实跑后如有微调再补提交）**

```bash
git add scripts/e2e_v2.mjs scripts/e2e_v2_1.mjs scripts/e2e_v2_2.mjs scripts/e2e_v2_3.mjs scripts/e2e_v2_4.mjs
git commit -m "test(e2e): 五套 E2E 适配零点击 + slim toolbar 选择器（upload-input 零点击 / status-chip 完成态）"
```

---

### 任务 10：本地门禁 + 集成测试（E2E 全绿 + 截图）

- [ ] **步骤 10.1：前端全量门禁**

运行：`cd app && pnpm test && pnpm build`
预期：全部通过。

- [ ] **步骤 10.2：Rust 门禁（发布前置，未改后端也应绿）**

运行：`cargo test --workspace && cargo clippy --workspace -- -D warnings`
预期：全绿。

- [ ] **步骤 10.3：构建 release 服务器 + 启动（39321，E2E v2/v2_1/v2_2/v2_3 前置）**

```bash
cargo build --release -p geminus-server
mkdir -p /tmp/geminus_e2e_slim && \
GEMINUS_BIND=127.0.0.1:39321 GEMINUS_STATIC_DIR=app/dist \
GEMINUS_ANON_PAGE_LIMIT=100000 GEMINUS_ALLOW_LOCAL_PATH=1 \
GEMINUS_CACHE_PATH=/tmp/geminus_e2e_slim/cache.db GEMINUS_AUTH_DB=/tmp/geminus_e2e_slim/quota.db \
GEMINUS_HISTORY_DB=/tmp/geminus_e2e_slim/history.db GEMINUS_UPLOADS_DIR=/tmp/geminus_e2e_slim/uploads \
GEMINUS_MINERU_CACHE_DIR=data/mineru_cache \
./target/release/geminus-server &
```
（`.env.local` 的双 Token 由 server 自动读取；`GEMINUS_MINERU_CACHE_DIR=data/mineru_cache` 保留 IEEE 离线重放缓存。）
预期：`curl -s http://127.0.0.1:39321/api/health` OK。

- [ ] **步骤 10.4：跑四套 E2E（39321）**

```bash
NODE_PATH=$(npm root -g) node scripts/e2e_v2.mjs
NODE_PATH=$(npm root -g) node scripts/e2e_v2_1.mjs
NODE_PATH=$(npm root -g) node scripts/e2e_v2_2.mjs
NODE_PATH=$(npm root -g) node scripts/e2e_v2_3.mjs
```
预期：各脚本输出全绿（对失败项逐个修复——优先怀疑选择器/等待口径，不要改产品逻辑迁就测试）。

- [ ] **步骤 10.5：跑 e2e_v2_4（自起 39322 服务器）**

```bash
NODE_PATH=$(npm root -g) node scripts/e2e_v2_4.mjs
```
预期：全绿。

- [ ] **步骤 10.6：视觉走查截图（新顶栏 + 无底栏 + 浮标栏）**

用 Playwright 快速脚本（或复用 e2e_v2 场景）截三张图到 `docs/screenshots/`：`slim-toolbar-idle.png`（空态）、`slim-toolbar-translating.png`（翻译中 2px 进度条）、`slim-toolbar-done.png`（完成 ✓ + 浮标双栏）。人工核验：48px 单行、无哈希 ID、无碎片文字、无底部栏。

- [ ] **步骤 10.7：杀掉 39321 服务器，Commit**

```bash
git add -A docs screenshots scripts
git commit -m "test(e2e): slim toolbar E2E 实跑修复 + 视觉走查截图（四套+1 全绿）"
```

---

### 任务 11：完成情况记录

- [ ] **步骤 11.1：在 `docs/_local/IMPLEMENTATION_LOG.md` 追加一节**（沿用既有格式）：标题「# v2.6 Slim Toolbar（2026-09-13，ui_design_0913）」，含：任务表（T1/T2/T3 各项结果）、关键决策（file-path-input 迁 Popover、重试失败段=逐段 retranslateParagraph、E2E 过期适配）、验证证据（单测数、E2E 各套通过数、构建/截图）、与设计稿偏差（如有）。

- [ ] **步骤 11.2：Commit**

```bash
git add docs/_local/IMPLEMENTATION_LOG.md
git commit -m "docs: IMPLEMENTATION_LOG 记录 v2.6 slim toolbar 迭代"
```

---

### 任务 12：发布（geminus-deploy 技能）

- [ ] **步骤 12.1：确认工作区干净、main 分支**

运行：`git status --short && git log --oneline -3`
预期：clean。

- [ ] **步骤 12.2：按技能流程发布**

调用项目技能 `geminus-deploy`（本计划的执行入口，须读取 `.zcode/skills/geminus-deploy/SKILL.md` 并遵循）：
- 核对服务器 `GEMINUS_REQUIRE_LOGIN=0`；
- `deploy/publish.sh v2.6-slim-toolbar-rc1 --public-base=http://x.wx-rdc.top`（门禁刚跑过可加 `--skip-tests`）；
- 双段冒烟 0 失败。

- [ ] **步骤 12.3：公网人工验收要点（浏览器 http://x.wx-rdc.top）**

1. 匿名零点击：选择 simple.pdf → 自动双栏+翻译；顶栏仅 `＋打开 / ⟳x/y / [原始│对照] / ⚙ / ◔N页 / 登录` 七件套，48px 单行，无哈希/游客模式碎片。
2. 翻译中顶栏下缘 2px 进度条推进；完成 Chip 变 ✓。
3. 点 Chip：计数 + [重试失败段/重新翻译全文]；点 ⚙：字号/语言对；点 ◔：剩余页数 + [登录解锁]/[查看翻译记录]。
4. 无底部栏；双栏左上角极淡「原文/译文」浮标。
5. `curl -s http://x.wx-rdc.top/ | grep -c pdf.worker` → 0（P0 防回归）。

---

## 自检（计划完成后核对）

**1. 规格覆盖度（ui_design_0913 逐节）：**
- §二原则 1-6 → 任务 5（单行 48px/三区/主色克制/图标+title Tooltip）、任务 2/3/4（Popover 收纳）✅
- §三顶栏 7 区 → 任务 5；2px 进度条 → 任务 5.3；Chip 完成态 → 任务 2 ✅
- §三底栏移除 → 任务 7（删 StatusBar + Toast + Chip 迁移）✅
- §三栏头极简/浮标 → 任务 8 ✅
- §四 Status Popover（缓存>0 才显示、重试失败段、重新翻译）→ 任务 2 ✅
- §四 Settings Popover（字号/语言对/提示）→ 任务 3 ✅
- §四 Quota Popover → 任务 4 ✅
- §五清单：哈希 ID 移除（docTitle 用 filePath，任务 7.3-3）✅；碎片收敛（任务 4）✅；语言对/字号/重译/模式合并/底栏/栏头 → 任务 3/2/5/7/8 ✅；`＋打开`/登录/文档名保留 → 任务 5 ✅
- §六视觉令牌 → 任务 5.3 样式（h-12=48px、图标钮 32×32 rounded-lg、Chip h-6=24px rounded-full text-[11px]、分段 h-8、控件 text-xs=12→13px 近似、绿 emerald-600≈#16a34a、橙 amber-700≈#d97706）✅
- §七 T1/T2/T3 与验证（组件测试+E2E+发布）→ 任务 1-8/9-10/12 ✅
- 遗留说明：Tooltip「hover 600ms」用原生 title 实现（无引入浮层库，YAGNI）——记为与设计稿的可接受偏差。

**2. 占位符扫描：** 无「待定/TODO/类似任务 N」；所有代码步骤含完整代码；E2E 适配给出逐脚本改法与代码。✅

**3. 类型一致性：** `Popover`（trigger/panel/panelTestId/align/panelClassName）在任务 2/3/4 用法一致；`StatusChip` props 与任务 5.3 传参一致（onStop↔onStopTranslate 等映射已写明）；`progressLabel` 定义于任务 5.3、消费于任务 7.3；`QuotaChip.onLoginClick/onViewUsage` 与任务 7.3 传参一致；`Pane`（label/containerRef/children）与任务 8.4 调用一致。✅
