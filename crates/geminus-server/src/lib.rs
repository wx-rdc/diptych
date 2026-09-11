//! geminus-server：axum HTTP+SSE 壳（docs/design.md §4）
//!
//! 将 plan.md §5 的 Tauri IPC 语义 1:1 映射为 HTTP：
//! - parse_docx           → POST /api/parse（JSON 本地路径 | multipart 上传）
//! - translate_document   → POST /api/translate（SSE：progress → complete/error）
//! - get_resource         → GET /api/resource/{doc_id}/{rid}
//!
//! 部署安全加固（docs/deploy_plan.md）：
//! - 认证：POST /api/auth/login|logout、GET /api/auth/me（预设用户，无自助注册）
//! - 限流：未登录按 IP 累计「查看+翻译」5 页（SQLite 持久）；登录用户不受限
//! - 本地路径解析默认禁用（GEMINUS_ALLOW_LOCAL_PATH=1 开启，供本地开发）

pub mod access;
pub mod auth;
pub mod history;
pub mod quota;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use geminus_core::mineru::{self, MineruClient, MineruConfig, SplitConfig};

use axum::extract::{FromRequest, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use geminus_core::cache::TranslationCache;
use geminus_core::model::{Document, TranslatedDocument};
use geminus_core::translate::{translate_document, LlmClient};
use geminus_core::ApiConfig;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;

use crate::auth::AuthService;
use crate::quota::{QuotaStore, RateWindow};

/// 应用状态（Clone 共享）
#[derive(Clone)]
pub struct AppState {
    /// 文档仓：doc_id → Document
    docs: Arc<Mutex<HashMap<String, Document>>>,
    /// 原始 PDF 路径：doc_id → 文件路径（原始渲染模式供前端取文件）
    original_pdfs: Arc<Mutex<HashMap<String, PathBuf>>>,
    /// 译文仓：doc_id → 最近一次完整翻译结果（对齐审计用）
    translations: Arc<Mutex<HashMap<String, TranslatedDocument>>>,
    /// 源文件内容哈希：doc_id → sha256（匿名计费去重用）
    file_hashes: Arc<Mutex<HashMap<String, String>>>,
    cache: Arc<TranslationCache>,
    api_base: String,
    api_key: String,
    model: String,
    concurrency: usize,
    /// MinerU 客户端（Token 未配置时为 None → PDF 走 builtin）
    mineru: Arc<Mutex<Option<MineruClient>>>,
    /// 认证服务（未配置预设用户时登录不可用）
    auth: Arc<AuthService>,
    /// 匿名页数配额（未登录按 IP）
    quota: Arc<QuotaStore>,
    /// 匿名单段重译频次窗口（二级防线）
    paragraph_rate: RateWindow,
    /// 上传落盘目录
    uploads_dir: PathBuf,
    /// v2.2 T1：MinerU raw 缓存根目录（离线重放 / 覆盖度审计）
    mineru_cache_root: PathBuf,
    /// 上传大小上限（字节）
    upload_limit: usize,
    /// 是否允许 JSON file_path 分支（本地开发用；生产默认禁用）
    allow_local_path: bool,
    /// v2.4：权限收口（查看/翻译强制登录；上传仍匿名+配额）。
    /// v2.4 新迭代默认关闭：匿名在配额内开放试用（access_token 防盗链，见 access.rs）
    require_login: bool,
    /// v2.4：用户文档历史
    history: Arc<history::HistoryStore>,
    /// v2.4：文档访问凭证（匿名防盗链三选一）
    doc_access: Arc<access::DocAccessStore>,
}

impl AppState {
    /// 测试/开发构造：本地路径可用、认证关闭、配额走内存库
    pub fn new(cache: Arc<TranslationCache>, cfg: ApiConfig, concurrency: usize) -> Self {
        Self {
            docs: Arc::new(Mutex::new(HashMap::new())),
            original_pdfs: Arc::new(Mutex::new(HashMap::new())),
            translations: Arc::new(Mutex::new(HashMap::new())),
            file_hashes: Arc::new(Mutex::new(HashMap::new())),
            cache,
            api_base: cfg.api_base,
            api_key: cfg.api_key,
            model: cfg.model,
            concurrency,
            // 显式构造不自动读 env（测试确定性）；生产入口 from_env 配置
            mineru: Arc::new(Mutex::new(None)),
            auth: Arc::new(AuthService::disabled(Duration::from_secs(7 * 24 * 3600))),
            quota: Arc::new(QuotaStore::open_in_memory(5).expect("配额内存库初始化失败")),
            paragraph_rate: RateWindow::new(30, Duration::from_secs(3600)),
            uploads_dir: std::env::temp_dir().join("geminus-uploads"),
            mineru_cache_root: PathBuf::from("data/mineru_cache"),
            upload_limit: 200 * 1024 * 1024,
            allow_local_path: true,
            require_login: false,
            history: Arc::new(history::HistoryStore::open_in_memory().expect("历史内存库初始化失败")),
            doc_access: Arc::new(access::DocAccessStore::open_in_memory().expect("访问凭证内存库初始化失败")),
        }
    }

    /// 测试用：raw 缓存根目录隔离（避免污染 data/）
    pub fn with_mineru_cache_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.mineru_cache_root = root.into();
        self
    }

    /// 测试用：注入 MinerU 客户端（指向 mock 上游）
    pub fn with_mineru(self, client: Option<MineruClient>) -> Self {
        *self.mineru.lock().unwrap() = client;
        self
    }

    /// 测试用：注入认证服务（预设用户 + 会话）
    pub fn with_auth(self, auth: AuthService) -> Self {
        let mut s = self;
        s.auth = Arc::new(auth);
        s
    }

    /// 测试用：注入配额仓
    pub fn with_quota(self, quota: QuotaStore) -> Self {
        let mut s = self;
        s.quota = Arc::new(quota);
        s
    }

    /// 测试用：开关本地路径分支
    pub fn with_allow_local_path(self, allow: bool) -> Self {
        let mut s = self;
        s.allow_local_path = allow;
        s
    }

    /// 测试用：注入历史仓
    pub fn with_history(mut self, history: Arc<history::HistoryStore>) -> Self {
        self.history = history;
        self
    }

    /// 测试用：查看/翻译强制登录
    pub fn with_require_login(mut self, require: bool) -> Self {
        self.require_login = require;
        self
    }

    /// 测试用：注入文档访问凭证仓
    pub fn with_doc_access(mut self, store: Arc<access::DocAccessStore>) -> Self {
        self.doc_access = store;
        self
    }

    fn mineru_client(&self) -> Option<MineruClient> {
        self.mineru.lock().unwrap().clone()
    }

    /// 从环境变量构造（.env.local → env → 默认值）。生产默认禁用本地路径解析。
    pub fn from_env() -> Self {
        let _ = geminus_core::ApiConfig::from_env(); // 触发 .env.local 加载
        let cfg = ApiConfig::from_env();
        let env_flag = |k: &str| std::env::var(k).map(|v| v == "1").unwrap_or(false);
        let cache_path = std::env::var("GEMINUS_CACHE_PATH")
            .unwrap_or_else(|_| "data/translate_cache.db".to_string());
        let cache = Arc::new(TranslationCache::open(&PathBuf::from(&cache_path)).expect("缓存库打开失败"));
        let concurrency = std::env::var("GEMINUS_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3);

        let anon_limit = std::env::var("GEMINUS_ANON_PAGE_LIMIT")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(5);
        let quota_db = std::env::var("GEMINUS_AUTH_DB")
            .unwrap_or_else(|_| "data/auth_quota.db".to_string());
        let quota = QuotaStore::open_at(&PathBuf::from(&quota_db), anon_limit)
            .unwrap_or_else(|e| panic!("配额库 {quota_db} 打开失败: {e}"));

        let users_file = std::env::var("GEMINUS_USERS_FILE")
            .unwrap_or_else(|_| "config/users.json".to_string());
        let auth = match auth::UserStore::load(std::path::Path::new(&users_file)) {
            Ok(store) => {
                eprintln!("[auth] 已加载预设用户: {users_file}");
                AuthService::new(store, session_ttl(), env_flag("GEMINUS_COOKIE_SECURE"))
            }
            Err(e) => {
                eprintln!("[auth] 预设用户未配置（{users_file}: {e}）→ 登录不可用");
                AuthService::disabled(session_ttl())
            }
        };

        let uploads_dir = std::env::var("GEMINUS_UPLOADS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("uploads"));
        let upload_limit = std::env::var("GEMINUS_MAX_UPLOAD_MB")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(200)
            * 1024
            * 1024;

        let state = Self::new(cache, cfg, concurrency)
            .with_auth(auth)
            .with_quota(quota);
        let mut state = state;
        state.allow_local_path = env_flag("GEMINUS_ALLOW_LOCAL_PATH");
        state.uploads_dir = uploads_dir;
        state.mineru_cache_root = PathBuf::from(
            std::env::var("GEMINUS_MINERU_CACHE_DIR").unwrap_or_else(|_| "data/mineru_cache".into()),
        );
        state.upload_limit = upload_limit;
        let history_db = std::env::var("GEMINUS_HISTORY_DB")
            .unwrap_or_else(|_| "data/history.db".to_string());
        let history = history::HistoryStore::open_at(&PathBuf::from(&history_db))
            .unwrap_or_else(|e| panic!("历史库 {history_db} 打开失败: {e}"));
        state.require_login = env_flag("GEMINUS_REQUIRE_LOGIN");
        state.history = Arc::new(history);
        let access_db = std::env::var("GEMINUS_ACCESS_DB")
            .unwrap_or_else(|_| "data/doc_access.db".to_string());
        let doc_access = access::DocAccessStore::open_at(&PathBuf::from(&access_db))
            .unwrap_or_else(|e| panic!("访问凭证库 {access_db} 打开失败: {e}"));
        state.doc_access = Arc::new(doc_access);
        *state.mineru.lock().unwrap() = MineruClient::from_env();
        state
    }

    fn get_doc(&self, doc_id: &str) -> Option<Document> {
        self.docs.lock().unwrap().get(doc_id).cloned()
    }

    fn get_translation(&self, doc_id: &str) -> Option<TranslatedDocument> {
        self.translations.lock().unwrap().get(doc_id).cloned()
    }

    /// doc_id → 源文件内容哈希（缺失时从原始文件补算，再缺失退化为 doc_id）
    fn content_hash_of(&self, doc_id: &str) -> String {
        if let Some(h) = self.file_hashes.lock().unwrap().get(doc_id) {
            return h.clone();
        }
        let path = self.original_pdfs.lock().unwrap().get(doc_id).cloned();
        if let Some(h) = path.and_then(|p| TranslationCache::file_sha256(&p).ok()) {
            self.file_hashes.lock().unwrap().insert(doc_id.to_string(), h.clone());
            return h;
        }
        doc_id.to_string()
    }

    /// 计费页数：MinerU 的 pdf_meta 真实页数 → builtin PDF 的 lopdf 页数 → DOCX 字符估算
    fn chargeable_pages(&self, doc: &Document, path: Option<&std::path::Path>) -> i64 {
        if let Some(meta) = &doc.pdf_meta {
            return meta.page_count.max(1) as i64;
        }
        let path = path
            .map(|p| p.to_path_buf())
            .or_else(|| self.original_pdfs.lock().unwrap().get(&doc.id).cloned());
        if let Some(p) = path {
            if matches!(detect_file_type(&p), Ok(FileType::Pdf)) {
                if let Ok(n) = geminus_core::pdf::count_pages(&p) {
                    return n.max(1) as i64;
                }
            }
        }
        doc.estimated_pages() as i64
    }

    /// 单段重译后同步内存译文（保持 /api/debug/audit 一致性）
    fn update_translation_block(&self, doc_id: &str, pair: geminus_core::model::TranslatedBlock) {
        let mut map = self.translations.lock().unwrap();
        if let Some(td) = map.get_mut(doc_id) {
            if let Some(p) = td.pairs.iter_mut().find(|p| p.block_id == pair.block_id) {
                *p = pair;
            }
        }
    }
}

fn session_ttl() -> Duration {
    let hours = std::env::var("GEMINUS_SESSION_TTL_HOURS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(7 * 24);
    Duration::from_secs(hours * 3600)
}

/// 客户端 IP：生产环境由 nginx 覆盖式注入 X-Real-IP（或 X-Forwarded-For 首段）。
/// 无代理头 = 直连本机（本地开发 / E2E）→ None，匿名配额不生效。
fn client_ip(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.trim().is_empty())
    {
        return Some(v.trim().to_string());
    }
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 当前登录用户（Cookie 会话）
fn current_user(state: &AppState, headers: &HeaderMap) -> Option<String> {
    let cookie = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok());
    state.auth.session_user(cookie)
}

/// v2.4 权限收口：require_login 开启时，查看/翻译类端点匿名一律 401
fn require_login_guard(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    if state.require_login && current_user(state, headers).is_none() {
        return Some(err_response(StatusCode::UNAUTHORIZED, "请登录后查看与翻译文档"));
    }
    None
}

/// v2.4 新迭代 T1：文档防盗链「三选一」校验（require_login 关闭时生效）：
/// ① 登录 Session 且历史关联该文档 ② access_token 匹配 ③ 上传者 IP 匹配。
/// 直连本机（无代理头）= 本地开发流量 → 放行（与匿名配额口径一致）。
/// 返回 Some(resp) = 拦截（403）；None = 放行。
fn doc_access_guard(
    state: &AppState,
    headers: &HeaderMap,
    doc_id: &str,
    token: Option<&str>,
) -> Option<Response> {
    if let Some(resp) = require_login_guard(state, headers) {
        return Some(resp);
    }
    // ① 登录用户（历史关联：自己上传过，或历史仓可回源）
    if current_user(state, headers).is_some() {
        return None;
    }
    // ② token 匹配
    if token.is_some_and(|t| state.doc_access.verify_token(doc_id, t)) {
        return None;
    }
    // ③ 上传者 IP 匹配；直连本机（None）放行
    let Some(ip) = client_ip(headers) else {
        return None;
    };
    if state.doc_access.uploader_ip(doc_id).as_deref() == Some(ip.as_str()) {
        return None;
    }
    Some(err_response(StatusCode::FORBIDDEN, "无权访问该文档（请登录，或使用上传时的浏览器打开）"))
}

/// 匿名上传成功的外壳响应（不含 IR：blocks/resources/render_meta/pdf_meta）
fn doc_shell(doc: &Document) -> serde_json::Value {
    json!({ "id": doc.id, "title": doc.title, "estimated_pages": doc.estimated_pages() })
}

#[derive(Deserialize)]
struct ParseRequest {
    file_path: String,
    /// 解析器选择："" / "auto"（默认，MinerU 失败回退 builtin）| "mineru" | "builtin"
    #[serde(default)]
    parser: String,
}

/// 支持的文档类型（magic bytes + 扩展名双重判定）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Docx,
    Pdf,
}

impl FileType {
    fn label(self) -> &'static str {
        match self {
            FileType::Docx => "docx",
            FileType::Pdf => "pdf",
        }
    }
}

/// 按扩展名 + magic bytes 识别文档类型
///
/// 返回 Err(msg) = 不支持的格式（400）；PDF 加密返回专门标记（422）。
pub fn detect_file_type(path: &std::path::Path) -> Result<FileType, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !matches!(ext.as_str(), "docx" | "pdf") {
        return Err(format!("不支持的文件格式 .{ext}（支持 .docx / .pdf）"));
    }
    // magic bytes 校验，防止伪扩展名（文件缺失时交给解析器报错，保持 422 语义）
    use std::io::Read as _;
    let mut magic = [0u8; 5];
    let has_magic = std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut magic))
        .is_ok_and(|n| n >= 2);
    let known = if !has_magic {
        return Ok(match ext.as_str() {
            "pdf" => FileType::Pdf,
            _ => FileType::Docx,
        });
    } else if magic.starts_with(b"%PDF") {
        Some(FileType::Pdf)
    } else if magic.starts_with(b"PK") {
        Some(FileType::Docx)
    } else {
        None
    };
    match known {
        Some(t) if matches!((t, ext.as_str()), (FileType::Pdf, "pdf") | (FileType::Docx, "docx")) => Ok(t),
        _ => Err("文件内容与扩展名不符（PDF 应以 %PDF 开头，DOCX 应为 ZIP）".to_string()),
    }
}

#[derive(Debug)]
enum ParseOutcome {
    Ok(Document, FileType),
    Unsupported(String),
    Encrypted,
    Failed(String),
}

/// 按文件类型分发到对应解析器（v1_plan T8.1）
fn parse_any(path: &std::path::Path) -> ParseOutcome {
    match detect_file_type(path) {
        Err(msg) => ParseOutcome::Unsupported(msg),
        Ok(FileType::Docx) => match geminus_core::parse_docx(path) {
            Ok(doc) => ParseOutcome::Ok(doc, FileType::Docx),
            Err(e) => ParseOutcome::Failed(e.to_string()),
        },
        Ok(FileType::Pdf) => match geminus_core::parse_pdf(path) {
            Ok(doc) => ParseOutcome::Ok(doc, FileType::Pdf),
            Err(geminus_core::PdfError::Encrypted) => ParseOutcome::Encrypted,
            Err(e) => ParseOutcome::Failed(e.to_string()),
        },
    }
}

#[derive(Deserialize)]
struct TranslateRequest {
    doc_id: String,
    #[serde(default = "default_source_lang")]
    source_lang: String,
    #[serde(default = "default_target_lang")]
    target_lang: String,
    /// v2.4 T1：匿名文档访问凭证（上传响应下发）
    #[serde(default)]
    access_token: Option<String>,
}

fn default_source_lang() -> String {
    "en".to_string()
}
fn default_target_lang() -> String {
    "zh".to_string()
}

fn err_response(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(json!({ "error": msg.into() }))).into_response()
}

// ────────────────────────── handlers ──────────────────────────

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

// ────────────────────────── 认证（部署安全加固） ──────────────────────────

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

async fn login_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Response {
    if !state.auth.is_enabled() {
        return err_response(StatusCode::SERVICE_UNAVAILABLE, "登录未启用（服务器未配置预设用户）");
    }
    let ip = client_ip(&headers).unwrap_or_else(|| "local".to_string());
    match state.auth.login(&ip, req.username.trim(), &req.password) {
        Ok(cookie) => (
            StatusCode::OK,
            [("set-cookie", cookie)],
            Json(json!({ "ok": true, "username": req.username.trim() })),
        )
            .into_response(),
        Err(auth::AuthError::Throttled) => {
            err_response(StatusCode::TOO_MANY_REQUESTS, "尝试过于频繁，请 10 分钟后再试")
        }
        Err(_) => err_response(StatusCode::UNAUTHORIZED, "用户名或密码错误"),
    }
}

async fn logout_handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let cookie = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok());
    let clear = state.auth.logout(cookie);
    (StatusCode::OK, [("set-cookie", clear)], Json(json!({ "ok": true }))).into_response()
}

async fn me_handler(State(state): State<AppState>, headers: HeaderMap) -> Json<serde_json::Value> {
    match current_user(&state, &headers) {
        Some(username) => Json(json!({
            "authenticated": true, "username": username,
            "login_enabled": state.auth.is_enabled(), "require_login": state.require_login })),
        None => Json(json!({
            "authenticated": false, "username": null,
            "login_enabled": state.auth.is_enabled(), "require_login": state.require_login })),
    }
}

// ────────────────────────── 解析（JSON 路径 | multipart 上传） ──────────────────────────

/// POST /api/parse：按 Content-Type 分发
/// - multipart/form-data：公网用户上传（file + 可选 parser 字段），落盘 uploads 后解析
/// - application/json：本地路径分支（GEMINUS_ALLOW_LOCAL_PATH 门控，本地开发用）
async fn parse_docx_handler(State(state): State<AppState>, req: axum::extract::Request) -> Response {
    let ct = req
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ct.starts_with("multipart/form-data") {
        parse_upload_handler(state, req).await
    } else {
        parse_path_handler(state, req).await
    }
}

async fn parse_path_handler(state: AppState, req: axum::extract::Request) -> Response {
    if !state.allow_local_path {
        return err_response(
            StatusCode::FORBIDDEN,
            "服务器已禁用本地路径解析，请改用文件上传",
        );
    }
    let headers = req.headers().clone();
    let body = match Json::<ParseRequest>::from_request(req, &()).await {
        Ok(b) => b,
        Err(e) => return err_response(StatusCode::BAD_REQUEST, format!("请求体无效: {e}")),
    };
    parse_from_path(&state, PathBuf::from(body.file_path.trim()), &body.parser, &headers, None).await
}

/// 上传文件名扩展名白名单（防路径注入：存储名由服务端生成）
fn safe_ext(filename: &str) -> Option<String> {
    let ext = filename.rsplit('.').next()?.to_ascii_lowercase();
    matches!(ext.as_str(), "docx" | "pdf").then_some(ext)
}

async fn parse_upload_handler(state: AppState, req: axum::extract::Request) -> Response {
    let headers = req.headers().clone();
    let mut mp = match axum::extract::Multipart::from_request(req, &()).await {
        Ok(mp) => mp,
        Err(e) => return err_response(StatusCode::BAD_REQUEST, format!("上传格式无效: {e}")),
    };

    let mut file_name: Option<String> = None;
    let mut file_bytes: Option<axum::body::Bytes> = None;
    let mut parser = "auto".to_string();
    loop {
        let field = match mp.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => return err_response(StatusCode::BAD_REQUEST, format!("上传读取失败: {e}")),
        };
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "file" => {
                file_name = field.file_name().map(|s| s.to_string());
                match field.bytes().await {
                    Ok(b) => file_bytes = Some(b),
                    Err(e) => return err_response(StatusCode::BAD_REQUEST, format!("上传读取失败: {e}")),
                }
            }
            "parser" => match field.text().await {
                Ok(t) => parser = t,
                Err(e) => return err_response(StatusCode::BAD_REQUEST, format!("parser 字段无效: {e}")),
            },
            _ => {}
        }
    }

    let (Some(name), Some(bytes)) = (file_name, file_bytes) else {
        return err_response(StatusCode::BAD_REQUEST, "缺少 file 字段（请选择 .docx / .pdf 文件）");
    };
    let Some(ext) = safe_ext(&name) else {
        return err_response(StatusCode::BAD_REQUEST, "不支持的文件格式（支持 .docx / .pdf）");
    };
    if bytes.len() > state.upload_limit {
        return err_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("文件超出上传上限（{} MB）", state.upload_limit / 1024 / 1024),
        );
    }

    // 落盘（存储名服务端生成；magic bytes 由 detect_file_type 二次校验）
    if let Err(e) = std::fs::create_dir_all(&state.uploads_dir) {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("uploads 目录不可用: {e}"));
    }
    let saved = state.uploads_dir.join(format!("{}.{}", uuid::Uuid::new_v4().simple(), ext));
    if let Err(e) = std::fs::write(&saved, &bytes) {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("写入失败: {e}"));
    }
    eprintln!("[upload] {} → {}（{} 字节）", name, saved.display(), bytes.len());

    parse_from_path(&state, saved, &parser, &headers, Some(name.as_str())).await
}

/// 匿名配额预检：PDF 在解析前用真实页数判断（保护 MinerU 云端额度不被超限请求消耗）
fn anon_precheck(
    state: &AppState,
    ip: Option<&str>,
    path: &std::path::Path,
) -> Result<(), Box<Response>> {
    let Some(ip) = ip else { return Ok(()) };
    if matches!(detect_file_type(path), Ok(FileType::Pdf)) {
        if let Ok(pages) = geminus_core::pdf::count_pages(path) {
            if state.quota.would_exceed(ip, pages as i64) {
                let used = state.quota.used_pages(ip);
                return Err(Box::new(err_response(
                    StatusCode::TOO_MANY_REQUESTS,
                    format!(
                        "未登录用户最多查看并翻译 {} 页（当前已用 {} 页）。请登录后继续使用。",
                        state.quota.limit(),
                        used
                    ),
                )));
            }
        }
    }
    Ok(())
}

/// 计费并登记哈希；Err(msg) = 超限（调用方按自身协议返回 429 JSON 或 SSE error 事件）
fn charge_anon(state: &AppState, ip: Option<&str>, hash: &str, pages: i64) -> Result<(), String> {
    if let Some(ip) = ip {
        if let Err(e) = state.quota.charge(ip, hash, pages) {
            return Err(e.message());
        }
    }
    Ok(())
}

/// 解析主流程（两个入口共用）：quota 预检 → builtin JSON / MinerU SSE → 成功后计费注册。
/// file_name：上传原名（JSON 本地路径分支传 None → 取文件主干）。
async fn parse_from_path(
    state: &AppState,
    path: PathBuf,
    parser_req: &str,
    headers: &HeaderMap,
    file_name: Option<&str>,
) -> Response {
    let parser = match parser_req {
        "" | "auto" => "auto",
        "mineru" => "mineru",
        "builtin" => "builtin",
        other => {
            return err_response(
                StatusCode::BAD_REQUEST,
                format!("未知 parser: {other}（支持 auto/mineru/builtin）"),
            );
        }
    };

    let user = current_user(state, headers);
    let ip = if user.is_some() { None } else { client_ip(headers) };
    if let Err(resp) = anon_precheck(state, ip.as_deref(), &path) {
        return *resp;
    }
    // v2.4：require_login 下匿名上传成功只回外壳（无 IR）；历史登记用的展示名
    let display_name = file_name
        .map(|s| s.to_string())
        .unwrap_or_else(|| file_stem(&path));
    let anon_shell = state.require_login && user.is_none();

    // 非 MinerU 路径：同步 JSON（v1 行为）
    let use_mineru = parser != "builtin"
        && matches!(detect_file_type(&path), Ok(FileType::Pdf))
        && state.mineru_client().is_some();
    if !use_mineru {
        return match parse_any(&path) {
            ParseOutcome::Ok(doc, ft) => {
                let hash = TranslationCache::file_sha256(&path).unwrap_or_else(|_| doc.id.clone());
                let pages = state.chargeable_pages(&doc, Some(&path));
                if let Some(msg) = charge_anon(state, ip.as_deref(), &hash, pages).err() {
                    return err_response(StatusCode::TOO_MANY_REQUESTS, msg);
                }
                eprintln!("[parse] {} [{}] {} 块", ft.label(), path.display(), doc.blocks.len());
                register_doc(state, &doc, &path, &hash, user.as_deref(), &display_name);
                let token = issue_doc_access(state, &doc.id, ip.as_deref(), user.is_some());
                let mut body = if anon_shell {
                    doc_shell(&doc)
                } else {
                    serde_json::to_value(&doc).unwrap_or_default()
                };
                body["access_token"] = json!(token);
                (StatusCode::OK, Json(body)).into_response()
            }
            ParseOutcome::Unsupported(msg) => err_response(StatusCode::BAD_REQUEST, msg),
            ParseOutcome::Encrypted => err_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "加密 PDF 暂不支持，请先解密后重试",
            ),
            ParseOutcome::Failed(msg) => err_response(StatusCode::UNPROCESSABLE_ENTITY, msg),
        };
    }

    // MinerU 路径：SSE 流（progress* → done | error）
    let Some(client) = state.mineru_client() else {
        return err_response(StatusCode::SERVICE_UNAVAILABLE, "MinerU 未配置（缺少 MINERU_API_KEY）");
    };
    let cache = state.cache.clone();
    let state2 = state.clone();
    let allow_fallback = parser == "auto";
    let ip2 = ip.clone();
    let user2 = user.clone();
    let display_name2 = display_name.clone();

    let (tx, rx) = mpsc::channel::<Result<Event, std::convert::Infallible>>(256);
    tokio::spawn(async move {
        // 进度转发任务：parse 侧经 channel 送来，这里并发排空（v1 T12 背压教训）
        let (stage_tx, mut stage_rx) = mpsc::channel::<serde_json::Value>(256);
        let forwarder = {
            let tx = tx.clone();
            tokio::spawn(async move {
                while let Some(payload) = stage_rx.recv().await {
                    let _ = tx
                        .send(Ok(Event::default().event("progress").data(payload.to_string())))
                        .await;
                }
            })
        };

        async fn send_error(tx: &mpsc::Sender<Result<Event, std::convert::Infallible>>, code: &str, message: String) {
            let _ = tx
                .send(Ok(Event::default()
                    .event("error")
                    .data(json!({ "code": code, "message": message }).to_string())))
                .await;
        }
        async fn send_done(
            tx: &mpsc::Sender<Result<Event, std::convert::Infallible>>,
            doc: &Document,
            shell: bool,
            token: &str,
        ) {
            let mut data = if shell {
                doc_shell(doc)
            } else {
                serde_json::to_value(doc).unwrap_or_default()
            };
            data["access_token"] = json!(token);
            let _ = tx.send(Ok(Event::default().event("done").data(data.to_string()))).await;
        }

        // 结果缓存：文件哈希命中 → 直接 done
        let hash = match TranslationCache::file_sha256(&path) {
            Ok(h) => h,
            Err(e) => {
                send_error(&tx, "IO", e.to_string()).await;
                drop(stage_tx);
                let _ = forwarder.await;
                return;
            }
        };
        if let Some(doc) = cache.get_mineru_parse(&hash, geminus_core::cache::IR_SCHEMA_VERSION) {
            let _ = stage_tx
                .send(json!({ "stage": "cached", "message": "MinerU 解析结果缓存命中" }))
                .await;
            let pages = state2.chargeable_pages(&doc, Some(&path));
            if let Some(msg) = charge_anon(&state2, ip2.as_deref(), &hash, pages).err() {
                send_error(&tx, "ANON_QUOTA", msg).await;
            } else {
                register_doc(&state2, &doc, &path, &hash, user2.as_deref(), &display_name2);
                let token = issue_doc_access(&state2, &doc.id, ip2.as_deref(), user2.is_some());
                send_done(&tx, &doc, anon_shell, &token).await;
            }
            drop(stage_tx);
            let _ = forwarder.await;
            return;
        }

        // v2.2 T1：IR 缓存版本失效（适配器升级）但 raw 已持久化 → 离线重适配（零 API）
        if let Some(raw_root) = cache.mineru_raw_root(&hash) {
            let root = state2.mineru_cache_root.join(&raw_root);
            match mineru::reparse_from_dir(&root, &file_stem(&path)) {
                Ok(doc) => {
                    let _ = stage_tx
                        .send(json!({ "stage": "replayed",
                            "message": "解析器已升级，从本地原始结果离线重放（零云端调用）" }))
                        .await;
                    let _ = cache.put_mineru_parse(&hash, &doc, Some(&raw_root),
                        geminus_core::cache::IR_SCHEMA_VERSION);
                    let pages = state2.chargeable_pages(&doc, Some(&path));
                    if let Some(msg) = charge_anon(&state2, ip2.as_deref(), &hash, pages).err() {
                        send_error(&tx, "ANON_QUOTA", msg).await;
                    } else {
                        register_doc(&state2, &doc, &path, &hash, user2.as_deref(), &display_name2);
                        let token = issue_doc_access(&state2, &doc.id, ip2.as_deref(), user2.is_some());
                        send_done(&tx, &doc, anon_shell, &token).await;
                    }
                    drop(stage_tx);
                    let _ = forwarder.await;
                    return;
                }
                Err(e) => {
                    eprintln!("[mineru] 离线重放失败（走在线解析）: {e}");
                }
            }
        }

        // 端到端 MinerU 解析（async 网络任务，直接在 tokio 上跑）
        let mut work = std::env::temp_dir();
        work.push(format!("geminus_mineru_{}", uuid::Uuid::new_v4().simple()));
        let stage_tx2 = stage_tx.clone();
        let result = mineru::parse_pdf_mineru(&path, &client, &MineruConfig::default(),
            &SplitConfig::default(), &work, move |stage| {
                let payload = serde_json::to_value(stage).unwrap_or_default();
                // 回调在 async 上下文里同步执行——用 try_send 避免 backpressure 死锁
                let _ = stage_tx2.try_send(payload);
            })
        .await;

        let parsed = match result {
            Ok(doc) => Some(doc),
            Err(e) => {
                eprintln!("[mineru] 失败: {e}");
                if allow_fallback {
                    let _ = stage_tx
                        .send(json!({
                            "stage": "fallback",
                            "message": format!("MinerU 解析失败（{e}），已回退内置解析"),
                        }))
                        .await;
                    None
                } else {
                    let code = match &e {
                        mineru::MineruError::AuthFailed => "MINERU_AUTH",
                        mineru::MineruError::QuotaExceeded => "MINERU_QUOTA",
                        mineru::MineruError::FileTooLarge => "MINERU_FILE_TOO_LARGE",
                        _ => "MINERU_ERROR",
                    };
                    send_error(&tx, code, e.to_string()).await;
                    drop(stage_tx);
                    let _ = forwarder.await;
                    return;
                }
            }
        };

        // 成功或回退：builtin 兜底（回退场景）+ 注册 + done
        let doc = parsed.or_else(|| match parse_any(&path) {
            ParseOutcome::Ok(doc, _) => Some(doc),
            _ => None,
        });
        match doc {
            Some(doc) => {
                if let Ok(h) = TranslationCache::file_sha256(&path) {
                    // v2.2 T1：在线解析成功 → raw 持久化（chunk 目录 + raw_meta）+ 版本化 IR 缓存
                    let raw_root = persist_mineru_raw(&state2, &work, &h);
                    let _ = cache.put_mineru_parse(&h, &doc, raw_root.as_deref(),
                        geminus_core::cache::IR_SCHEMA_VERSION);
                }
                let pages = state2.chargeable_pages(&doc, Some(&path));
                if let Some(msg) = charge_anon(&state2, ip2.as_deref(), &hash, pages).err() {
                    send_error(&tx, "ANON_QUOTA", msg).await;
                } else {
                    register_doc(&state2, &doc, &path, &hash, user2.as_deref(), &display_name2);
                    let token = issue_doc_access(&state2, &doc.id, ip2.as_deref(), user2.is_some());
                    send_done(&tx, &doc, anon_shell, &token).await;
                }
            }
            None => {
                send_error(&tx, "PARSE_FAILED", "解析失败（回退亦失败）".to_string()).await;
            }
        }
        drop(stage_tx);
        let _ = forwarder.await;
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

/// 注册文档 + 登记原始文件路径与内容哈希；登录用户写历史（v2.4 T2）
fn register_doc(
    state: &AppState,
    doc: &Document,
    path: &std::path::Path,
    hash: &str,
    owner: Option<&str>,
    file_name: &str,
) {
    state.docs.lock().unwrap().insert(doc.id.clone(), doc.clone());
    state
        .original_pdfs
        .lock()
        .unwrap()
        .insert(doc.id.clone(), path.to_path_buf());
    if !hash.is_empty() {
        state.file_hashes.lock().unwrap().insert(doc.id.clone(), hash.to_string());
    }
    if let Some(user) = owner {
        let file_type = detect_file_type(path).map(|t| t.label().to_string()).unwrap_or_default();
        let pages = state.chargeable_pages(doc, Some(path));
        state.history.insert(
            user,
            &history::HistoryEntry {
                doc_id: doc.id.clone(),
                file_name: file_name.to_string(),
                file_type,
                title: doc.title.clone(),
                file_path: path.display().to_string(),
                pages,
                uploaded_at: 0, // SQL 端 unixepoch() 填
                status: "parsed".to_string(),
            },
        );
    }
}

/// 文件名主干（离线重放的文档标题）
fn file_stem(path: &std::path::Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("doc")
        .to_string()
}

/// 新解析成功后登记访问凭证（v2.4 T1）：
/// - 公网匿名上传 → token + uploader_ip + 匿名标记（24h TTL 清理范围）；
/// - 登录上传 → 仅 token（Session 即访问权）；
/// - 本地开发（无代理头）→ 仅 token（直连放行，不参与 TTL）。
fn issue_doc_access(state: &AppState, doc_id: &str, ip: Option<&str>, logged_in: bool) -> String {
    let is_anon = !logged_in && ip.is_some();
    state.doc_access.issue(doc_id, ip, is_anon)
}

/// v2.2 T1：把 MinerU 在线解析的 work_dir（chunk_NN 解压目录）持久化到
/// `mineru_cache_root/<hash>/`，写 raw_meta.json（含各片首页偏移，取自拆分结果）。
/// 返回相对根目录的路径（写入 mineru_parses.raw_root）。失败仅日志降级（重放不可用时走在线）。
fn persist_mineru_raw(state: &AppState, work_dir: &std::path::Path, file_hash: &str) -> Option<String> {
    let dest = state.mineru_cache_root.join(file_hash);
    let mut entries: Vec<serde_json::Value> = Vec::new();
    let mut chunk_dirs: Vec<std::path::PathBuf> = std::fs::read_dir(work_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("chunk_") && p.is_dir())
        })
        .collect();
    chunk_dirs.sort();
    if chunk_dirs.is_empty() {
        return None;
    }
    // 首页偏移：各片 layout 页数累计（与 reparse_from_dir 的回退推导一致）
    let mut offset = 0usize;
    for c in &chunk_dirs {
        entries.push(serde_json::json!({
            "dir": c.file_name().unwrap().to_string_lossy(),
            "start_page": offset,
        }));
        let sizes = geminus_core::mineru::adapter::load_page_sizes(c);
        offset += sizes.keys().copied().max().map(|m| m + 1).unwrap_or(0);
    }
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(&dest)?;
        std::fs::write(
            dest.join("raw_meta.json"),
            serde_json::to_string(&serde_json::json!({ "chunks": entries }))?,
        )?;
        for c in &chunk_dirs {
            let target = dest.join(c.file_name().unwrap());
            if target.exists() {
                continue; // 同哈希重解析：复用既有 raw（内容确定相同）
            }
            copy_dir_recursive(c, &target)?;
        }
        Ok(())
    };
    match write() {
        Ok(()) => Some(file_hash.to_string()),
        Err(e) => {
            eprintln!("[mineru] raw 持久化失败（离线重放不可用）: {e}");
            None
        }
    }
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// 解析覆盖度审计（v2.2 T1）：IR vs MinerU 原始 JSON 的三层度量
/// （类型/字符/面积 + violations）。本地单用户只读端点，与 /api/debug/audit 同策略。
async fn debug_coverage_handler(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    let Some(doc_id) = params.get("doc_id") else {
        return err_response(StatusCode::BAD_REQUEST, "缺少 doc_id 参数".to_string());
    };
    let Some(hash) = state.file_hashes.lock().unwrap().get(doc_id).cloned() else {
        return err_response(
            StatusCode::NOT_FOUND,
            format!("文档无 MinerU 解析记录: {doc_id}"),
        );
    };
    let raw = geminus_core::mineru::raw::load_raw_items_from_root(&state.mineru_cache_root.join(&hash));
    let raw = match raw {
        Ok(r) => r,
        Err(e) => {
            return err_response(
                StatusCode::NOT_FOUND,
                format!("raw 缓存缺失（{e}）；重新解析一次以建立缓存", ),
            );
        }
    };
    // 用当前适配器从 raw 重放 IR（审计反映适配器现状而非历史缓存）
    let ir = match state.get_doc(doc_id) {
        Some(d) => d,
        None => return err_response(StatusCode::NOT_FOUND, format!("文档不存在: {doc_id}")),
    };
    Json(geminus_core::audit::coverage_audit(&ir, &raw)).into_response()
}

async fn get_document_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
    Path(doc_id): Path<String>,
) -> Response {
    // v2.4 T1：三选一防盗链（?token= 或 Session 或 uploader_ip）
    if let Some(resp) = doc_access_guard(&state, &headers, &doc_id, params.get("token").map(String::as_str)) {
        return resp;
    }
    if let Some(doc) = state.get_doc(&doc_id) {
        return Json(doc).into_response();
    }
    // v2.4 T2：历史回源——重启后内存丢失，按落盘文件重解析（MinerU 缓存优先）
    if let Some(entry) = state.history.find_doc(&doc_id) {
        let path = std::path::PathBuf::from(&entry.file_path);
        if path.exists() {
            let hash = TranslationCache::file_sha256(&path).unwrap_or_default();
            if let Some(doc) = state
                .cache
                .get_mineru_parse(&hash, geminus_core::cache::IR_SCHEMA_VERSION)
            {
                register_doc(&state, &doc, &path, &hash, None, &entry.file_name);
                return Json(doc).into_response();
            }
            if let ParseOutcome::Ok(doc, _) = parse_any(&path) {
                register_doc(&state, &doc, &path, &hash, None, &entry.file_name);
                return Json(doc).into_response();
            }
        }
    }
    err_response(StatusCode::NOT_FOUND, format!("文档不存在: {doc_id}"))
}

/// v2.4 T2：当前用户文档历史（时间倒序；匿名 401）
async fn history_handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(user) = current_user(&state, &headers) else {
        return err_response(StatusCode::UNAUTHORIZED, "请登录后查看历史记录");
    };
    Json(json!({ "items": state.history.list(&user, 100, 0) })).into_response()
}

async fn translate_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<TranslateRequest>,
) -> Response {
    // v2.4 T1：三选一防盗链（body access_token / Session / uploader_ip）
    if let Some(resp) =
        doc_access_guard(&state, &headers, &req.doc_id, req.access_token.as_deref())
    {
        return resp;
    }
    let user = current_user(&state, &headers);
    let Some(doc) = state.get_doc(&req.doc_id) else {
        return err_response(StatusCode::NOT_FOUND, format!("文档不存在: {}", req.doc_id));
    };

    // 匿名限流：同内容对该 IP 已计费 → 翻译不重复扣页数
    if user.is_none() {
        if let Some(ip) = client_ip(&headers) {
            let hash = state.content_hash_of(&req.doc_id);
            let pages = state.chargeable_pages(&doc, None);
            if let Err(e) = state.quota.charge(&ip, &hash, pages) {
                return err_response(StatusCode::TOO_MANY_REQUESTS, e.message());
            }
        }
    }

    // SSE 流：progress* → complete（或 error）
    let (tx, rx) = mpsc::channel::<Result<Event, std::convert::Infallible>>(64);

    let cfg = ApiConfig::new(state.api_base.clone(), state.api_key.clone(), state.model.clone());
    let client = Arc::new(LlmClient::new(cfg));
    let cache = state.cache.clone();
    let concurrency = state.concurrency;
    let state_t = state.clone();
    let history_user = user.clone();
    let source_lang = req.source_lang;
    let target_lang = req.target_lang;

    tokio::spawn(async move {
        let (progress_tx, mut progress_rx) = mpsc::channel(64);
        let translate = tokio::spawn(async move {
            translate_document(&doc, client, cache, progress_tx, concurrency, &source_lang, &target_lang).await
        });

        while let Some(ev) = progress_rx.recv().await {
            let data = serde_json::to_string(&ev).unwrap_or_default();
            let _ = tx
                .send(Ok(Event::default().event("progress").data(data)))
                .await;
        }

        match translate.await {
            Ok(result) => {
                // 内存留存译文（对齐审计 /api/debug/audit 用）+ 历史状态推进（v2.4 T2）
                state_t
                    .translations
                    .lock()
                    .unwrap()
                    .insert(result.doc_id.clone(), result.clone());
                if let Some(u) = &history_user {
                    state_t.history.update_status(u, &result.doc_id, "translated");
                }
                let data = serde_json::to_string(&result).unwrap_or_default();
                let _ = tx
                    .send(Ok(Event::default().event("complete").data(data)))
                    .await;
            }
            Err(e) => {
                let _ = tx
                    .send(Ok(Event::default()
                        .event("error")
                        .data(json!({ "message": e.to_string() }).to_string())))
                    .await;
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

async fn resource_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
    Path((doc_id, rid)): Path<(String, String)>,
) -> Response {
    // v2.4 T1：资源直链同样三选一防盗链（?token=）
    if let Some(resp) = doc_access_guard(&state, &headers, &doc_id, params.get("token").map(String::as_str)) {
        return resp;
    }
    let Some(doc) = state.get_doc(&doc_id) else {
        return err_response(StatusCode::NOT_FOUND, format!("文档不存在: {doc_id}"));
    };
    let Some(res) = doc.resources.get(&rid) else {
        return err_response(StatusCode::NOT_FOUND, format!("资源不存在: {rid}"));
    };
    use base64::Engine as _;
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&res.data_b64) else {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, "资源解码失败");
    };
    let mime = if res.mime.is_empty() {
        "application/octet-stream".to_string()
    } else {
        res.mime.clone()
    };
    (
        StatusCode::OK,
        [("content-type", mime)],
        bytes,
    )
        .into_response()
}

/// 原始 PDF 文件（前端 pdf.js 渲染用）
async fn original_pdf_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
    Path(doc_id): Path<String>,
) -> Response {
    // v2.4 T1：原始文件直链同样三选一防盗链（?token=）
    if let Some(resp) = doc_access_guard(&state, &headers, &doc_id, params.get("token").map(String::as_str)) {
        return resp;
    }
    let Some(path) = state.original_pdfs.lock().unwrap().get(&doc_id).cloned() else {
        return err_response(StatusCode::NOT_FOUND, format!("文档无原始文件: {doc_id}"));
    };
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [("content-type", "application/pdf")],
            bytes,
        )
            .into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, format!("读取失败: {e}")),
    }
}

/// 对齐审计（v2.1 T1.3）：比对原文 IR 与最近一次译文，报告 missing/extra/空译/fallback。
/// 本地单用户只读端点，不做 debug 门控（release E2E 审计脚本需要）。
async fn debug_audit_handler(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    let Some(doc_id) = params.get("doc_id") else {
        return err_response(StatusCode::BAD_REQUEST, "缺少 doc_id 参数".to_string());
    };
    let (Some(orig), Some(trans)) = (state.get_doc(doc_id), state.get_translation(doc_id)) else {
        return err_response(
            StatusCode::NOT_FOUND,
            format!("文档或译文不存在: {doc_id}（需先完成一次翻译）"),
        );
    };
    Json(geminus_core::audit::alignment_audit(&orig, &trans)).into_response()
}

async fn mineru_config_get_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({ "configured": state.mineru_client().is_some(), "model_version": "vlm" }))
}

#[derive(serde::Deserialize)]
struct MineruConfigRequest {
    token: String,
}

async fn mineru_config_post_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<MineruConfigRequest>,
) -> Response {
    // 运行时改配置属管理操作：仅登录用户可用（防匿名篡改上游）
    if current_user(&state, &headers).is_none() {
        return err_response(StatusCode::UNAUTHORIZED, "请登录后再配置 MinerU Token");
    }
    let token = req.token.trim().trim_matches('"').to_string();
    let base = std::env::var("MINERU_API_BASE").unwrap_or_else(|_| "https://mineru.net/api/v4".into());
    let client = MineruClient::new(base, token);
    *state.mineru.lock().unwrap() = Some(client);
    Json(json!({ "configured": true })).into_response()
}

// ────────────────────────── v2.1：单段重译（T7.2） ──────────────────────────

#[derive(Deserialize)]
struct TranslateParagraphRequest {
    doc_id: String,
    block_id: String,
    #[serde(default = "default_source_lang")]
    source_lang: String,
    #[serde(default = "default_target_lang")]
    target_lang: String,
    /// v2.4 T1：匿名文档访问凭证
    #[serde(default)]
    access_token: Option<String>,
}

/// 单段重译（v2.1 T7.2）：绕过读缓存强制重译 → 覆盖写缓存 → 返回重建块（JSON）。
/// v2.2 T4：算法块单块重译（行级 [L n]，失败返回 fallback 对）
async fn retranslate_algorithm(
    state: &AppState,
    req: &TranslateParagraphRequest,
    block: &geminus_core::model::Block,
) -> Response {
    use geminus_core::translate::algorithm as algo;

    let lines = algo::extract_translatable_lines(block);
    if lines.is_empty() {
        return err_response(StatusCode::BAD_REQUEST, "算法块无可翻译行".to_string());
    }
    let source_text = lines.iter().map(|l| l.source.clone()).collect::<Vec<_>>().join("\n");
    let srcs: Vec<&str> = lines.iter().map(|l| l.source.as_str()).collect();
    let prompt = algo::build_algorithm_prompt(&srcs, &req.source_lang, &req.target_lang);
    let cfg = ApiConfig::new(state.api_base.clone(), state.api_key.clone(), state.model.clone());
    let client = Arc::new(LlmClient::new(cfg));
    let resp = match client.translate_raw(&prompt).await {
        Ok(t) => t,
        Err(e) => return err_response(StatusCode::BAD_GATEWAY, format!("重译失败: {e}")),
    };
    let parts = algo::split_l_numbered(&resp, lines.len());
    let (rebuilt, fallback) = match parts {
        Some(parts) => {
            let mut b = block.clone();
            let ok = algo::rebuild_algorithm_lines(&mut b, &lines, &parts);
            if ok {
                let _ = state.cache.put(
                    &source_text,
                    &req.source_lang,
                    &req.target_lang,
                    &parts.join("\n"),
                    &state.model,
                );
            }
            (b, !ok)
        }
        None => (block.clone(), true),
    };
    let pair = geminus_core::model::TranslatedBlock {
        block_id: req.block_id.clone(),
        original: block.clone(),
        translated: Some(if fallback { block.clone() } else { rebuilt }),
        fallback,
    };
    state.update_translation_block(&req.doc_id, pair.clone());
    Json(pair).into_response()
}

async fn translate_paragraph_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<TranslateParagraphRequest>,
) -> Response {
    // v2.4 T1：三选一防盗链（body access_token / Session / uploader_ip）
    if let Some(resp) =
        doc_access_guard(&state, &headers, &req.doc_id, req.access_token.as_deref())
    {
        return resp;
    }
    let Some(doc) = state.get_doc(&req.doc_id) else {
        return err_response(StatusCode::NOT_FOUND, format!("文档不存在: {}", req.doc_id));
    };
    // 匿名门控：文档必须在该 IP 的免费额度内（防脚本化刷 LLM 调用），且每小时限 30 次
    if current_user(&state, &headers).is_none() {
        if let Some(ip) = client_ip(&headers) {
            let hash = state.content_hash_of(&req.doc_id);
            if !state.quota.already_charged(&ip, &hash) {
                return err_response(
                    StatusCode::TOO_MANY_REQUESTS,
                    "该文档不在当前未登录额度内，请登录后使用单段重译",
                );
            }
            if !state.paragraph_rate.allow(&ip) {
                return err_response(StatusCode::TOO_MANY_REQUESTS, "操作过于频繁，请稍后再试或登录后使用");
            }
        }
    }
    let Some(block) = doc.blocks.iter().find(|b| b.id() == req.block_id) else {
        return err_response(StatusCode::NOT_FOUND, format!("块不存在: {}", req.block_id));
    };
    // v2.2 T4：算法块走行级重译路径
    if let geminus_core::model::Block::Algorithm { .. } = block {
        return retranslate_algorithm(&state, &req, block).await;
    }
    let geminus_core::model::Block::Paragraph { content, .. } = block else {
        return err_response(StatusCode::BAD_REQUEST, format!("块 {} 不是可翻译段落", req.block_id));
    };
    let (source_text, refs) = geminus_core::translate::extract_source_text(content);
    if source_text.trim().is_empty() {
        return err_response(StatusCode::BAD_REQUEST, "段落无可见文本".to_string());
    }
    // v2.3 B4b：与整篇管线同款 math_slot 防线（公式不经过模型）+ 版本化缓存键，
    // 消除与 translate_document 路径的不对称（此前原始 $...$ 直发模型被改写）
    let (slotted, math_slots) = geminus_core::translate::math_slot::extract(&source_text);
    let cfg = ApiConfig::new(state.api_base.clone(), state.api_key.clone(), state.model.clone());
    let client = Arc::new(LlmClient::new(cfg));
    let translated = match client
        .translate_text(&slotted, &req.source_lang, &req.target_lang)
        .await
    {
        Ok(t) if math_slots.is_empty() || geminus_core::translate::math_slot::verify(&slotted, &t) => t,
        Ok(_) => {
            eprintln!("[math_slot] 单段重译令牌校验失败，重试: {}", req.block_id);
            match client
                .translate_text(&slotted, &req.source_lang, &req.target_lang)
                .await
            {
                Ok(t2) if geminus_core::translate::math_slot::verify(&slotted, &t2) => t2,
                Ok(t2) => {
                    // v2.3 B4：择优强制回填（缺失公式按序追加），不再让重译失败
                    let (restored, missing) =
                        geminus_core::translate::math_slot::force_restore(&t2, &math_slots);
                    eprintln!(
                        "[math_slot][warn] 单段重译强制回填 {}: missing={:?}",
                        req.block_id, missing
                    );
                    restored
                }
                Err(e) => return err_response(StatusCode::BAD_GATEWAY, format!("重译失败: {e}")),
            }
        }
        Err(e) => return err_response(StatusCode::BAD_GATEWAY, format!("重译失败: {e}")),
    };
    let restored = if math_slots.is_empty() {
        translated
    } else {
        geminus_core::translate::math_slot::restore(&translated, &math_slots)
    };
    // 覆盖写缓存（INSERT OR REPLACE），下次整篇翻译即用新译文；
    // 含公式段落用版本化键（与整篇管线口径一致，管线升级自动失效）
    let _ = if math_slots.is_empty() {
        state.cache.put(&slotted, &req.source_lang, &req.target_lang, &restored, &state.model)
    } else {
        state.cache.put_versioned(
            &slotted,
            &req.source_lang,
            &req.target_lang,
            &restored,
            &state.model,
            geminus_core::cache::MATH_PIPELINE_VERSION,
        )
    };
    let (rebuilt, _) = geminus_core::translate::rebuild::rebuild_paragraph(block, &restored, &refs);
    let pair = geminus_core::model::TranslatedBlock {
        block_id: req.block_id.clone(),
        original: block.clone(),
        translated: Some(rebuilt),
        fallback: false,
    };
    // 同步内存译文（保持 /api/debug/audit 一致性）
    state.update_translation_block(&req.doc_id, pair.clone());
    Json(pair).into_response()
}

// ────────────────────────── 路由 ──────────────────────────

pub fn build_app(state: AppState) -> Router {
    let static_dir =
        std::env::var("GEMINUS_STATIC_DIR").unwrap_or_else(|_| "app/dist".to_string());
    // 请求体上限 = 上传上限 + 16MB 余量（multipart 边界开销），超限由 handler 给出 JSON 413
    let body_limit = state.upload_limit + 16 * 1024 * 1024;
    Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/login", post(login_handler))
        .route("/api/auth/logout", post(logout_handler))
        .route("/api/auth/me", get(me_handler))
        .route("/api/parse", post(parse_docx_handler))
        .route("/api/translate", post(translate_handler))
        .route("/api/translate/paragraph", post(translate_paragraph_handler))
        .route("/api/documents/:doc_id", get(get_document_handler))
        .route("/api/history", get(history_handler))
        .route("/api/resource/:doc_id/:rid", get(resource_handler))
        .route("/api/original/:doc_id", get(original_pdf_handler))
        .route("/api/config/mineru", get(mineru_config_get_handler).post(mineru_config_post_handler))
        .route("/api/debug/audit", get(debug_audit_handler))
        .route("/api/debug/coverage", get(debug_coverage_handler))
        .fallback_service(tower_http::services::ServeDir::new(static_dir))
        .layer(axum::extract::DefaultBodyLimit::max(body_limit))
        .with_state(state)
}

/// 启动 HTTP 服务器（main 入口复用）
pub async fn run() {
    let port = std::env::var("GEMINUS_PORT").unwrap_or_else(|_| "39321".to_string());
    // GEMINUS_BIND 接受 host:port 或纯 host（此时与 GEMINUS_PORT 组合）
    let addr = match std::env::var("GEMINUS_BIND") {
        Ok(bind) if bind.contains(':') => bind,
        Ok(host) => format!("{host}:{port}"),
        Err(_) => format!("127.0.0.1:{port}"),
    };
    let state = AppState::from_env();
    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("监听 {addr} 失败: {e}"));
    eprintln!("geminus-server listening on http://{addr}");
    axum::serve(listener, app).await.expect("服务器错误");
}
