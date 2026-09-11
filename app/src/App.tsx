import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Block, GDocument, ParseProgress, RenderMode, TranslateProgress } from './types';
import { parseDoc, parseDocUpload, translateDoc, fetchDocument, BackendError } from './services/backend';
import { fetchMe, logout } from './services/auth';
import { fetchHistory, type HistoryItem } from './services/history';
import Toolbar, { type TranslateButtonState } from './components/Toolbar';
import StatusBar from './components/StatusBar';
import DocumentViewer from './components/DocumentViewer';
import PdfCompareView from './components/PdfCompareView';
import LoginDialog from './components/LoginDialog';
import HistoryPanel from './components/HistoryPanel';
import type { ParagraphToolbarInteractive } from './components/BlockRenderer';
import { useReadingFontSize } from './hooks/useReadingFontSize';
import { paragraphPlainText } from './types';

export default function App() {
  const [filePath, setFilePath] = useState('');
  // 部署安全加固：上传文件（选择后优先于路径输入）
  const [uploadFile, setUploadFile] = useState<File | null>(null);
  const [doc, setDoc] = useState<GDocument | null>(null);
  const [translatedMap, setTranslatedMap] = useState<Record<string, Block | null>>({});
  const [sourceLang, setSourceLang] = useState('en');
  const [targetLang, setTargetLang] = useState('zh');
  const [parsing, setParsing] = useState(false);
  const [translating, setTranslating] = useState(false);
  const [progress, setProgress] = useState<TranslateProgress | null>(null);
  const [stats, setStats] = useState({ done: 0, cached: 0, errors: 0 });
  const [error, setError] = useState<string | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  // v2.1：翻译失败回退原文的块（右栏「未翻译」徽标）
  const [fallbackIds, setFallbackIds] = useState<Set<string>>(new Set());
  // v2.1：翻译完成态（按钮状态机 done 分支）
  const [lastDone, setLastDone] = useState(false);
  // v2：MinerU 解析进度 + 渲染模式
  const [parseProgress, setParseProgress] = useState<ParseProgress | null>(null);
  const [renderMode, setRenderMode] = useState<RenderMode>('original');
  // v2.3 C1：左栏 pdf.js 致命失败 → 自动降级文本对照 + 横幅重试
  const [pdfViewerError, setPdfViewerError] = useState<string | null>(null);

  // v2.4 T2：「我的文档」历史面板
  const [historyOpen, setHistoryOpen] = useState(false);
  const [historyItems, setHistoryItems] = useState<HistoryItem[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);

  // 部署安全加固：认证状态 + 登录对话框
  const [username, setUsername] = useState<string | null>(null);
  const [loginOpen, setLoginOpen] = useState(false);
  const [loginHint, setLoginHint] = useState<string | null>(null);
  // v2.4：服务器权限模式（require_login=1 时未登录拦截打开/翻译）
  const [requireLogin, setRequireLogin] = useState(false);
  // v2.4：登录后续跑的挂起动作（打开/翻译）
  const pendingActionRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    fetchMe()
      .then((me) => {
        setUsername(me.username);
        setRequireLogin(!!me.require_login);
      })
      .catch(() => setUsername(null));
  }, []);

  const handleLoggedIn = useCallback((user: string) => {
    setUsername(user);
    setLoginOpen(false);
    setLoginHint(null);
    setError(null);
    // 登录成功 → 自动续跑被拦截的动作（v2.4 T1）
    const pending = pendingActionRef.current;
    pendingActionRef.current = null;
    pending?.();
  }, []);

  const handleLogout = useCallback(async () => {
    await logout();
    setUsername(null);
  }, []);

  const openLogin = useCallback((hint?: string) => {
    setLoginHint(hint ?? null);
    setLoginOpen(true);
  }, []);

  // v2.4 T1：服务器要求登录且未登录 → 拦截动作，登录后自动续跑
  const requireAuth = useCallback(
    (action: () => void, hint?: string) => {
      if (!requireLogin || username) {
        action();
        return;
      }
      pendingActionRef.current = action;
      openLogin(hint ?? '登录后即可打开并翻译文档');
    },
    [requireLogin, username, openLogin],
  );

  // v2.3 C1：左栏致命失败（worker 加载等）→ 切文本对照（IR 已有原文文本，左栏不空白）
  const handleOriginalFatal = useCallback((msg: string) => {
    setPdfViewerError(msg);
    setRenderMode('text');
  }, []);

  // 429（未登录额度用尽）/ 401 → 提示并引导登录
  const reportError = useCallback(
    (e: unknown) => {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
      if (e instanceof BackendError && (e.status === 429 || e.status === 401)) {
        openLogin(message);
      }
    },
    [openLogin],
  );

  const handlePickFile = useCallback((file: File) => {
    setUploadFile(file);
    setFilePath(file.name);
    setError(null);
  }, []);

  const handleFilePathChange = useCallback((v: string) => {
    setFilePath(v);
    setUploadFile(null); // 手动编辑路径 = 放弃已选文件
  }, []);

  // v2.4：双栏滚动容器（useHoverDrivenSync 在两侧视图内接管同步）
  const originalRef = useRef<HTMLDivElement>(null);
  const translatedRef = useRef<HTMLDivElement>(null);
  const { fontSize, increase: fontIncrease, decrease: fontDecrease } = useReadingFontSize();

  // 来源文件类型（徽章）：优先上传文件名，其次路径扩展名；文档打开后保持
  const [fileType, setFileType] = useState<'docx' | 'pdf' | null>(null);

  const blockMap = useMemo(() => {
    const m: Record<string, Block> = {};
    if (doc) for (const b of doc.blocks) m[b.id] = b;
    return m;
  }, [doc]);

  async function runParse() {
    setParsing(true);
    setError(null);
    setParseProgress(null);
    try {
      const name = (uploadFile?.name ?? filePath).trim().toLowerCase();
      const d = uploadFile
        ? await parseDocUpload(uploadFile, 'auto', setParseProgress)
        : await parseDoc(filePath.trim(), 'auto', setParseProgress);
      setFileType(name.endsWith('.pdf') ? 'pdf' : 'docx');
      setDoc(d);
      setTranslatedMap({});
      setProgress(null);
      setStats({ done: 0, cached: 0, errors: 0 });
      // PDF 且带 pdf_meta → 默认原始渲染模式；否则文本对照
      setRenderMode(d.pdf_meta ? 'original' : 'text');
    } catch (e) {
      setDoc(null);
      reportError(e);
    } finally {
      setParsing(false);
    }
  }

  // v2.4：打开动作经登录拦截（require_login=1 时弹框，登录后续跑）
  const handleParse = useCallback(
    () => requireAuth(runParse, '登录后即可打开并翻译文档'),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [requireAuth, uploadFile, filePath],
  );

  async function runTranslate() {
    if (!doc) return;
    setTranslating(true);
    setError(null);
    setStats({ done: 0, cached: 0, errors: 0 });
    setProgress(null);
    setFallbackIds(new Set());
    setLastDone(false);
    try {
      const result = await translateDoc(doc.id, sourceLang, targetLang, (p) => {
        setProgress(p);
        if (p.status === 'done') setStats((s) => ({ ...s, done: s.done + 1 }));
        else if (p.status === 'cached') setStats((s) => ({ ...s, cached: s.cached + 1 }));
        else if (p.status === 'error') setStats((s) => ({ ...s, errors: s.errors + 1 }));

        const text = p.translated_text;
        if (text) {
          // 流式预览：先用纯文本回填，complete 后用权威结果（含行内资产）
          const orig = blockMap[p.block_id];
          const origStyle = orig?.type === 'paragraph' ? orig.style : {};
          setTranslatedMap((prev) => ({
            ...prev,
            [p.block_id]: {
              type: 'paragraph',
              id: p.block_id,
              style: origStyle,
              content: [{ type: 'text', text }],
            },
          }));
        }
      });
      const finalMap: Record<string, Block | null> = {};
      const fbIds = new Set<string>();
      for (const pair of result.pairs) {
        finalMap[pair.block_id] = pair.translated;
        if (pair.fallback) fbIds.add(pair.block_id);
      }
      setTranslatedMap(finalMap);
      setFallbackIds(fbIds);
      setLastDone(true);
    } catch (e) {
      reportError(e);
    } finally {
      setTranslating(false);
    }
  }

  // v2.4：翻译动作经登录拦截
  const handleTranslate = useCallback(
    () => requireAuth(runTranslate, '登录后即可翻译全文'),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [requireAuth, doc, blockMap, sourceLang, targetLang],
  );

  // v2.1 T7.2：单段重译回填（工具条回调）
  const handleRetranslated = useCallback(
    ({ blockId, block, fallback }: { blockId: string; block: Block; fallback: boolean }) => {
      setTranslatedMap((prev) => ({ ...prev, [blockId]: block }));
      setFallbackIds((prev) => {
        const next = new Set(prev);
        if (fallback) next.add(blockId);
        else next.delete(blockId);
        return next;
      });
    },
    [],
  );

  // v2.4 T2：打开/关闭历史面板（打开时拉取列表）
  const openHistory = useCallback(async () => {
    if (historyOpen) {
      setHistoryOpen(false);
      return;
    }
    setHistoryOpen(true);
    setHistoryLoading(true);
    try {
      setHistoryItems(await fetchHistory());
    } catch (e) {
      reportError(e);
      setHistoryOpen(false);
    } finally {
      setHistoryLoading(false);
    }
  }, [historyOpen, reportError]);

  // v2.4 T2：历史条目回载（无需重新上传）
  const openHistoryItem = useCallback(
    async (item: HistoryItem) => {
      try {
        const d = await fetchDocument(item.doc_id);
        setDoc(d);
        setFilePath(item.file_name);
        setUploadFile(null);
        setFileType(item.file_type === 'pdf' ? 'pdf' : 'docx');
        setTranslatedMap({});
        setProgress(null);
        setStats({ done: 0, cached: 0, errors: 0 });
        setLastDone(false);
        setRenderMode(d.pdf_meta ? 'original' : 'text');
        setHistoryOpen(false);
        setError(null);
      } catch (e) {
        reportError(e);
      }
    },
    [reportError],
  );

  // v2.1 T7.2：译文侧段落交互（hover 工具条）
  const interactive: ParagraphToolbarInteractive | undefined = useMemo(    () =>
      doc
        ? {
            docId: doc.id,
            translatedText: (blockId) => {
              const t = translatedMap[blockId];
              return t?.type === 'paragraph' ? paragraphPlainText(t.content) : null;
            },
            sourceLang,
            targetLang,
            onRetranslated: handleRetranslated,
          }
        : undefined,
    [doc, translatedMap, sourceLang, targetLang, handleRetranslated],
  );

  // v2.1 T7.1：翻译按钮状态机
  const buttonState: TranslateButtonState = translating
    ? { kind: 'translating', current: progress?.current ?? 0, total: progress?.total ?? 0 }
    : error
      ? { kind: 'error' }
      : lastDone
        ? { kind: 'done', errors: stats.errors }
        : { kind: 'idle' };

  return (
    <div className="relative flex h-full flex-col bg-slate-100 text-slate-800">
      <Toolbar
        filePath={filePath}
        onFilePathChange={handleFilePathChange}
        onParse={handleParse}
        parsing={parsing}
        parseProgress={parseProgress}
        renderMode={renderMode}
        onRenderModeChange={setRenderMode}
        canToggleRenderMode={!!doc?.pdf_meta && fileType === 'pdf'}
        docTitle={doc?.title ?? null}
        sourceLang={sourceLang}
        targetLang={targetLang}
        onSourceLangChange={setSourceLang}
        onTargetLangChange={setTargetLang}
        onTranslate={handleTranslate}
        translating={translating}
        canTranslate={doc !== null}
        fontSize={fontSize}
        onFontIncrease={fontIncrease}
        onFontDecrease={fontDecrease}
        buttonState={buttonState}
        onPickFile={handlePickFile}
        username={username}
        onLoginClick={() => openLogin()}
        onLogoutClick={handleLogout}
        onHistoryClick={username ? openHistory : undefined}
      />

      <div className="narrow-hint items-center justify-center gap-2 bg-amber-50 px-4 py-1 text-xs text-amber-700">
        窗口较窄，已自动缩小字号；建议全屏使用获得最佳阅读体验
      </div>

      {pdfViewerError ? (
        <div
          className="flex flex-wrap items-center gap-3 bg-red-50 px-4 py-1.5 text-xs text-red-700"
          data-testid="pdf-fallback-banner"
        >
          <span>原始渲染暂不可用（{pdfViewerError}），已切换为文本对照模式</span>
          <button
            className="rounded border border-red-300 px-2 py-0.5 hover:bg-red-100"
            onClick={() => {
              setPdfViewerError(null);
              setRenderMode('original');
            }}
          >
            重试原始渲染
          </button>
        </div>
      ) : null}

      {doc ? (
        renderMode === 'original' && doc.pdf_meta ? (
          <PdfCompareView
            doc={doc}
            translatedMap={translatedMap}
            activeId={activeId}
            onSelect={setActiveId}
            originalRef={originalRef}
            translatedRef={translatedRef}
            fallbackIds={fallbackIds}
            interactive={interactive}
            onOriginalFatalError={handleOriginalFatal}
          />
        ) : (
          <DocumentViewer
            doc={doc}
            fileType={fileType}
            translatedMap={translatedMap}
            activeId={activeId}
            onSelect={setActiveId}
            originalRef={originalRef}
            translatedRef={translatedRef}
            fallbackIds={fallbackIds}
            interactive={interactive}
          />
        )
      ) : (
        <main className="flex flex-1 flex-col items-center justify-center gap-2 text-slate-400">
          <div className="text-5xl">📄</div>
          <p className="text-sm">点击「📁 选择文件」上传 .docx / .pdf 文档</p>
          {error ? (
            <p className="max-w-md text-sm text-red-500" data-testid="empty-error">
              {error}
            </p>
          ) : null}
        </main>
      )}

      <StatusBar progress={progress} translating={translating} stats={stats} error={error} />

      {historyOpen ? (
        <HistoryPanel
          items={historyItems}
          loading={historyLoading}
          onOpen={openHistoryItem}
          onClose={() => setHistoryOpen(false)}
        />
      ) : null}

      {loginOpen ? (
        <LoginDialog
          hint={loginHint}
          onClose={() => {
            setLoginOpen(false);
            setLoginHint(null);
          }}
          onLoggedIn={handleLoggedIn}
        />
      ) : null}
    </div>
  );
}
