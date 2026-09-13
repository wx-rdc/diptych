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
          className={'flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-sm text-slate-500 hover:bg-slate-100 hover:text-slate-700 ' + (isOpen ? 'bg-slate-100 text-slate-700' : '')}
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
              onKeyDown={(e) => { if (e.key === 'Enter' && e.currentTarget.value.trim()) p.onParseLocal(); }}
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
