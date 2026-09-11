//! 预设用户认证（部署安全加固）：
//! - 不开放自助注册，仅允许 `config/users.json` 中预先设置的用户登录；
//! - 口令以 PBKDF2-SHA256 哈希存储（`pbkdf2_sha256$<迭代>$<salt_b64>$<hash_b64>`）；
//! - 会话为服务端内存态 + HttpOnly Cookie（服务重启即失效，验证环境可接受）；
//! - 登录接口带滑动窗口限速，防在线爆破。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use hmac::Hmac;
use serde::Deserialize;
use sha2::Sha256;
use subtle::ConstantTimeEq;

/// 会话 Cookie 名
pub const SESSION_COOKIE: &str = "geminus_session";

/// 登录限速参数：每 (IP, 用户名) 与每 IP 各一个窗口
const LOGIN_WINDOW: Duration = Duration::from_secs(10 * 60);
const LOGIN_MAX_PER_USER: usize = 5;
const LOGIN_MAX_PER_IP: usize = 20;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("用户文件不可用: {0}")]
    StoreUnavailable(String),
    #[error("用户名或密码错误")]
    InvalidCredentials,
    #[error("尝试过于频繁，请稍后再试")]
    Throttled,
}

// ────────────────────────── 用户仓（只读） ──────────────────────────

#[derive(Debug, Deserialize)]
struct UsersFile {
    users: Vec<PresetUser>,
}

#[derive(Debug, Deserialize, Clone)]
struct PresetUser {
    username: String,
    password_hash: String,
}

/// 预设用户仓（进程内只读快照）
#[derive(Debug, Clone)]
pub struct UserStore {
    users: Vec<PresetUser>,
}

impl UserStore {
    /// 从 JSON 文件加载；文件缺失/格式错误返回 Err（调用方决定是否禁用登录）
    pub fn load(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path).map_err(|e| format!("读取 {e}"))?;
        let parsed: UsersFile =
            serde_json::from_str(&raw).map_err(|e| format!("JSON 解析失败: {e}"))?;
        if parsed.users.is_empty() {
            return Err("用户列表为空".to_string());
        }
        Ok(Self { users: parsed.users })
    }

    /// 空仓（无预设用户 → 一律拒绝登录）。测试与未配置时的默认态。
    pub fn empty() -> Self {
        Self { users: Vec::new() }
    }

    /// 校验口令（PBKDF2 重算 + 常数时间比较）
    fn verify(&self, username: &str, password: &str) -> bool {
        let Some(user) = self.users.iter().find(|u| u.username == username) else {
            // 仍做一次 dummy 校验，拉平用户不存在/口令错误的耗时
            verify_hash(DUMMY_HASH, password);
            return false;
        };
        verify_hash(&user.password_hash, password)
    }
}

/// 固定格式哑哈希（用户不存在时的等耗时占位）
const DUMMY_HASH: &str = "pbkdf2_sha256$600000$AAAAAAAAAAAAAAAAAAAAAA==$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

fn verify_hash(stored: &str, password: &str) -> bool {
    let parts: Vec<&str> = stored.split('$').collect();
    if parts.len() != 4 || parts[0] != "pbkdf2_sha256" {
        return false;
    }
    let (Ok(iters), Ok(salt), Ok(hash)) = (
        parts[1].parse::<u32>(),
        B64.decode(parts[2]),
        B64.decode(parts[3]),
    ) else {
        return false;
    };
    let mut derived = [0u8; 32];
    let _ = pbkdf2::pbkdf2::<Hmac<Sha256>>(password.as_bytes(), &salt, iters, &mut derived);
    // 常数时间比较（subtle）；b64 解码长度不齐时直接 false
    if hash.len() != derived.len() {
        return false;
    }
    hash.ct_eq(derived.as_ref()).into()
}

/// 生成口令哈希（随机 16 字节盐）。运维新增/轮换预设用户时可写小工具调用。
pub fn encode_password_hash(password: &str, iterations: u32) -> String {
    let salt = uuid::Uuid::new_v4();
    let mut derived = [0u8; 32];
    let _ = pbkdf2::pbkdf2::<Hmac<Sha256>>(password.as_bytes(), salt.as_bytes(), iterations, &mut derived);
    format!(
        "pbkdf2_sha256${}${}${}",
        iterations,
        B64.encode(salt.as_bytes()),
        B64.encode(derived)
    )
}

// ────────────────────────── 会话仓（内存） ──────────────────────────

struct Session {
    username: String,
    expires_at: Instant,
}

/// 服务端会话仓：token → 用户名（滑动过期）
#[derive(Clone, Default)]
pub struct SessionStore {
    ttl: Duration,
    sessions: std::sync::Arc<Mutex<HashMap<String, Session>>>,
}

impl SessionStore {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            sessions: std::sync::Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 创建会话，返回 token
    pub fn create(&self, username: &str) -> String {
        let token = uuid::Uuid::new_v4().to_string();
        self.sessions.lock().unwrap().insert(
            token.clone(),
            Session {
                username: username.to_string(),
                expires_at: Instant::now() + self.ttl,
            },
        );
        token
    }

    /// 查询会话（命中即续期）
    pub fn get(&self, token: &str) -> Option<String> {
        let mut map = self.sessions.lock().unwrap();
        let now = Instant::now();
        map.retain(|_, s| s.expires_at > now);
        map.get_mut(token)
            .filter(|s| s.expires_at > now)
            .map(|s| {
                s.expires_at = now + self.ttl;
                s.username.clone()
            })
    }

    /// 注销会话
    pub fn remove(&self, token: &str) {
        self.sessions.lock().unwrap().remove(token);
    }
}

// ────────────────────────── 登录限速（滑动窗口） ──────────────────────────

/// 简单滑动窗口计数器（内存态）：key → 失败时间戳
#[derive(Clone, Default)]
struct SlidingWindow {
    max: usize,
    events: std::sync::Arc<Mutex<HashMap<String, Vec<Instant>>>>,
}

impl SlidingWindow {
    fn new(max: usize) -> Self {
        Self {
            max,
            events: std::sync::Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 窗口内计数是否已达上限
    fn blocked(&self, key: &str) -> bool {
        self.retain(key) >= self.max
    }

    /// 记录一次失败
    fn record(&self, key: &str) {
        self.events
            .lock()
            .unwrap()
            .entry(key.to_string())
            .or_default()
            .push(Instant::now());
    }

    /// 清除 key（登录成功后调用）
    fn clear(&self, key: &str) {
        self.events.lock().unwrap().remove(key);
    }

    fn retain(&self, key: &str) -> usize {
        let now = Instant::now();
        let mut map = self.events.lock().unwrap();
        let v = map.entry(key.to_string()).or_default();
        v.retain(|t| now.duration_since(*t) < LOGIN_WINDOW);
        v.len()
    }
}

/// 认证服务门面：用户仓 + 会话 + 登录限速
pub struct AuthService {
    users: UserStore,
    sessions: SessionStore,
    per_user: SlidingWindow,
    per_ip: SlidingWindow,
    cookie_secure: bool,
    ttl_secs: u64,
}

impl AuthService {
    pub fn new(users: UserStore, ttl: Duration, cookie_secure: bool) -> Self {
        Self {
            users,
            sessions: SessionStore::new(ttl),
            per_user: SlidingWindow::new(LOGIN_MAX_PER_USER),
            per_ip: SlidingWindow::new(LOGIN_MAX_PER_IP),
            cookie_secure,
            ttl_secs: ttl.as_secs(),
        }
    }

    /// 是否配置了预设用户（false = 登录不可用）
    pub fn is_enabled(&self) -> bool {
        !self.users.users.is_empty()
    }

    /// 无预设用户（登录一律失败）
    pub fn disabled(ttl: Duration) -> Self {
        Self::new(UserStore::empty(), ttl, false)
    }

    /// 登录：成功返回 Set-Cookie 值；失败返回错误（401/429 语义）
    pub fn login(&self, ip: &str, username: &str, password: &str) -> Result<String, AuthError> {
        if self.per_user.blocked(&format!("{ip}|{username}")) || self.per_ip.blocked(ip) {
            return Err(AuthError::Throttled);
        }
        if !self.users.verify(username, password) {
            self.per_user.record(&format!("{ip}|{username}"));
            self.per_ip.record(ip);
            return Err(AuthError::InvalidCredentials);
        }
        self.per_user.clear(&format!("{ip}|{username}"));
        let token = self.sessions.create(username);
        Ok(self.cookie_header(&token))
    }

    /// 由 Cookie 头解析当前用户
    pub fn session_user(&self, cookie_header: Option<&str>) -> Option<String> {
        let token = cookie_header.and_then(|h| find_cookie(SESSION_COOKIE, h))?;
        self.sessions.get(&token)
    }

    /// 注销（由 Cookie 头解析 token）
    pub fn logout(&self, cookie_header: Option<&str>) -> String {
        if let Some(token) = cookie_header.and_then(|h| find_cookie(SESSION_COOKIE, h)) {
            self.sessions.remove(&token);
        }
        self.clear_cookie_header()
    }

    fn cookie_header(&self, token: &str) -> String {
        format!(
            "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
            self.ttl_secs,
            if self.cookie_secure { "; Secure" } else { "" }
        )
    }

    fn clear_cookie_header(&self) -> String {
        format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
    }
}

/// 从 Cookie 头取指定键值
fn find_cookie(name: &str, header: &str) -> Option<String> {
    header.split(';').find_map(|pair| {
        let (k, v) = pair.trim().split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with_user() -> UserStore {
        // pbkdf2_sha256$2$<salt>$<hash>，口令 "secret123"，迭代 2（测试提速）
        let salt = B64.encode(b"0123456789abcdef");
        let mut hash = [0u8; 32];
        let _ = pbkdf2::pbkdf2::<Hmac<Sha256>>(b"secret123", b"0123456789abcdef", 2, &mut hash);
        let encoded = format!("pbkdf2_sha256$2${}${}", salt, B64.encode(hash));
        UserStore {
            users: vec![PresetUser {
                username: "alice".into(),
                password_hash: encoded,
            }],
        }
    }

    #[test]
    fn verify_correct_and_wrong_password() {
        let s = store_with_user();
        assert!(s.verify("alice", "secret123"));
        assert!(!s.verify("alice", "wrong"));
        assert!(!s.verify("bob", "secret123"));
    }

    #[test]
    fn python_generator_hash_is_accepted() {
        // scripts/gen_users.py 生成的哈希必须可被本校验器识别（生成/校验跨语言兼容门禁）
        let s = UserStore {
            users: vec![PresetUser {
                username: "reader1".into(),
                password_hash: "pbkdf2_sha256$2$Zml4ZWR0ZXN0c2FsdDE2$Xfcua7gYnqmRC5MVl0iAlZqxI8dUCwuO4X/yfA2N39Q=".into(),
            }],
        };
        assert!(s.verify("reader1", "compat-check-pw"));
        assert!(!s.verify("reader1", "compat-check-pw2"));
    }

    #[test]
    fn malformed_hash_rejected() {
        let s = UserStore {
            users: vec![PresetUser {
                username: "a".into(),
                password_hash: "not-a-hash".into(),
            }],
        };
        assert!(!s.verify("a", "x"));
    }

    #[test]
    fn dummy_hash_shape_is_valid() {
        // 哑哈希必须能走完整校验路径（b64 合法且 hash 恰为 32 字节），保证耗时拉平
        let hash_part = DUMMY_HASH.rsplit('$').next().unwrap();
        assert_eq!(B64.decode(hash_part).unwrap().len(), 32);
        assert!(!verify_hash(DUMMY_HASH, "x"));
    }

    #[test]
    fn encode_password_hash_roundtrip() {
        let encoded = encode_password_hash("hunter2", 2);
        assert!(verify_hash(&encoded, "hunter2"));
        assert!(!verify_hash(&encoded, "wrong"));
        // 每次生成盐不同 → 串不同但均可校验
        let encoded2 = encode_password_hash("hunter2", 2);
        assert_ne!(encoded, encoded2);
        assert!(verify_hash(&encoded2, "hunter2"));
    }

    #[test]
    fn login_sets_cookie_and_session_roundtrip() {
        let svc = AuthService::new(store_with_user(), Duration::from_secs(60), false);
        let cookie = svc.login("1.2.3.4", "alice", "secret123").unwrap();
        assert!(cookie.contains("geminus_session=") && cookie.contains("HttpOnly"));
        let token = cookie.split(';').next().unwrap().split('=').nth(1).unwrap();
        assert_eq!(svc.session_user(Some(&cookie)), Some("alice".into()));
        assert_ne!(token, "");
        assert_eq!(svc.session_user(Some("geminus_session=bogus")), None);
    }

    #[test]
    fn login_throttles_after_failures() {
        let svc = AuthService::new(store_with_user(), Duration::from_secs(60), false);
        for _ in 0..LOGIN_MAX_PER_USER {
            assert!(svc.login("9.9.9.9", "alice", "bad").is_err());
        }
        // 正确口令也会被限速窗口挡住
        assert!(matches!(
            svc.login("9.9.9.9", "alice", "secret123"),
            Err(AuthError::Throttled)
        ));
        // 其他 IP 不受影响
        assert!(svc.login("8.8.8.8", "alice", "secret123").is_ok());
    }

    #[test]
    fn logout_invalidates_session() {
        let svc = AuthService::new(store_with_user(), Duration::from_secs(60), false);
        let cookie = svc.login("1.1.1.1", "alice", "secret123").unwrap();
        assert!(svc.session_user(Some(&cookie)).is_some());
        svc.logout(Some(&cookie));
        assert_eq!(svc.session_user(Some(&cookie)), None);
    }
}
