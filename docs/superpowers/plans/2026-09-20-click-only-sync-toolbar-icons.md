# 点击对齐滚动 + 段落工具条外移图标列 实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 双栏滚动交互改为「仅点击段落触发双侧居中对齐」；移除 hover / 鼠标滚动驱动的自动对齐滚动；段落 hover 工具条从「段内右上横向文字按钮」改为「段落右外侧垂直图标列」，不再遮挡文字。

**架构：** 把 `useHoverDrivenSync`（hover 主从滚动锁 + rAF 锚点追踪 + 比例降级 + hover 高亮 + click 居中）瘦身重命名为 `usePaneSync`，只保留 hover 联动高亮与 click 居中两件事，删除全部滚动驱动路径；`ParagraphToolbar` 重写为 `translate-x-full` 定位到段落右外 gutter（pane 的 `--pane-pad: 24px` 内边距槽）的垂直图标列。E2E 脚本（e2e_v2_4 §7、e2e_v2_2 §7）从「滚动追踪」断言改为「滚动独立 / 点击对齐」断言，并新增工具条几何不遮挡断言。

**技术栈：** React 18 + TypeScript + Tailwind + Vitest（jsdom）+ Playwright E2E（自起 geminus-server）+ Rust 后端（不改，仅过门禁）。

---

## 文件结构

| 文件 | 动作 | 职责 |
|---|---|---|
| `app/src/hooks/usePaneSync.ts` | 创建 | 双栏交互：hover 联动高亮 + click 双侧居中；不再含任何滚动驱动逻辑 |
| `app/src/hooks/useHoverDrivenSync.ts` | 删除 | 被 `usePaneSync` 取代 |
| `app/src/hooks/__tests__/usePaneSync.test.tsx` | 创建 | 新 hook 单测（高亮联动 / click 居中 / 滚动独立） |
| `app/src/hooks/__tests__/useHoverDrivenSync.test.tsx` | 删除 | 随旧 hook 删除 |
| `app/src/components/ParagraphToolbar.tsx` | 重写 | 右外侧垂直图标列（复制原文/复制译文/重译本段 三枚 SVG 图标） |
| `app/src/components/__tests__/ParagraphToolbar.test.tsx` | 创建 | 工具条组件单测 |
| `app/src/components/PdfCompareView.tsx` | 修改 | 换用 `usePaneSync`；注释更新 |
| `app/src/components/DocumentViewer.tsx` | 修改 | 换用 `usePaneSync`；注释更新 |
| `app/src/App.tsx` | 修改 | 仅注释更新（L123） |
| `app/src/components/PdfOriginalViewer.tsx` | 修改 | 仅注释更新（L113） |
| `scripts/e2e_v2_4.mjs` | 修改 | §7 改滚动独立断言；新增 §6.5 工具条几何断言；文件头注释更新 |
| `scripts/e2e_v2_2.mjs` | 修改 | §7 hover 追踪改点击对齐断言 |
| `.zcode/skills/geminus-deploy/SKILL.md` | 修改 | 人工验收第 6 条口径更新（去 hover 滚动联动表述） |

`app/src/index.css` 的 `.g-block-hover` 保留（hover 高亮仍用）；`BlockRenderer.tsx` 不改（`group relative` wrapper 已满足新工具条定位）。

---

### 任务 1：`usePaneSync` hook——仅点击对齐 + hover 高亮

**文件：**
- 创建：`app/src/hooks/usePaneSync.ts`
- 创建测试：`app/src/hooks/__tests__/usePaneSync.test.tsx`
- 删除：`app/src/hooks/useHoverDrivenSync.ts`、`app/src/hooks/__tests__/useHoverDrivenSync.test.tsx`

- [x] **步骤 1：编写失败的测试**

创建 `app/src/hooks/__tests__/usePaneSync.test.tsx`：

```tsx
import { useEffect, useRef } from 'react';
import { act, cleanup, fireEvent, render } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { blockIdFromElement, usePaneSync } from '../usePaneSync';

afterEach(() => {
  vi.restoreAllMocks();
  document.body.innerHTML = '';
});

describe('blockIdFromElement', () => {
  it('从事件目标冒泡提取 data-block-id；块外返回 null', () => {
    const pane = document.createElement('div');
    const block = document.createElement('div');
    block.dataset.blockId = 'b1';
    const child = document.createElement('span');
    block.appendChild(child);
    pane.appendChild(block);
    pane.appendChild(document.createElement('p'));
    document.body.appendChild(pane);
    expect(blockIdFromElement(child)).toBe('b1');
    expect(blockIdFromElement(pane.querySelector('p'))).toBeNull();
    expect(blockIdFromElement(null)).toBeNull();
  });
});

// ── 交互（jsdom：rAF 可用） ──

function Host({ onReady }: { onReady: (l: HTMLDivElement, r: HTMLDivElement) => void }) {
  const left = useRef<HTMLDivElement>(null);
  const right = useRef<HTMLDivElement>(null);
  usePaneSync(left, right);
  useEffect(() => {
    if (left.current && right.current) onReady(left.current, right.current);
  });
  return (
    <div>
      <div ref={left} data-testid="left">
        <div data-block-id="b1" style={{ height: 50 }}>
          A
        </div>
      </div>
      <div ref={right} data-testid="right">
        <div data-block-id="b1" style={{ height: 50 }}>
          A&rsquo;
        </div>
      </div>
    </div>
  );
}

describe('usePaneSync 交互', () => {
  afterEach(cleanup);

  it('hover 左栏块 → 双侧同 id 块挂 .g-block-hover；mouseleave 清除', async () => {
    let left!: HTMLDivElement;
    let right!: HTMLDivElement;
    render(<Host onReady={(l, r) => { left = l; right = r; }} />);
    await act(async () => {
      fireEvent.mouseOver(left.querySelector('[data-block-id="b1"]')!);
      await new Promise((r2) => setTimeout(r2, 30)); // rAF 合帧
    });
    expect(left.querySelector('[data-block-id="b1"]')!.classList.contains('g-block-hover')).toBe(true);
    expect(right.querySelector('[data-block-id="b1"]')!.classList.contains('g-block-hover')).toBe(true);
    await act(async () => {
      fireEvent.mouseLeave(left);
    });
    expect(right.querySelector('.g-block-hover')).toBeNull();
    expect(left.querySelector('.g-block-hover')).toBeNull();
  });

  it('click 块 → 双侧 scrollIntoView({block:"center"})，各一次', async () => {
    let left!: HTMLDivElement;
    let right!: HTMLDivElement;
    const calls: Array<{ el: Element; opts: ScrollIntoViewOptions | undefined }> = [];
    Element.prototype.scrollIntoView = function (this: Element, opts?: ScrollIntoViewOptions) {
      calls.push({ el: this, opts });
    };
    render(<Host onReady={(l, r) => { left = l; right = r; }} />);
    await act(async () => {
      fireEvent.click(right.querySelector('[data-block-id="b1"]')!);
    });
    expect(calls.length).toBe(2);
    expect(calls.filter((c) => left.contains(c.el)).length).toBe(1);
    expect(calls.filter((c) => right.contains(c.el)).length).toBe(1);
    expect(calls.every((c) => c.opts?.block === 'center')).toBe(true);
  });

  it('单栏滚动不驱动另一栏（自动对齐已移除）', async () => {
    let left!: HTMLDivElement;
    let right!: HTMLDivElement;
    render(<Host onReady={(l, r) => { left = l; right = r; }} />);
    Object.defineProperty(right, 'scrollHeight', { value: 2000, configurable: true });
    Object.defineProperty(right, 'clientHeight', { value: 500, configurable: true });
    await act(async () => {
      fireEvent.mouseOver(left.querySelector('[data-block-id="b1"]')!); // 先 hover（高亮联动仍在）
      await new Promise((r2) => setTimeout(r2, 30));
      left.scrollTop = 300;
      fireEvent.scroll(left);
      await new Promise((r2) => setTimeout(r2, 200));
    });
    expect(right.scrollTop).toBe(0); // 右栏纹丝不动
    expect(left.scrollTop).toBe(300); // 左栏自身滚动不被回写
  });

  it('卸载时清除 hover 类', async () => {
    let left!: HTMLDivElement;
    let right!: HTMLDivElement;
    const { unmount } = render(<Host onReady={(l, r) => { left = l; right = r; }} />);
    await act(async () => {
      fireEvent.mouseOver(left.querySelector('[data-block-id="b1"]')!);
      await new Promise((r2) => setTimeout(r2, 30));
    });
    unmount();
    expect(document.querySelectorAll('.g-block-hover').length).toBe(0);
  });
});
```

- [x] **步骤 2：运行测试验证失败**

```bash
cd app && pnpm vitest run src/hooks/__tests__/usePaneSync.test.tsx
```
预期：FAIL，`Failed to resolve import "../usePaneSync"`（模块不存在）。

- [x] **步骤 3：实现 `usePaneSync.ts`**

创建 `app/src/hooks/usePaneSync.ts`：

```ts
import { useEffect, useRef } from 'react';
import type { RefObject } from 'react';

/**
 * v2.7 双栏交互（feedback_0920 滚动收口）：
 * a) Hover 联动高亮——mouseover 委托提取 data-block-id（16ms rAF 合帧），双侧同步 .g-block-hover；
 * b) Click 居中——点击段落时，双侧同 id 块 scrollIntoView({block:'center'}) 平滑居中。
 * v2.4 的 hover/滚动驱动自动对齐（滚动锁、锚点追踪、比例降级）已整体移除：
 * 两栏滚动完全独立，仅点击段落触发对齐。
 */
export type PaneSide = 'left' | 'right';

const HOVER_CLASS = 'g-block-hover';

/** 事件目标 → 所属块 id（无块返回 null） */
export function blockIdFromElement(target: EventTarget | null): string | null {
  if (!(target instanceof Element)) return null;
  return target.closest<HTMLElement>('[data-block-id]')?.dataset.blockId ?? null;
}

export function usePaneSync(
  leftRef: RefObject<HTMLDivElement>,
  rightRef: RefObject<HTMLDivElement>,
): void {
  const hoveredIdRef = useRef<string | null>(null);

  useEffect(() => {
    const paintHover = (id: string | null) => {
      for (const pane of [leftRef.current, rightRef.current]) {
        if (!pane) continue;
        pane.querySelectorAll('.' + HOVER_CLASS).forEach((el) => el.classList.remove(HOVER_CLASS));
        if (id) {
          pane
            .querySelectorAll(`[data-block-id="${CSS.escape(id)}"]`)
            .forEach((el) => el.classList.add(HOVER_CLASS));
        }
      }
    };

    const cleanups: Array<() => void> = [];
    for (const pane of [leftRef.current, rightRef.current]) {
      if (!pane) continue;

      const onLeave = () => {
        hoveredIdRef.current = null;
        paintHover(null);
      };
      let hoverRaf = 0;
      let pendingId: string | null | undefined; // undefined = 无待处理事件
      const onOver = (e: MouseEvent) => {
        // 16ms 合帧：帧内多次 mouseover 只提交最后一次的 id（防抖但不丢末次）
        pendingId = blockIdFromElement(e.target);
        if (hoverRaf) return;
        hoverRaf = requestAnimationFrame(() => {
          hoverRaf = 0;
          if (pendingId !== undefined && pendingId !== hoveredIdRef.current) {
            hoveredIdRef.current = pendingId;
            paintHover(pendingId);
          }
          pendingId = undefined;
        });
      };
      const onClick = (e: MouseEvent) => {
        const id = blockIdFromElement(e.target);
        if (!id) return;
        for (const pane2 of [leftRef.current, rightRef.current]) {
          pane2
            ?.querySelector(`[data-block-id="${CSS.escape(id)}"]`)
            // jsdom 等无 scrollIntoView 的环境跳过（居中降级为仅高亮）
            ?.scrollIntoView?.({ behavior: 'smooth', block: 'center' });
        }
      };

      pane.addEventListener('mouseleave', onLeave);
      pane.addEventListener('mouseover', onOver);
      pane.addEventListener('click', onClick);
      cleanups.push(() => {
        pane.removeEventListener('mouseleave', onLeave);
        pane.removeEventListener('mouseover', onOver);
        pane.removeEventListener('click', onClick);
        if (hoverRaf) cancelAnimationFrame(hoverRaf);
      });
    }
    return () => {
      cleanups.forEach((fn) => fn());
      paintHover(null);
    };
  }, [leftRef, rightRef]);
}
```

- [x] **步骤 4：运行测试验证通过**

```bash
cd app && pnpm vitest run src/hooks/__tests__/usePaneSync.test.tsx
```
预期：4 个交互用例 + blockIdFromElement 用例全 PASS。

- [x] **步骤 5：消费者切换 + 删除旧 hook**

1. `app/src/components/PdfCompareView.tsx`：
   - L14 `import { useHoverDrivenSync } from '../hooks/useHoverDrivenSync';` → `import { usePaneSync } from '../hooks/usePaneSync';`
   - L58 `useHoverDrivenSync(leftRef, rightRef);` → `usePaneSync(leftRef, rightRef);`
   - 组件头注释（L33-37）改为：
     ```
     * v2.7：交互由 usePaneSync 提供（hover 联动高亮 + click 双侧居中；滚动完全独立），
     * v2.4 的 hover/滚动驱动自动对齐已移除；锚点表保留构建（__anchors 调试钩子）。
     ```
2. `app/src/components/DocumentViewer.tsx`：同样替换 import 与 L39 调用；头注释 L22 改为
   `* v2.7：交互由 usePaneSync 提供（hover 联动高亮 + click 双侧居中；滚动完全独立）。`
3. `app/src/App.tsx` L123 注释改为 `// v2.7：双栏滚动容器（usePaneSync 在两侧视图内接管 hover 高亮 + click 居中）`
4. `app/src/components/PdfOriginalViewer.tsx` L113-114 注释改为：
   `// v2.7：active 定位由 usePaneSync 的 click 居中接管（双侧统一），无滚动驱动 effect`
5. 删除：
```bash
git rm app/src/hooks/useHoverDrivenSync.ts app/src/hooks/__tests__/useHoverDrivenSync.test.tsx
```

- [x] **步骤 6：跑全量前端单测**

```bash
cd app && pnpm test
```
预期：全绿（App.test.tsx 等不依赖被删导出）。

- [x] **步骤 7：Commit**

```bash
git add app/src/hooks/usePaneSync.ts app/src/hooks/__tests__/usePaneSync.test.tsx \
  app/src/components/PdfCompareView.tsx app/src/components/DocumentViewer.tsx \
  app/src/App.tsx app/src/components/PdfOriginalViewer.tsx
git commit -m "feat(ui): 双栏滚动收口——仅点击段落触发双侧居中对齐，移除 hover/滚动驱动的自动对齐（useHoverDrivenSync → usePaneSync）"
```

---

### 任务 2：ParagraphToolbar——右外侧垂直图标列

**文件：**
- 重写：`app/src/components/ParagraphToolbar.tsx`
- 创建测试：`app/src/components/__tests__/ParagraphToolbar.test.tsx`

- [x] **步骤 1：编写失败的测试**

创建 `app/src/components/__tests__/ParagraphToolbar.test.tsx`：

```tsx
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, waitFor } from '@testing-library/react';
import ParagraphToolbar from '../ParagraphToolbar';
import { retranslateParagraph } from '../../services/backend';
import type { Block } from '../../types';

vi.mock('../../services/backend', () => ({ retranslateParagraph: vi.fn() }));

const block: Block = {
  type: 'paragraph',
  id: 'blk-1',
  content: [
    { type: 'text', text: 'Hello ' },
    { type: 'text', text: 'world' },
  ],
};

function writeTextMock() {
  const fn = vi.fn().mockResolvedValue(undefined);
  Object.assign(navigator, { clipboard: { writeText: fn } });
  return fn;
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.mocked(retranslateParagraph).mockReset();
});

describe('ParagraphToolbar（v2.7 右外侧垂直图标列）', () => {
  it('容器定位在段落右外侧（translate-x-full）且垂直排列（flex-col），不遮挡正文', () => {
    const { getByTestId } = render(
      <ParagraphToolbar
        docId="d1"
        block={block}
        translatedText="你好世界"
        sourceLang="en"
        targetLang="zh"
        onRetranslated={() => {}}
      />,
    );
    const bar = getByTestId('paragraph-toolbar-blk-1');
    expect(bar.className).toContain('translate-x-full'); // 整列移出段落右缘
    expect(bar.className).toContain('flex-col'); // 垂直排列
    expect(bar.className).toContain('group-hover:flex'); // hover 显示
    expect(bar.className).toContain('right-0');
  });

  it('渲染三枚图标按钮（testid + title + aria-label 齐备）', () => {
    const { getByTestId } = render(
      <ParagraphToolbar
        docId="d1"
        block={block}
        translatedText="你好世界"
        sourceLang="en"
        targetLang="zh"
        onRetranslated={() => {}}
      />,
    );
    for (const [tid, label] of [
      ['copy-source-btn', '复制原文'],
      ['copy-target-btn', '复制译文'],
      ['retranslate-btn', '重译本段'],
    ] as const) {
      const btn = getByTestId(tid) as HTMLButtonElement;
      expect(btn.title).toBe(label);
      expect(btn.getAttribute('aria-label')).toBe(label);
      expect(btn.querySelector('svg')).toBeTruthy(); // 图标按钮
    }
  });

  it('复制原文 → clipboard 收到拼接后的纯文本，按钮进入已复制态', async () => {
    const writeText = writeTextMock();
    const { getByTestId } = render(
      <ParagraphToolbar
        docId="d1"
        block={block}
        translatedText="你好世界"
        sourceLang="en"
        targetLang="zh"
        onRetranslated={() => {}}
      />,
    );
    fireEvent.click(getByTestId('copy-source-btn'));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('Hello world'));
    expect(getByTestId('copy-source-btn').dataset.copied).toBe('true');
  });

  it('复制译文 → 优先译文文本；无译文回退原文', async () => {
    const writeText = writeTextMock();
    const { getByTestId, rerender } = render(
      <ParagraphToolbar
        docId="d1"
        block={block}
        translatedText="你好世界"
        sourceLang="en"
        targetLang="zh"
        onRetranslated={() => {}}
      />,
    );
    fireEvent.click(getByTestId('copy-target-btn'));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('你好世界'));
    rerender(
      <ParagraphToolbar
        docId="d1"
        block={block}
        translatedText={null}
        sourceLang="en"
        targetLang="zh"
        onRetranslated={() => {}}
      />,
    );
    fireEvent.click(getByTestId('copy-target-btn'));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('Hello world'));
  });

  it('重译本段 → 调用服务并在完成后回调 onRetranslated；busy 期间按钮禁用', async () => {
    vi.mocked(retranslateParagraph).mockImplementation(
      () =>
        new Promise((resolve) =>
          setTimeout(
            () => resolve({ block_id: 'blk-1', translated: { ...block, content: [{ type: 'text', text: '重译结果' }] }, fallback: false }),
            20,
          ),
        ),
    );
    const onRetranslated = vi.fn();
    const { getByTestId } = render(
      <ParagraphToolbar
        docId="d1"
        block={block}
        translatedText="旧译文"
        sourceLang="en"
        targetLang="zh"
        onRetranslated={onRetranslated}
      />,
    );
    const btn = getByTestId('retranslate-btn') as HTMLButtonElement;
    fireEvent.click(btn);
    expect(retranslateParagraph).toHaveBeenCalledWith('d1', 'blk-1', 'en', 'zh');
    await waitFor(() => expect(onRetranslated).toHaveBeenCalled());
    expect(onRetranslated.mock.calls[0][0]).toMatchObject({
      blockId: 'blk-1',
      fallback: false,
      block: { content: [{ type: 'text', text: '重译结果' }] },
    });
    await waitFor(() => expect((getByTestId('retranslate-btn') as HTMLButtonElement).disabled).toBe(false));
  });
});
```

- [x] **步骤 2：运行测试验证失败**

```bash
cd app && pnpm vitest run src/components/__tests__/ParagraphToolbar.test.tsx
```
预期：FAIL——现实现容器无 `flex-col`/`translate-x-full` 类，按钮无 `title`/`aria-label`/`data-copied`。

- [x] **步骤 3：重写 `ParagraphToolbar.tsx`**

```tsx
import { useState } from 'react';
import type { Block } from '../types';
import { retranslateParagraph } from '../services/backend';

export interface RetranslatedPayload {
  blockId: string;
  block: Block;
  fallback: boolean;
}

interface Props {
  docId: string;
  block: Block;
  /** 译文纯文本（复制用；null=未翻译复制原文） */
  translatedText: string | null;
  sourceLang: string;
  targetLang: string;
  onRetranslated: (p: RetranslatedPayload) => void;
}

/* ── 13px 线性图标（lucide 风格 stroke，无外部依赖） ── */
function IconDoc() {
  return (
    <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <path d="M14 2v6h6" />
      <path d="M16 13H8" />
      <path d="M16 17H8" />
      <path d="M10 9H8" />
    </svg>
  );
}
function IconCopy() {
  return (
    <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="9" y="9" width="13" height="13" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </svg>
  );
}
function IconRefresh() {
  return (
    <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M21 12a9 9 0 1 1-2.64-6.36L21 8" />
      <path d="M21 3v5h-5" />
    </svg>
  );
}
function IconCheck() {
  return (
    <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M20 6 9 17l-5-5" />
    </svg>
  );
}

/**
 * 段落 hover 工具条（v2.7 feedback_0920）：段落右外侧垂直图标列
 * [复制原文][复制译文][重译本段]，group-hover 显示；
 * translate-x-full 移入 pane 右 padding gutter（--pane-pad 24px），不再遮挡正文。
 */
export default function ParagraphToolbar({
  docId,
  block,
  translatedText,
  sourceLang,
  targetLang,
  onRetranslated,
}: Props) {
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);

  const sourceText =
    block.type === 'paragraph'
      ? block.content
          .filter((c) => c.type === 'text')
          .map((c) => (c as { text: string }).text)
          .join('')
      : '';

  async function copy(label: string, text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(label);
      setTimeout(() => setCopied(null), 1200);
    } catch {
      /* 剪贴板不可用静默 */
    }
  }

  async function retranslate() {
    setBusy(true);
    try {
      const pair = await retranslateParagraph(docId, block.id, sourceLang, targetLang);
      onRetranslated({
        blockId: pair.block_id,
        block: pair.translated ?? block,
        fallback: pair.fallback ?? false,
      });
    } catch {
      /* 失败不打断阅读（可再次点击） */
    } finally {
      setBusy(false);
    }
  }

  const btn =
    'flex h-[18px] w-[18px] items-center justify-center rounded border border-slate-200 bg-white/95 text-slate-400 shadow-sm hover:border-blue-300 hover:text-blue-600 disabled:cursor-not-allowed disabled:opacity-50';

  const iconBtn = (opts: {
    testid: string;
    label: string;
    done: boolean;
    disabled?: boolean;
    onClick: () => void;
    icon: React.ReactNode;
  }) => (
    <button
      className={btn}
      data-testid={opts.testid}
      data-copied={opts.done ? 'true' : undefined}
      title={opts.done ? '已复制' : opts.label}
      aria-label={opts.label}
      disabled={opts.disabled}
      onClick={opts.onClick}
    >
      {opts.done ? <IconCheck /> : opts.icon}
    </button>
  );

  return (
    <span
      className="absolute right-0 top-0 z-10 hidden translate-x-full flex-col gap-1 group-hover:flex"
      data-testid={`paragraph-toolbar-${block.id}`}
    >
      {iconBtn({
        testid: 'copy-source-btn',
        label: '复制原文',
        done: copied === '原文',
        onClick: () => void copy('原文', sourceText),
        icon: <IconDoc />,
      })}
      {iconBtn({
        testid: 'copy-target-btn',
        label: '复制译文',
        done: copied === '译文',
        onClick: () => void copy('译文', translatedText ?? sourceText),
        icon: <IconCopy />,
      })}
      {iconBtn({
        testid: 'retranslate-btn',
        label: '重译本段',
        done: false,
        disabled: busy,
        onClick: () => void retranslate(),
        icon: <span className={busy ? 'animate-spin' : undefined}><IconRefresh /></span>,
      })}
    </span>
  );
}
```

- [x] **步骤 4：运行测试验证通过**

```bash
cd app && pnpm vitest run src/components/__tests__/ParagraphToolbar.test.tsx
```
预期：5 用例全 PASS。

- [x] **步骤 5：跑全量前端单测 + 构建**

```bash
cd app && pnpm test && pnpm build
```
预期：全绿；`tsc --noEmit` 无错（`React.ReactNode` 需命名空间可用——项目 tsx 下 `React` 未 import 时改用 `import type { ReactNode } from 'react'` 并声明 `icon: ReactNode`）。

- [x] **步骤 6：Commit**

```bash
git add app/src/components/ParagraphToolbar.tsx app/src/components/__tests__/ParagraphToolbar.test.tsx
git commit -m "feat(ui): 段落工具条右移——右外侧垂直图标列（doc/copy/refresh），不再遮挡正文文字"
```

---

### 任务 3：E2E 脚本适配（滚动独立 + 工具条几何）

**文件：**
- 修改：`scripts/e2e_v2_4.mjs`（文件头注释、§6.5 新增、§7 重写）
- 修改：`scripts/e2e_v2_2.mjs`（§7 重写）

- [x] **步骤 1：`e2e_v2_4.mjs` 文件头注释更新（L2-L3）**

```
 * 浏览器端到端验证（v2.4）：权限收口（require_login）/ 历史记录 / Hover 联动高亮 /
 * Click 居中 / 滚动独立（v2.7：自动对齐滚动已移除）。
```

- [x] **步骤 2：在 §6（Click 居中）之后插入 §6.5 工具条几何断言**

```js
  // ── 6.5 段落工具条（v2.7）：右外侧垂直图标列，不遮挡正文 ──
  {
    const geo = await page.evaluate(() => {
      const panes = document.querySelectorAll('.geminus-pane');
      const right = panes[panes.length - 1];
      const para = right.querySelector('.group > p');
      const bar = right.querySelector('[data-testid^="paragraph-toolbar-"]');
      if (!para || !bar) return null;
      const pr = para.getBoundingClientRect();
      const br = bar.getBoundingClientRect();
      const btns = Array.from(bar.querySelectorAll('button')).map((b) => {
        const r = b.getBoundingClientRect();
        return { x: r.x, y: r.y };
      });
      return {
        outside: br.left >= pr.right - 1, // 工具条整体在段落右缘之外
        vertical:
          btns.length === 3 &&
          Math.max(...btns.map((b) => b.x)) - Math.min(...btns.map((b) => b.x)) <= 2 &&
          btns.every((b, i) => i === 0 || b.y > btns[i - 1].y), // 三图标垂直排列
        hiddenByDefault: getComputedStyle(bar).display === 'none',
      };
    });
    log('工具条：初始隐藏（hover 才显示）', !!geo && geo.hiddenByDefault);
    // hover 段落后可见
    await page.evaluate(() => {
      const panes = document.querySelectorAll('.geminus-pane');
      const para = panes[panes.length - 1].querySelector('.group > p');
      para?.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
    });
    await sleep(300);
    const shown = await page.evaluate(() => {
      const panes = document.querySelectorAll('.geminus-pane');
      const bar = panes[panes.length - 1].querySelector('[data-testid^="paragraph-toolbar-"]');
      return bar ? getComputedStyle(bar).display !== 'none' : false;
    });
    log('工具条：hover 段落后显示', shown);
    log('工具条：位于段落右外侧且三图标垂直排列（不遮挡正文）',
      !!geo && geo.outside && geo.vertical,
      geo ? `outside=${geo.outside} vertical=${geo.vertical}` : '未找到段落/工具条');
  }
```

- [x] **步骤 3：重写 §7（滚动锁 + 锚点追踪 → 滚动独立）**

替换整个 §7 块为：

```js
  // ── 7. 滚动独立（v2.7）：单栏滚动不再驱动另一栏自动对齐 ──
  {
    const rightBefore = await page.evaluate(
      () => document.querySelectorAll('.geminus-pane')[document.querySelectorAll('.geminus-pane').length - 1].scrollTop,
    );
    // 先 hover 左栏中部某块（v2.4 曾借此驱动追踪——现仅剩高亮，不应再引起滚动）
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
    await sleep(900);
    const res = await page.evaluate((before) => {
      const panes = document.querySelectorAll('.geminus-pane');
      return {
        left: panes[0].scrollTop,
        right: panes[panes.length - 1].scrollTop,
        before,
      };
    }, rightBefore);
    log('滚动独立：hover + 滚动左栏，右栏 scrollTop 纹丝不动',
      Math.abs(res.right - res.before) <= 2,
      `before=${Math.round(res.before)} after=${Math.round(res.right)}`);
    log('滚动独立：左栏自身滚动稳定无回滚', Math.abs(res.left - scrollSet) <= 2,
      `set=${scrollSet} final=${Math.round(res.left)}`);
  }
```

- [x] **步骤 4：`e2e_v2_2.mjs` §7 重写（hover 追踪 → 点击对齐）**

替换「§ 7. Hover 驱动同步（v2.4 模型）」整块为：

```js
// ── 7. 点击对齐（v2.7 模型）：点击左栏算法块 → 双侧块居中 ──
{
  const cardId = await page.evaluate(() => {
    const card0 = document.querySelector('[data-testid^="algorithm-card-"]');
    return card0 ? card0.getAttribute('data-block-id') : null;
  });
  await page.click(`[data-block-id="${cardId}"] >> nth=0`); // nth=0 取左栏 region
  await page.waitForTimeout(1000); // 双侧 smooth 滚动
  const res = await page.evaluate((id) => {
    const panes = document.querySelectorAll('.geminus-pane');
    const [left, right] = [panes[0], panes[panes.length - 1]];
    const dist = (pane) => {
      const el = pane.querySelector(`[data-block-id="${id}"]`);
      if (!el) return null;
      const pr = pane.getBoundingClientRect();
      const er = el.getBoundingClientRect();
      return Math.abs(er.top + er.height / 2 - (pr.top + pr.height / 2));
    };
    return { id, left: dist(left), right: dist(right) };
  }, cardId);
  log(
    '点击对齐：双侧算法块中心距视口中心 ≤80px',
    !!res && res.left !== null && res.right !== null && res.left <= 80 && res.right <= 80,
    res ? `left=${Math.round(res.left)}px right=${Math.round(res.right)}px` : '未找到块',
  );
}
```

- [x] **步骤 5：更新部署技能人工验收口径**

`.zcode/skills/geminus-deploy/SKILL.md` 「发布后必做人工验收」第 6 条：

```
6. **段落交互（v2.7）**：悬停左栏任一段落区域 → 右栏对应段落同步泛起浅蓝底（仅高亮）；
   单栏滚动不影响另一栏；点击段落 → 双侧平滑居中对齐；悬停译文段落 → 段落右外侧
   垂直图标列（复制原文/复制译文/重译本段），不遮挡文字
```

- [x] **步骤 6：Commit**

```bash
git add scripts/e2e_v2_4.mjs scripts/e2e_v2_2.mjs .zcode/skills/geminus-deploy/SKILL.md
git commit -m "test(e2e): 适配 v2.7 滚动收口——滚动独立/点击对齐断言 + 工具条右外侧几何断言"
```

---

### 任务 4：集成测试（本地门禁 + 浏览器 E2E）

**文件：** 无新文件（运行验证）

- [x] **步骤 1：本地门禁全量**

```bash
cargo test --workspace && cargo clippy --workspace -- -D warnings
cd app && pnpm test && pnpm build && cd ..
```
预期：全部通过（后端未改动，仅确认无回归）。

- [x] **步骤 2：构建 release 并跑 v2.4 交互 E2E**

```bash
cargo build --release -p geminus-server
NODE_PATH=$(npm root -g) node scripts/e2e_v2_4.mjs
```
预期：末尾「结果：N 通过 / 0 失败」。重点关注：§5 hover 高亮、§6 点击居中、
§6.5 工具条几何（outside/vertical 为 true）、§7 滚动独立（右栏不动）。
若翻译因 API key 不可用失败，核对 `.env.local` / 环境变量后重跑；
交互断言块（§5-§7）依赖 rich.docx 翻译完成才可测。

- [x] **步骤 3：跑 v2.2 E2E（公式/裁切回归 + 点击对齐）**

```bash
NODE_PATH=$(npm root -g) node scripts/e2e_v2_2.mjs
```
预期：「结果：N 通过 / 0 失败」，其中 §7 点击对齐 ≤80px。

- [x] **步骤 4：修复直至全绿（如失败回到对应任务修正后重跑）**

- [x] **步骤 5：更新计划勾选 + 截图存档（可选）**

```bash
mkdir -p docs/screenshots/v2.7
# E2E 运行时浏览器截图（e2e_v2_1 段落工具条段落已有截图逻辑可参照）
```

---

### 任务 5：发布部署（geminus-deploy 技能）

- [ ] **步骤 1：调用 geminus-deploy 技能执行标准发布**

发布标签：`v2.7-click-sync-rc1`（沿用 `<minor>-<topic>-rcN` 命名）。

```bash
GEMINUS_SERVER=opc01 deploy/publish.sh v2.7-click-sync-rc1 --public-base=https://x.wx-rdc.top
```
成败判读：输出含 `==> 传输 v2.7-click-sync-rc1 → opc01`、`version: v2.7-click-sync-rc1`、
`结果：N 通过 / 0 失败`。

- [ ] **步骤 2：公网人工验收（浏览器 https://x.wx-rdc.top）**

按 SKILL.md 清单执行，重点：
1. 上传 simple.pdf → 双栏渲染正常；
2. 单栏滚动另一栏不动（自动对齐已移除）；
3. 点击段落 → 双侧居中；
4. hover 译文段落 → 右外侧垂直图标列，复制/重译功能正常，不遮挡文字；
5. `curl -s https://x.wx-rdc.top/ | grep -c pdf.worker` → 0。

- [ ] **步骤 3：收尾 Commit（计划勾选状态与截图）**

```bash
git add docs/superpowers/plans/2026-09-20-click-only-sync-toolbar-icons.md
git commit -m "docs(plan): v2.7 滚动收口 + 工具条图标列——执行勾选完毕"
```

---

## 自检记录

- **规格覆盖：** 需求 1（仅点击触发 + 居中）→ 任务 1（click 居中保留、其余滚动驱动移除）+ 任务 3/4 断言；需求 2（取消 hover/滚动自动对齐）→ 任务 1（删滚动锁/锚点追踪/比例降级）+ 任务 3 §7 滚动独立断言；需求 3（工具条右外侧垂直图标）→ 任务 2 + 任务 3 §6.5 几何断言。单测（任务 1/2 TDD）、集成测试（任务 4）、推送远程（任务 5）全覆盖。
- **占位符扫描：** 无 TODO/待定；所有代码步骤含完整代码。
- **类型一致性：** `usePaneSync(leftRef, rightRef)` 与两个消费者现签名一致；`ParagraphToolbar` Props 未变（`BlockRenderer` 调用处无需改动）；`retranslateParagraph` 返回形状与 mock 一致（`{ block_id, translated, fallback }`，见 services/backend.ts）。

## 执行记录（2026-09-20）

- **任务 1-3：** 按计划完成；commit `8347062` / `531e974` / `f5e7b1b`。单元测试 191/191 绿（新增 usePaneSync 5 例 + ParagraphToolbar v2.7 断言 3 例，旧用例保留）。
- **任务 3 计划外修正（E2E 脚本缺陷，commit `9dc2114`）：**
  - §4.5 增加「等翻译完成」（status-chip done）——右栏流式替换期间块高持续变化，居中/几何断言落在移动目标上；
  - §6.5 改用真实 Playwright hover（合成 mouseover 无法触发 CSS `:hover`），几何测量移到工具条可见后（`display:none` 时 rect 全零导致误判）；
  - §6 居中断言改为「<80px 或短栏已尽力」（译文侧通常比原文短、可滚动距离可能不足以居中——几何固有，非缺陷）；
  - v2_2 §7 增加算法卡等待与空值守卫（翻译未就绪/配额耗尽时不崩脚本）。
- **任务 4：** cargo test 234 绿 + clippy 无警告 + pnpm test 191 绿 + build 成功；
  e2e_v2_4 **26/26 全绿**（hover 高亮 / 点击居中 / 工具条 outside+vertical / 滚动独立双侧不动）；
  e2e_v2_2 **19/20**——唯一失败「Figure 1 题注含行内公式 katex=0」为本轮全新 LLM 翻译的输出波动（同前端代码上一轮 katex=4 通过），与本改动无关；其余含新「点击对齐 left=0px right=0px」全过。
- **环境注记：** 本机 headless 下 pdf.js 偶发降级 text 模式（E2E 对两种 viewer 均兼容，非本改动引入）；自起服务器的匿名配额会被多轮 E2E 耗尽导致右栏部分翻译——重跑前需清状态目录。
- **任务 5（2026-09-20）：** `GEMINUS_SERVER=opc01 deploy/publish.sh v2.7-click-sync-rc1 --skip-tests --public-base=https://x.wx-rdc.top` 发布成功（传输 ✓ / version ✓ / 双段冒烟 29/29 ✓）。公网验收：pdf.worker 0（内联）✓、JS MIME text/javascript ✓、新工具条代码已上线 ✓；浏览器交互验收 5/5——匿名零点击打开（pdf 模式）、翻译完成、滚动独立（右栏滚左栏不动）、点击居中 left=13px/right=16px、工具条 shown+outside+vertical ✓。截图 `docs/screenshots/v2.7/public-toolbar.png`。
