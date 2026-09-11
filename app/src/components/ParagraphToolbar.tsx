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

/** 段落 hover 工具条（v2.1 T7.2）：[复制原文][复制译文][重译本段]，group-hover 显示 */
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
    'rounded border border-slate-200 bg-white px-1.5 py-0.5 text-[10px] text-slate-500 shadow-sm hover:border-blue-300 hover:text-blue-600';

  return (
    <span
      className="absolute -top-2 right-0 z-10 hidden items-center gap-1 group-hover:flex"
      data-testid={`paragraph-toolbar-${block.id}`}
    >
      {copied ? <span className="text-[10px] text-emerald-600">{copied}已复制</span> : null}
      <button className={btn} data-testid="copy-source-btn" onClick={() => void copy('原文', sourceText)}>
        复制原文
      </button>
      <button
        className={btn}
        data-testid="copy-target-btn"
        onClick={() => void copy('译文', translatedText ?? sourceText)}
      >
        复制译文
      </button>
      <button className={btn} data-testid="retranslate-btn" disabled={busy} onClick={() => void retranslate()}>
        {busy ? '重译中…' : '重译本段'}
      </button>
    </span>
  );
}
