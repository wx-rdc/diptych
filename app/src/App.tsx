import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Block, GDocument, RenderMode, TranslateProgress } from './types';
import {
  translateDoc,
  fetchDocument,
  BackendError,
  QuotaLimitError,
  TranslateCancelledError,
  type QuotaInfo,
} from './services/backend';
import { fetchMe, logout } from './services/auth';
import { fetchHistory, type HistoryItem } from './services/history';
import Toolbar from './components/Toolbar';
import StatusBar from './components/StatusBar';
import DocumentViewer from './components/DocumentViewer';
import PdfCompareView from './components/PdfCompareView';
import LoginDialog from './components/LoginDialog';
import HistoryPanel from './components/HistoryPanel';
import QuotaExceededCard from './components/QuotaExceededCard';
import TranslateControl from './components/TranslateControl';
import type { ParagraphToolbarInteractive } from './components/BlockRenderer';
import { useReadingFontSize } from './hooks/useReadingFontSize';
import { useQuotaGuard } from './hooks/useQuotaGuard';
import { useAutoPipeline } from './hooks/useAutoPipeline';
import { paragraphPlainText } from './types';

export default function App() {
  const [filePath, setFilePath] = useState('');
  const [doc, setDoc] = useState<GDocument | null>(null);
  const [translatedMap, setTranslatedMap] = useState<Record<string, Block | null>>({});
  const [sourceLang, setSourceLang] = useState('en');
  const [targetLang, setTargetLang] = useState('zh');
  const [translating, setTranslating] = useState(false);
  const [progress, setProgress] = useState<TranslateProgress | null>(null);
  const [stats, setStats] = useState({ done: 0, cached: 0, errors: 0 });
  const [error, setError] = useState<string | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  // v2.1：翻译失败回退原文的块（右栏「未翻译」徽标）
  const [fallbackIds, setFallbackIds] = useState<Set<string>>(new Set());
  // v2.1：翻译完成态
  const [lastDone, setLastDone] = useState(false);
  // v2.4 T3/T6：熔断卡片（额度用尽；已译部分保留可读）
  const [limitInfo, setLimitInfo] = useState<QuotaInfo | null>(null);
  // v2.4 T3/T5：暂停态（用户停止 / 断开）→ [继续翻译]
  const [paused, setPaused] = useState(false);
  // v2.4 T3：自动翻译是否已对该文档执行过（仅首次自动，防循环）
  const autoTranslatedRef = useRef<string | null>(null);
  // v2：渲染模式
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

  // v2.4 T3：零点击流水线 + 本地配额预检
  const quotaGuard = useQuotaGuard();
  const pipeline = useAutoPipeline();

  useEffect(() => {
    fetchMe()
      .then((me) => setUsername(me.username))
      .catch(() => setUsername(null));
  }, []);

  const handleLoggedIn = useCallback((user: string) => {
    setUsername(user);
    setLoginOpen(false);
    setLoginHint(null);
    setError(null);
  }, []);

  const handleLogout = useCallback(async () => {
    await logout();
    setUsername(null);
  }, []);

  const openLogin = useCallback((hint?: string) => {
    setLoginHint(hint ?? null);
    setLoginOpen(true);
  }, []);

  // v2.3 C1：左栏致命失败（worker 加载等）→ 切文本对照（IR 已有原文文本，左栏不空白）
  const handleOriginalFatal = useCallback((msg: string) => {
    setPdfViewerError(msg);
    setRenderMode('text');
  }, []);

  const reportError = useCallback(
    (e: unknown) => {
      // v2.4 T3.5：解析预检 402 → QuotaExceededCard（结构化配额字段）
      if (e instanceof BackendError && e.quota) {
        setLimitInfo(e.quota);
        return;
      }
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
      if (e instanceof BackendError && (e.status === 401 || e.status === 403)) {
        openLogin(message);
      }
    },
    [openLogin],
  );

  const handleFilePathChange = useCallback((v: string) => {
    setFilePath(v);
  }, []);

  // v2.4：双栏滚动容器（useHoverDrivenSync 在两侧视图内接管同步）
  const originalRef = useRef<HTMLDivElement>(null);
  const translatedRef = useRef<HTMLDivElement>(null);
  const { fontSize, increase: fontIncrease, decrease: fontDecrease } = useReadingFontSize();

  // 来源文件类型（徽章）：优先上传文件名，其次路径扩展名；文档打开后保持
  const [fileType, setFileType] = useState<'docx' | 'pdf' | null>(null);

  /** 解析成功 → 应用文档状态（runParse/历史回载/零点击流水线共用） */
  const applyDoc = useCallback((d: GDocument, name: string) => {
    setDoc(d);
    setTranslatedMap({});
    setProgress(null);
    setStats({ done: 0, cached: 0, errors: 0 });
    setFallbackIds(new Set());
    setLastDone(false);
    setLimitInfo(null);
    setPaused(false);
    autoTranslatedRef.current = null;
    setFileType(name.toLowerCase().endsWith('.pdf') ? 'pdf' : 'docx');
    setRenderMode(d.pdf_meta ? 'original' : 'text');
  }, []);

  /** 翻译执行（零点击自动 / 继续 / 重新翻译共用） */
  const runTranslate = useCallback(
    async (target: GDocument) => {
      if (!target) return;
      setTranslating(true);
      setError(null);
      setLimitInfo(null);
      setPaused(false);
      setStats({ done: 0, cached: 0, errors: 0 });
      setProgress(null);
      setFallbackIds(new Set());
      setLastDone(false);
      const map = (() => {
        const m: Record<string, Block> = {};
        for (const b of target.blocks) m[b.id] = b;
        return m;
      })();
      try {
        const result = await translateDoc(target.id, sourceLang, targetLang, (p) => {
          setProgress(p);
          if (p.status === 'done') setStats((s) => ({ ...s, done: s.done + 1 }));
          else if (p.status === 'cached') setStats((s) => ({ ...s, cached: s.cached + 1 }));
          else if (p.status === 'error') setStats((s) => ({ ...s, errors: s.errors + 1 }));

          const text = p.translated_text;
          if (text) {
            // 流式预览：先用纯文本回填，complete 后用权威结果（含行内资产）
            const orig = map[p.block_id];
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
        if (e instanceof QuotaLimitError) {
          // 熔断：已译部分（translatedMap）保留可读，QuotaExceededCard 引导登录
          setLimitInfo(e.quota);
        } else if (e instanceof TranslateCancelledError) {
          // 暂停态：[继续翻译] 走缓存秒回 + 增量续译
          setPaused(true);
        } else {
          reportError(e);
        }
      } finally {
        setTranslating(false);
      }
    },
    [sourceLang, targetLang, reportError],
  );

  // v2.4 T3 零点击：选择文件 → 本地预检 → 自动解析 → 自动翻译
  const handlePickFile = useCallback(
    async (file: File) => {
      setFilePath(file.name);
      setError(null);
      pipeline.reset();
      try {
        const d = await pipeline.runUpload(file, 'auto', () => quotaGuard.check(file));
        if (!d) {
          // 预检拦截：不上传，引导登录
          if (quotaGuard.result.blocked) {
            setError(quotaGuard.result.message);
          }
          return;
        }
        applyDoc(d, file.name);
        if (autoTranslatedRef.current !== d.id) {
          autoTranslatedRef.current = d.id;
          await runTranslate(d);
        }
      } catch (e) {
        reportError(e);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [pipeline, quotaGuard, applyDoc, runTranslate, reportError],
  );

  // 本地路径开发分支（Enter 触发同款零点击流水线）
  const handleParseLocal = useCallback(async () => {
    const path = filePath.trim();
    if (!path) return;
    setError(null);
    pipeline.reset();
    try {
      const d = await pipeline.runPath(path, 'auto', (name) =>
        quotaGuard.check(new File([], name || path)),
      );
      if (!d) return;
      applyDoc(d, path);
      if (autoTranslatedRef.current !== d.id) {
        autoTranslatedRef.current = d.id;
        await runTranslate(d);
      }
    } catch (e) {
      reportError(e);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filePath, pipeline, quotaGuard, applyDoc, runTranslate, reportError]);

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

  // v2.4 T2：历史条目回载（无需重新上传；登录用户自动续译）
  const openHistoryItem = useCallback(
    async (item: HistoryItem) => {
      try {
        const d = await fetchDocument(item.doc_id);
        applyDoc(d, item.file_name);
        setFilePath(item.file_name);
        setHistoryOpen(false);
        if (autoTranslatedRef.current !== d.id) {
          autoTranslatedRef.current = d.id;
          await runTranslate(d);
        }
      } catch (e) {
        reportError(e);
      }
    },
    [applyDoc, runTranslate, reportError],
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

  return (
    <div className="relative flex h-full flex-col bg-slate-100 text-slate-800">
      <Toolbar
        filePath={filePath}
        onFilePathChange={handleFilePathChange}
        onParseLocal={handleParseLocal}
        parseProgress={pipeline.state.parseProgress}
        renderMode={renderMode}
        onRenderModeChange={setRenderMode}
        canToggleRenderMode={!!doc?.pdf_meta && fileType === 'pdf'}
        docTitle={doc?.title ?? null}
        sourceLang={sourceLang}
        targetLang={targetLang}
        onSourceLangChange={setSourceLang}
        onTargetLangChange={setTargetLang}
        fontSize={fontSize}
        onFontIncrease={fontIncrease}
        onFontDecrease={fontDecrease}
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
          <p className="text-sm">点击「📁 选择文件」——自动解析并翻译，无需其他操作</p>
          {error ? (
            <p className="max-w-md text-sm text-red-500" data-testid="empty-error">
              {error}
            </p>
          ) : null}
        </main>
      )}

      <StatusBar progress={progress} translating={translating} stats={stats} error={error} />

      {/* v2.4 T5/T6：翻译控制（进度 + 停止/继续/重新翻译） */}
      {doc ? (
        <TranslateControl
          doc={doc}
          translating={translating}
          paused={paused}
          done={lastDone}
          progress={progress}
          onContinue={() => doc && runTranslate(doc)}
          onRetranslate={() => doc && runTranslate(doc)}
        />
      ) : null}

      {/* v2.4 T6：额度用尽引导卡片（已译部分保留可读） */}
      {limitInfo ? (
        <QuotaExceededCard
          quota={limitInfo}
          onLogin={() => openLogin('登录后不限量翻译；登录成功后可继续翻译本文档')}
          onDismiss={() => setLimitInfo(null)}
        />
      ) : null}

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
