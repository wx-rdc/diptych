//! OpenAI 兼容 LLM 客户端（reqwest + rustls）。
//!
//! deepseek-v4-flash 为推理模型：最终译文在 `choices[0].message.content`，
//! `reasoning_content` 为思维链（忽略）；`max_tokens` 需覆盖推理消耗（4096）。

use std::time::Duration;

use serde_json::Value;
use thiserror::Error;

use crate::config::ApiConfig;

use super::prompt::build_messages;

#[derive(Debug, Error)]
pub enum TranslateError {
    #[error("网络错误: {0}")]
    Network(String),
    #[error("HTTP {status}: {body}")]
    Status { status: u16, body: String },
    #[error("响应被截断（finish_reason=length 且 content 为空），可考虑增大 max_tokens")]
    Truncated,
    #[error("响应缺少有效 content")]
    EmptyContent,
    #[error("API 错误响应: {0}")]
    Api(String),
    #[error("响应 JSON 解析失败: {0}")]
    Json(String),
}

pub struct LlmClient {
    http: reqwest::Client,
    cfg: ApiConfig,
    /// 重试退避基数（测试可置零加速）
    retry_delay: Duration,
    max_tokens: u32,
}

impl LlmClient {
    pub fn new(cfg: ApiConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("reqwest Client 构建失败");
        Self {
            http,
            cfg,
            retry_delay: Duration::from_secs(1),
            max_tokens: 4096,
        }
    }

    /// 测试用：无退避重试
    pub fn with_retry_delay(mut self, delay: Duration) -> Self {
        self.retry_delay = delay;
        self
    }

    pub fn config(&self) -> &ApiConfig {
        &self.cfg
    }

    /// 翻译单段文本
    pub async fn translate_text(
        &self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
    ) -> Result<String, TranslateError> {
        let mut last_err: Option<TranslateError> = None;
        for attempt in 0..3 {
            match self.request_once(text, source_lang, target_lang).await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    let retryable = match &e {
                        TranslateError::Network(_) => true,
                        TranslateError::Status { status, .. } => *status == 429 || *status >= 500,
                        _ => false,
                    };
                    last_err = Some(e);
                    if retryable && attempt < 2 {
                        let delay = self.retry_delay * (attempt as u32 + 1);
                        if !delay.is_zero() {
                            tokio::time::sleep(delay).await;
                        }
                        continue;
                    }
                    return Err(last_err.unwrap());
                }
            }
        }
        Err(last_err.expect("至少一次尝试"))
    }

    /// 翻译合并 chunk：user_content 已是完整指令文本（含 [N] 编号段落），
    /// 不再包翻译模板；max_tokens 放大（多段译文远长于单段）。
    pub async fn translate_raw(&self, user_content: &str) -> Result<String, TranslateError> {
        self.request_once_messages(
            vec![serde_json::json!({"role": "user", "content": user_content})],
            (self.max_tokens * 3).max(12288),
        )
        .await
    }

    async fn request_once(
        &self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
    ) -> Result<String, TranslateError> {
        self.request_once_messages(
            build_messages(text, source_lang, target_lang),
            self.max_tokens,
        )
        .await
    }

    async fn request_once_messages(
        &self,
        messages: Vec<Value>,
        max_tokens: u32,
    ) -> Result<String, TranslateError> {
        let body = serde_json::json!({
            "model": self.cfg.model,
            "messages": messages,
            "temperature": 0.3,
            "max_tokens": max_tokens,
        });

        let url = format!("{}/chat/completions", self.cfg.api_base.trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.cfg.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| TranslateError::Network(e.to_string()))?;

        let status = resp.status().as_u16();
        let raw = resp
            .text()
            .await
            .map_err(|e| TranslateError::Network(e.to_string()))?;

        if !(200..300).contains(&status) {
            // 尝试提取 OpenAI 风格错误信息
            if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                if let Some(msg) = v["error"]["message"].as_str() {
                    return Err(TranslateError::Status {
                        status,
                        body: msg.to_string(),
                    });
                }
            }
            return Err(TranslateError::Status {
                status,
                body: raw.chars().take(200).collect(),
            });
        }

        let v: Value = serde_json::from_str(&raw).map_err(|e| TranslateError::Json(e.to_string()))?;

        // 上游业务错误（HTTP 200 但 body 带 error）
        if let Some(msg) = v["error"]["message"].as_str() {
            return Err(TranslateError::Api(msg.to_string()));
        }

        let finish_reason = v["choices"][0]["finish_reason"].as_str().unwrap_or("");
        let content = v["choices"][0]["message"]["content"].as_str().unwrap_or("");

        if content.trim().is_empty() {
            if finish_reason == "length" {
                return Err(TranslateError::Truncated);
            }
            return Err(TranslateError::EmptyContent);
        }

        Ok(clean_output(content))
    }
}

/// 清洗模型输出：剥离包裹性代码围栏
fn clean_output(content: &str) -> String {
    let trimmed = content.trim();
    let mut out = trimmed.to_string();
    if out.starts_with("```") {
        // 去掉首行 ```lang
        if let Some(nl) = out.find('\n') {
            out = out[nl + 1..].to_string();
        }
        // 去掉尾行 ```
        if let Some(pos) = out.rfind("```") {
            out.truncate(pos);
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::post;
    use axum::Router;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    async fn spawn_mock(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn client(base: &str) -> LlmClient {
        LlmClient::new(ApiConfig::new(base, "sk-test", "deepseek-v4-flash"))
            .with_retry_delay(Duration::ZERO)
    }

    #[tokio::test]
    async fn parses_deepseek_reasoning_model_response() {
        let router = Router::new().route(
            "/chat/completions",
            post(|| async {
                axum::Json(serde_json::json!({
                    "choices": [{
                        "message": {
                            "role": "assistant",
                            "content": "敏捷的棕色狐狸跳过了懒狗。",
                            "reasoning_content": "thinking... (must be ignored)"
                        },
                        "finish_reason": "stop"
                    }],
                    "model": "deepseek-v4-flash"
                }))
            }),
        );
        let base = spawn_mock(router).await;
        let out = client(&base).translate_text("The quick brown fox...", "en", "zh").await.unwrap();
        assert_eq!(out, "敏捷的棕色狐狸跳过了懒狗。");
    }

    #[tokio::test]
    async fn retries_on_429_then_succeeds() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls2 = calls.clone();
        let router = Router::new().route(
            "/chat/completions",
            post(move || {
                let calls = calls2.clone();
                async move {
                    if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                        return (
                            axum::http::StatusCode::TOO_MANY_REQUESTS,
                            axum::Json(serde_json::json!({"error": {"message": "rate limited"}})),
                        );
                    }
                    (
                        axum::http::StatusCode::OK,
                        axum::Json(serde_json::json!({
                            "choices": [{"message": {"content": "第二次成功"}, "finish_reason": "stop"}]
                        })),
                    )
                }
            }),
        );
        let base = spawn_mock(router).await;
        let out = client(&base).translate_text("hello", "en", "zh").await.unwrap();
        assert_eq!(out, "第二次成功");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn gives_up_after_three_5xx() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls2 = calls.clone();
        let router = Router::new().route(
            "/chat/completions",
            post(move || {
                let calls = calls2.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "boom")
                }
            }),
        );
        let base = spawn_mock(router).await;
        let err = client(&base).translate_text("hello", "en", "zh").await.unwrap_err();
        assert!(matches!(err, TranslateError::Status { status: 500, .. }), "got {err:?}");
        assert_eq!(calls.load(Ordering::SeqCst), 3); // 1 + 2 重试
    }

    #[tokio::test]
    async fn truncated_response_is_error_without_retry() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls2 = calls.clone();
        let router = Router::new().route(
            "/chat/completions",
            post(move || {
                let calls = calls2.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    axum::Json(serde_json::json!({
                        "choices": [{"message": {"content": ""}, "finish_reason": "length"}]
                    }))
                }
            }),
        );
        let base = spawn_mock(router).await;
        let err = client(&base).translate_text("long text...", "en", "zh").await.unwrap_err();
        assert!(matches!(err, TranslateError::Truncated), "got {err:?}");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn strips_code_fences() {
        let router = Router::new().route(
            "/chat/completions",
            post(|| async {
                axum::Json(serde_json::json!({
                    "choices": [{"message": {"content": "```\n译文在围栏里\n```"}, "finish_reason": "stop"}]
                }))
            }),
        );
        let base = spawn_mock(router).await;
        let out = client(&base).translate_text("hello", "en", "zh").await.unwrap();
        assert_eq!(out, "译文在围栏里");
    }

    #[tokio::test]
    async fn network_error_is_reported() {
        // 连接不存在的端口
        let err = client("http://127.0.0.1:1")
            .translate_text("hello", "en", "zh")
            .await
            .unwrap_err();
        assert!(matches!(err, TranslateError::Network(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn sends_expected_payload_shape() {
        let captured = Arc::new(std::sync::Mutex::new(None::<serde_json::Value>));
        let captured2 = captured.clone();
        let router = Router::new().route(
            "/chat/completions",
            post(move |body: axum::Json<serde_json::Value>| {
                let captured = captured2.clone();
                async move {
                    *captured.lock().unwrap() = Some(body.0);
                    axum::Json(serde_json::json!({
                        "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}]
                    }))
                }
            }),
        );
        let base = spawn_mock(router).await;
        client(&base).translate_text("hello world", "en", "zh").await.unwrap();
        let body = captured.lock().unwrap().clone().unwrap();
        assert_eq!(body["model"], "deepseek-v4-flash");
        assert_eq!(body["temperature"], 0.3);
        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(body["messages"][1]["content"], "hello world");
        assert!(body["messages"][0]["content"].as_str().unwrap().contains("翻译"));
    }
}
