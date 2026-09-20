import { useState } from 'react';
import type { ReactNode } from 'react';
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
    <svg
      viewBox="0 0 24 24"
      width="13"
      height="13"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
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
    <svg
      viewBox="0 0 24 24"
      width="13"
      height="13"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="9" y="9" width="13" height="13" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </svg>
  );
}
function IconRefresh() {
  return (
    <svg
      viewBox="0 0 24 24"
      width="13"
      height="13"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M21 12a9 9 0 1 1-2.64-6.36L21 8" />
      <path d="M21 3v5h-5" />
    </svg>
  );
}
function IconCheck() {
  return (
    <svg
      viewBox="0 0 24 24"
      width="13"
      height="13"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
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
    icon: ReactNode;
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
        icon: (
          <span className={busy ? 'animate-spin' : undefined}>
            <IconRefresh />
          </span>
        ),
      })}
    </span>
  );
}
