//! LLM API 配置（.env.local → 环境变量 → 默认值）

use dotenvy;

/// OpenAI 兼容 API 配置
#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub api_base: String,
    pub api_key: String,
    pub model: String,
}

impl ApiConfig {
    /// 从环境变量构造；先尝试加载项目根的 `.env.local`。
    /// - `DEEPSEEK_API_BASE`（默认 `https://api.deepseek.com`）
    /// - `DEEPSEEK_API_KEY`（无默认；缺失时为空串，调用时将失败）
    /// - `DEEPSEEK_MODEL`（默认 `deepseek-v4-flash`）
    pub fn from_env() -> Self {
        // dotenvy 默认找 .env；本项目密钥在 .env.local
        let _ = dotenvy::from_filename(".env.local");
        Self {
            api_base: std::env::var("DEEPSEEK_API_BASE")
                .unwrap_or_else(|_| "https://api.deepseek.com".to_string()),
            api_key: std::env::var("DEEPSEEK_API_KEY").unwrap_or_default(),
            model: std::env::var("DEEPSEEK_MODEL")
                .unwrap_or_else(|_| "deepseek-v4-flash".to_string()),
        }
    }

    pub fn new(api_base: impl Into<String>, api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_base: api_base.into(),
            api_key: api_key.into(),
            model: model.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn reads_env_overrides() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("DEEPSEEK_API_BASE", "http://localhost:9999");
        std::env::set_var("DEEPSEEK_API_KEY", "sk-test");
        std::env::set_var("DEEPSEEK_MODEL", "test-model");
        let cfg = ApiConfig::from_env();
        assert_eq!(cfg.api_base, "http://localhost:9999");
        assert_eq!(cfg.api_key, "sk-test");
        assert_eq!(cfg.model, "test-model");
        std::env::remove_var("DEEPSEEK_API_BASE");
        std::env::remove_var("DEEPSEEK_API_KEY");
        std::env::remove_var("DEEPSEEK_MODEL");
    }

    #[test]
    fn defaults_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("DEEPSEEK_API_BASE");
        std::env::remove_var("DEEPSEEK_MODEL");
        let cfg = ApiConfig::from_env();
        assert_eq!(cfg.api_base, "https://api.deepseek.com");
        assert_eq!(cfg.model, "deepseek-v4-flash");
    }
}
