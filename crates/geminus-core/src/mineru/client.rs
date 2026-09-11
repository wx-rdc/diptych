//! MinerU 精准解析 API 客户端（docs/mineru-output-spec.md 实测协议）。
//!
//! 流程：批量签名上传（file-urls/batch → OSS PUT）→ 轮询 extract-results/batch → 下载 ZIP。
//! 实测要点：file_urls 为 URL 字符串数组；OSS PUT 不得携带 Content-Type（签名 403）；
//! 错误响应用 `msgCode`（如 A0202），与成功响应的 `code` 字段不同名。

use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MineruError {
    #[error("MinerU 认证失败（Token 无效或过期）")]
    AuthFailed,
    #[error("MinerU 每日额度已用尽")]
    QuotaExceeded,
    #[error("文件超限（>200MB 或 >200 页），需进一步拆分")]
    FileTooLarge,
    #[error("解析任务失败: {0}")]
    TaskFailed(String),
    #[error("网络错误: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("ZIP 解压错误: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("轮询超时（{0:?}）")]
    Timeout(Duration),
    #[error("PDF 拆分失败: {0}")]
    Split(#[from] crate::mineru::splitter::SplitError),
    #[error("响应格式异常: {0}")]
    BadResponse(String),
}

/// MinerU 解析配置
#[derive(Debug, Clone)]
pub struct MineruConfig {
    pub model_version: String, // "vlm"
    pub language: String,      // "ch"
}

impl Default for MineruConfig {
    fn default() -> Self {
        Self { model_version: "vlm".into(), language: "ch".into() }
    }
}

/// 单文件页级进度
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FileProgress {
    pub extracted_pages: u32,
    pub total_pages: u32,
}

/// 批量任务中单文件状态
#[derive(Debug, Clone, PartialEq)]
pub enum BatchState {
    WaitingFile,
    Pending,
    Running(FileProgress),
    Done,
    Failed(String),
}

/// 轮询到的单文件结果
#[derive(Debug, Clone)]
pub struct BatchFileResult {
    pub file_name: String,
    pub state: BatchState,
    pub full_zip_url: Option<String>,
}

/// MinerU API 客户端
#[derive(Clone)]
pub struct MineruClient {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

impl MineruClient {
    pub fn new(base_url: String, token: String) -> Self {
        Self {
            base_url,
            token,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(180))
                .build()
                .expect("reqwest client 构建失败"),
        }
    }

    /// 从环境构造（.env.local → env）；Token 缺失返回 None（MinerU 不可用）
    pub fn from_env() -> Option<Self> {
        let _ = dotenvy::from_filename(".env.local");
        let token = std::env::var("MINERU_API_KEY")
            .ok()
            .map(|t| t.trim().trim_matches('"').trim_matches('\'').to_string())
            .filter(|t| !t.is_empty())?;
        let base = std::env::var("MINERU_API_BASE")
            .unwrap_or_else(|_| "https://mineru.net/api/v4".to_string());
        Some(Self::new(base, token))
    }

    /// 1) 批量获取签名上传 URL。实测 `file_urls` 是与请求 files 顺序对应的 URL 字符串数组。
    pub async fn request_upload_urls(
        &self,
        names: &[&str],
        cfg: &MineruConfig,
    ) -> Result<(String, Vec<String>), MineruError> {
        let files: Vec<serde_json::Value> = names
            .iter()
            .map(|n| serde_json::json!({ "name": n, "is_ocr": false, "data_id": n }))
            .collect();
        let body = serde_json::json!({
            "enable_formula": true,
            "enable_table": true,
            "model_version": cfg.model_version,
            "language": cfg.language,
            "files": files,
        });
        let resp = self
            .http
            .post(format!("{}/file-urls/batch", self.base_url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let json: serde_json::Value = resp.json().await?;
        if let Some(e) = classify_error(status, &json) {
            return Err(e);
        }
        let data = json
            .get("data")
            .ok_or_else(|| MineruError::BadResponse("缺少 data 字段".into()))?;
        let batch_id = data
            .get("batch_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MineruError::BadResponse("缺少 batch_id".into()))?
            .to_string();
        let urls: Vec<String> = data
            .get("file_urls")
            .and_then(|v| v.as_array())
            .ok_or_else(|| MineruError::BadResponse("file_urls 缺失或非数组".into()))?
            .iter()
            .map(|v| v.as_str().unwrap_or_default().to_string())
            .collect();
        if urls.len() != names.len() {
            return Err(MineruError::BadResponse(format!(
                "file_urls 数量 {} 与请求 {} 不符",
                urls.len(),
                names.len()
            )));
        }
        Ok((batch_id, urls))
    }

    /// 2) 上传文件到签名 URL。
    ///
    /// OSS 签名校验要求：不携带 Content-Type 头；网络错误/5xx 指数退避重试 ≤3 次。
    pub async fn upload_file(&self, url: &str, path: &Path) -> Result<(), MineruError> {
        let bytes = std::fs::read(path)?;
        let mut attempt = 0u32;
        loop {
            let result = self.http.put(url).body(bytes.clone()).send().await;
            match result {
                Ok(resp) if resp.status().is_success() => return Ok(()),
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.json::<serde_json::Value>().await.ok();
                    if let Some(v) = &body {
                        if let Some(e) = classify_error(status, v) {
                            return Err(e);
                        }
                    }
                    if status.is_server_error() && attempt < 2 {
                        attempt += 1;
                        tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
                        continue;
                    }
                    return Err(MineruError::BadResponse(format!("上传失败: HTTP {status}")));
                }
                Err(e) => {
                    if attempt < 2 && e.is_timeout() || e.is_connect() {
                        attempt += 1;
                        tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }
    }

    /// 3) 轮询批量结果直至全部 done/failed 或超时。每轮经 progress_cb 回调当前快照。
    pub async fn poll_batch<F>(
        &self,
        batch_id: &str,
        timeout: Duration,
        interval: Duration,
        mut progress_cb: F,
    ) -> Result<Vec<BatchFileResult>, MineruError>
    where
        F: FnMut(&[BatchFileResult]),
    {
        let url = format!("{}/extract-results/batch/{batch_id}", self.base_url);
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let resp = self.http.get(&url).bearer_auth(&self.token).send().await?;
            let status = resp.status();
            let json: serde_json::Value = resp.json().await?;
            if let Some(e) = classify_error(status, &json) {
                return Err(e);
            }
            let results = parse_batch_results(
                json.get("data")
                    .and_then(|d| d.get("extract_result"))
                    .and_then(|r| r.as_array())
                    .ok_or_else(|| MineruError::BadResponse("extract_result 缺失".into()))?,
            )?;
            progress_cb(&results);
            if results
                .iter()
                .all(|r| matches!(r.state, BatchState::Done | BatchState::Failed(_)))
            {
                return Ok(results);
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(MineruError::Timeout(timeout));
            }
            tokio::time::sleep(interval).await;
        }
    }

    /// 4) 下载结果 ZIP 并解压到 dest 目录，返回 dest。
    pub async fn download_zip(&self, zip_url: &str, dest: &Path) -> Result<PathBuf, MineruError> {
        std::fs::create_dir_all(dest)?;
        let bytes = self.http.get(zip_url).send().await?.error_for_status()?.bytes().await?;
        let reader = std::io::Cursor::new(&bytes[..]);
        let mut archive = zip::ZipArchive::new(reader)?;
        archive.extract(dest)?;
        Ok(dest.to_path_buf())
    }
}

/// 解析 extract_result 数组
fn parse_batch_results(arr: &[serde_json::Value]) -> Result<Vec<BatchFileResult>, MineruError> {
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        let file_name = v
            .get("file_name")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        let state = match v.get("state").and_then(|s| s.as_str()) {
            Some("waiting-file") => BatchState::WaitingFile,
            Some("pending") => BatchState::Pending,
            Some("running") => {
                let p = v.get("extract_progress").cloned().unwrap_or(serde_json::Value::Null);
                BatchState::Running(FileProgress {
                    extracted_pages: p.get("extracted_pages").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                    total_pages: p.get("total_pages").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                })
            }
            Some("done") => BatchState::Done,
            Some("failed") => BatchState::Failed(
                v.get("err_msg").and_then(|x| x.as_str()).unwrap_or("未知错误").to_string(),
            ),
            other => {
                return Err(MineruError::BadResponse(format!("未知 state: {other:?}")));
            }
        };
        let full_zip_url = v
            .get("full_zip_url")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        out.push(BatchFileResult { file_name, state, full_zip_url });
    }
    Ok(out)
}

/// 错误响应分类（msgCode 字段；HTTP 401 等）
fn classify_error(status: reqwest::StatusCode, body: &serde_json::Value) -> Option<MineruError> {
    let code = body.get("code").and_then(|c| c.as_i64());
    let msg_code = body.get("msgCode").and_then(|c| c.as_str());
    if code == Some(0) {
        return None;
    }
    // 数字类错误码（-60005 等）可能在 code 或 msgCode
    let numeric = code.or_else(|| msg_code.and_then(|m| m.parse::<i64>().ok()));
    let textual = msg_code.unwrap_or_default();
    if status == reqwest::StatusCode::UNAUTHORIZED || textual.starts_with("A02") {
        return Some(MineruError::AuthFailed);
    }
    match numeric {
        Some(-60005) | Some(-60006) => Some(MineruError::FileTooLarge),
        Some(-60018) => Some(MineruError::QuotaExceeded),
        _ => {
            if code.is_some() || msg_code.is_some() {
                let msg = body.get("msg").and_then(|m| m.as_str()).unwrap_or("未知错误");
                Some(MineruError::BadResponse(format!("HTTP {status}: {msg}")))
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::{get, post, put};
    use axum::Router;
    use std::io::Write;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    async fn spawn_mock(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap(); });
        format!("http://{addr}")
    }

    /// 构造最小结果 ZIP：content_list.json + layout.json + images/x.jpg
    fn sample_zip() -> Vec<u8> {
        let buf = std::io::Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(buf);
        let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        w.start_file("content_list.json", opts).unwrap();
        w.write_all(br#"[{"type":"text","text":"hello","bbox":[1,2,3,4],"page_idx":0}]"#).unwrap();
        w.start_file("layout.json", opts).unwrap();
        w.write_all(br#"{"pdf_info":[{"page_size":[612,792],"page_idx":0}]}"#).unwrap();
        w.start_file("images/x.jpg", opts).unwrap();
        w.write_all(b"\xff\xd8fakejpg").unwrap();
        w.finish().unwrap().into_inner()
    }

    #[tokio::test]
    async fn full_flow_upload_poll_download() {
        let zip_bytes = Arc::new(sample_zip());
        let polls = Arc::new(AtomicU32::new(0));
        let upload_header_ok = Arc::new(std::sync::Mutex::new(true));

        let p_router = polls.clone();
        let zh = upload_header_ok.clone();
        let z1 = zip_bytes.clone();
        let router = Router::new()
            .route(
                "/api/v4/file-urls/batch",
                post(|body: axum::Json<serde_json::Value>| async move {
                    let n = body["files"].as_array().map(|a| a.len()).unwrap_or(0);
                    assert_eq!(n, 1);
                    axum::Json(serde_json::json!({"code": 0, "data": {"batch_id": "b1",
                        "file_urls": ["http://127.0.0.1:1/oss.pdf"]}}))
                }),
            )
            .route(
                "/oss.pdf",
                put(|headers: axum::http::HeaderMap, body: axum::body::Bytes| async move {
                    assert!(!body.is_empty());
                    // OSS 签名要求：不得带 Content-Type
                    if headers.get("content-type").is_some() {
                        *zh.lock().unwrap() = false;
                    }
                    "ok"
                }),
            )
            .route(
                "/api/v4/extract-results/batch/:batch_id",
                get(move || {
                    let p = p_router.clone();
                    async move {
                        let n = p.fetch_add(1, Ordering::SeqCst);
                        if n == 0 {
                            axum::Json(serde_json::json!({"code": 0, "data": {"extract_result": [
                                {"file_name": "a.pdf", "state": "running",
                                 "extract_progress": {"extracted_pages": 1, "total_pages": 2}}]}}))
                        } else {
                            axum::Json(serde_json::json!({"code": 0, "data": {"extract_result": [
                                {"file_name": "a.pdf", "state": "done", "full_zip_url": "http://127.0.0.1:1/result.zip"}]}}))
                        }
                    }
                }),
            )
            .route(
                "/result.zip",
                get(move || {
                    let z = z1.clone();
                    async move {
                        (
                            [(axum::http::header::CONTENT_TYPE, "application/zip")],
                            z.as_ref().clone(),
                        )
                    }
                }),
            );
        let base = spawn_mock(router).await;
        // 把 mock 的 oss/result 地址替换为可达地址：直接以本 mock base 重建 URL
        let client = MineruClient::new(format!("{base}/api/v4"), "tok".into());

        let (batch_id, urls) = client
            .request_upload_urls(&["a.pdf"], &MineruConfig::default())
            .await
            .unwrap();
        assert_eq!(batch_id, "b1");
        assert_eq!(urls.len(), 1);
        // 上传到 mock 换成可达 URL
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"%PDF-1.4 fake").unwrap();
        let upload_url = urls[0].replace("http://127.0.0.1:1/oss.pdf", &format!("{base}/oss.pdf"));
        client.upload_file(&upload_url, tmp.path()).await.unwrap();
        assert!(*upload_header_ok.lock().unwrap(), "PUT 不应携带 Content-Type");

        let progress_seen = Arc::new(std::sync::Mutex::new(Vec::<usize>::new()));
        let zip_url_holder = Arc::new(std::sync::Mutex::new(None::<String>));
        let zh2 = zip_url_holder.clone();
        let ps = progress_seen.clone();
        let results = client
            .poll_batch(&batch_id, Duration::from_secs(5), Duration::from_millis(1), move |rs| {
                if let Some(BatchFileResult { full_zip_url: Some(u), .. }) =
                    rs.iter().find(|r| r.file_name == "a.pdf")
                {
                    *zh2.lock().unwrap() = Some(u.clone());
                }
                ps.lock().unwrap().push(rs.len());
            })
            .await
            .unwrap();
        assert!(progress_seen.lock().unwrap().len() >= 2, "至少两轮轮询");
        assert!(matches!(results[0].state, BatchState::Done));

        let zip_url = zip_url_holder.lock().unwrap().clone().unwrap()
            .replace("http://127.0.0.1:1/result.zip", &format!("{base}/result.zip"));
        let dir = tempfile::tempdir().unwrap();
        client.download_zip(&zip_url, dir.path()).await.unwrap();
        assert!(dir.path().join("content_list.json").exists());
        assert!(dir.path().join("images/x.jpg").exists());
    }

    #[tokio::test]
    async fn auth_error_classified() {
        let router = Router::new().route(
            "/api/v4/file-urls/batch",
            post(|| async {
                (
                    axum::http::StatusCode::UNAUTHORIZED,
                    axum::Json(serde_json::json!({"msgCode": "A0202", "msg": "user authenticate failed"})),
                )
            }),
        );
        let base = spawn_mock(router).await;
        let client = MineruClient::new(format!("{base}/api/v4"), "bad".into());
        let err = client.request_upload_urls(&["a.pdf"], &MineruConfig::default()).await.unwrap_err();
        assert!(matches!(err, MineruError::AuthFailed), "{err:?}");
    }

    #[tokio::test]
    async fn error_codes_classified() {
        assert!(matches!(
            classify_error(reqwest::StatusCode::OK, &serde_json::json!({"code": -60018})),
            Some(MineruError::QuotaExceeded)
        ));
        assert!(matches!(
            classify_error(reqwest::StatusCode::OK, &serde_json::json!({"msgCode": "-60005"})),
            Some(MineruError::FileTooLarge)
        ));
        assert!(classify_error(reqwest::StatusCode::OK, &serde_json::json!({"code": 0})).is_none());
    }

    #[tokio::test]
    async fn poll_timeout_returns_error() {
        let router = Router::new().route(
            "/api/v4/extract-results/batch/:batch_id",
            get(|| async {
                axum::Json(serde_json::json!({"code": 0, "data": {"extract_result": [
                    {"file_name": "a.pdf", "state": "running",
                     "extract_progress": {"extracted_pages": 0, "total_pages": 9}}]}}))
            }),
        );
        let base = spawn_mock(router).await;
        let client = MineruClient::new(format!("{base}/api/v4"), "tok".into());
        let err = client
            .poll_batch("b", Duration::from_millis(30), Duration::from_millis(10), |_| {})
            .await
            .unwrap_err();
        assert!(matches!(err, MineruError::Timeout(_)));
    }

    #[tokio::test]
    async fn task_failed_state_reported() {
        let results = parse_batch_results(&[serde_json::json!({
            "file_name": "a.pdf", "state": "failed", "err_msg": "boom"
        })])
        .unwrap();
        assert!(matches!(&results[0].state, BatchState::Failed(m) if m == "boom"));
    }

    #[test]
    fn upload_retries_on_server_error() {
        // 编排：前两次 500，第三次 200（tokio runtime 手动驱动）
        let rt = tokio::runtime::Runtime::new().unwrap();
        let attempts = Arc::new(AtomicU32::new(0));
        let a = attempts.clone();
        let router = Router::new().route(
            "/oss",
            put(move || {
                let a = a.clone();
                async move {
                    if a.fetch_add(1, Ordering::SeqCst) < 2 {
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR
                    } else {
                        axum::http::StatusCode::OK
                    }
                }
            }),
        );
        let listener = rt.block_on(async {
            tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap()
        });
        let addr = listener.local_addr().unwrap();
        rt.spawn(async move { axum::serve(listener, router).await.unwrap(); });
        let client = MineruClient::new("http://unused".into(), "t".into());
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"data").unwrap();
        rt.block_on(client.upload_file(&format!("http://{addr}/oss"), tmp.path())).unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }
}
