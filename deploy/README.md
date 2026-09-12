# geminus 部署工件（自托管部署）

目录结构：

| 文件 | 用途 |
|------|------|
| `geminus-server.service` | systemd unit（EnvironmentFile → `/opt/geminus/config/geminus.env`） |
| `nginx-geminus.conf.template` | nginx 站点模板（80/443 server + ACME 挑战 location，certbot webroot；占位符 `{{SERVER_NAME}}` / `{{CERT_PATH}}` / `{{KEY_PATH}}`） |
| `geminus-locations.conf` | 公共 location 片段：静态长缓存（`/assets/` 显式全量 types 根治 `.mjs` MIME，v2.3 P0）+ `/api/` 反代（SSE 关缓冲、10min 超时、220m 上传、X-Real-IP 覆盖式注入） |
| `package.sh` | 本地 release 构建 + 前端构建 → `dist/geminus-release-<tag>.tar.gz` |
| `smoke.sh` | 部署后冒烟：健康/静态/登录/匿名限流/上传/SSE + 静态资产完整性（index 引用资产 200 + `.mjs` JS MIME，`PUBLIC_BASE` 提供时经公网校验） |
| `cleanup_uploads.sh` | uploads 过期清理（cron 调用，默认 24h） |

## 发布 SOP（阶段 5）

> **v2.3 起：整个发布/回滚流程已脚本化**——
> `deploy/publish.sh <tag> [--skip-tests] [--public-base=URL]`
> （门禁→打包→传输→切链→nginx 同步→重启→双段冒烟）。
> 下列手工步骤保留作参考。

```bash
# 1. 本地：门禁 + 打包
cargo test --workspace && cargo clippy --workspace -- -D warnings
(cd app && pnpm test && pnpm build)
deploy/package.sh v2.1-deploy-rc1

# 2. 传输 + 铺开（user@your-server）
scp dist/geminus-release-<tag>.tar.gz user@your-server:/tmp/
ssh user@your-server '
  set -e
  tar -xzf /tmp/geminus-release-<tag>.tar.gz -C /opt/geminus/releases/
  ln -sfn /opt/geminus/releases/geminus /opt/geminus/current   # 版本切换（回滚 = 指回上一版）
  sudo systemctl restart geminus-server
'

# 3. 冒烟
ssh user@your-server 'BASE=http://127.0.0.1:39321 /opt/geminus/current/deploy/smoke.sh'
#（首次部署把 users.json / geminus.env 放入 /opt/geminus/config/，见下）
```

## 首次部署清单

```bash
ssh user@your-server '
  sudo mkdir -p /opt/geminus/{releases,config,data,uploads,logs}
  # 专用系统用户 + 目录属主（最小权限）
  sudo useradd -r -s /usr/sbin/nologin geminus || true
  sudo chown -R geminus:geminus /opt/geminus/{config,data,uploads,logs}
'
# 预设用户（明文留底在本地 .secrets/，勿传服务器以外的地方）
python3 scripts/gen_users.py --count 3 --prefix reader
scp config/users.json user@your-server:/opt/geminus/config/users.json

# 运行环境（真实密钥；600 权限）
scp .env.local user@your-server:/opt/geminus/config/geminus.env   # 再按 .env.example 补 GEMINUS_* 项
ssh user@your-server 'sudo chmod 600 /opt/geminus/config/geminus.env && sudo chown geminus:geminus /opt/geminus/config/*'

# systemd + nginx（首次先以 HTTP-only 引导上线：80 块含 ACME 挑战 location，
# 供下一步 certbot webroot 签发；证书就绪后再切换完整 80+443 模板，见下节）
sudo cp deploy/geminus-server.service /etc/systemd/system/ && sudo systemctl daemon-reload && sudo systemctl enable --now geminus-server
sudo cp deploy/geminus-locations.conf /etc/nginx/snippets/
sudo tee /etc/nginx/sites-available/geminus >/dev/null <<'NGINX'
server {
    listen 80 default_server;
    listen [::]:80 default_server;
    server_name <your-domain> _;

    location /.well-known/acme-challenge/ { root /var/www/html; }

    include /etc/nginx/snippets/geminus-locations.conf;
}
NGINX
sudo ln -sfn /etc/nginx/sites-available/geminus /etc/nginx/sites-enabled/ && sudo nginx -t && sudo systemctl reload nginx

# uploads 清理 cron（每小时）
(crontab -l 2>/dev/null; echo '0 * * * * /opt/geminus/current/deploy/cleanup_uploads.sh /opt/geminus/uploads') | crontab -
```

## 启用 HTTPS（certbot webroot + 自动续期）

前置：域名 A 记录已指向服务器公网 IP，80/443 端口可公网访问。

```bash
# 1) 签发证书（webroot 复用引导配置的 ACME location；deploy-hook 在续期后自动 reload nginx）
sudo certbot certonly --webroot -w /var/www/html -d <your-domain> --deploy-hook "systemctl reload nginx"

# 2) 切换为完整 80+443 模板（填充三个占位符）
sudo sed -e 's/{{SERVER_NAME}}/<your-domain>/' \
         -e 's|{{CERT_PATH}}|/etc/letsencrypt/live/<your-domain>/fullchain.pem|' \
         -e 's|{{KEY_PATH}}|/etc/letsencrypt/live/<your-domain>/privkey.pem|' \
         deploy/nginx-geminus.conf.template | sudo tee /etc/nginx/sites-available/geminus
sudo nginx -t && sudo systemctl reload nginx

# 3) HTTPS 下会话 Cookie 加 Secure 标记
echo 'GEMINUS_COOKIE_SECURE=1' | sudo tee -a /opt/geminus/config/geminus.env
sudo systemctl restart geminus-server

# 4) 验收 + 续期演练
curl -sI http://<your-domain>/ | grep -i location    # 期望 301 → https
BASE=https://<your-domain> USERS_FILE=.secrets/preset_users.txt deploy/smoke.sh
sudo certbot renew --dry-run
```

自动续期由 certbot 包自带的 `certbot.timer`（systemd timer，每日两次）触发，无需额外 cron。
**踩坑提示**：模板中 ACME 挑战 location 必须同时存在于 80 与 443 两个 server 块——
80 块的 server 级 `if return 301` 在 location 匹配之前执行，LE 的 HTTP-01 校验会跟随
301 到 HTTPS，挑战文件最终由 443 块的 webroot 命中；只写在 80 块时
`certbot renew --dry-run` 会报 404（生产实测）。

## 安全要点（加固要点）

1. **预设用户登录**：`POST /api/auth/login`（PBKDF2-SHA256 600k 迭代），HttpOnly 会话 Cookie；
   无自助注册；登录接口 10 分钟滑动窗口限速（每用户名 5 次 / 每 IP 20 次失败）。
2. **统一页数额度（v2.5）**：游客按 IP 5 页 / 24 小时（`GEMINUS_ANON_PAGE_LIMIT`），
   登录用户按用户名 1000 页 / 24 小时（`GEMINUS_USER_PAGE_LIMIT`），SQLite 持久；
   24 小时滚动窗口自动重置；识别+翻译 1 页 = 1 个计数单位（同 (主体, 文件哈希, 批) 幂等，
   断点续传/缓存秒回不重复扣）。超页文件不整体拒绝——只处理剩余额度内页数
   （MinerU 在线识别前 PDF 预裁剪到额度内页数，不浪费云端资源；翻译循环逐批熔断，
   剩余 0 页才 402）。单段重译仅限已计费文档且每小时 ≤30 次。本地直连（无代理头）不计额。
3. **本地路径门控**：JSON `file_path` 分支默认 403（`GEMINUS_ALLOW_LOCAL_PATH=0`），公网仅
   multipart 上传（扩展名白名单 + magic bytes + 200MB 上限 + 服务端生成存储名）。
4. **IP 识别**：nginx 覆盖式注入 `X-Real-IP: $remote_addr`；后端仅监听 127.0.0.1。
5. **运维端点**：`POST /api/config/mineru`（运行时改上游）需登录。
6. **权限收口（v2.4）**：`GEMINUS_REQUIRE_LOGIN=1` 时匿名仍可上传（响应只含 doc_id 外壳，
   不返回 IR），`GET /api/documents/:id`、`POST /api/translate`、`/api/translate/paragraph`、
   `/api/original/:id`、`/api/resource/:id` 匿名一律 401；前端未登录点「打开/翻译」先弹登录框，
   登录后自动续跑。本地开发保持 0（匿名 + 配额旧行为，存量 E2E 兼容）。
7. **用户文档历史（v2.4）**：`GEMINUS_HISTORY_DB`（默认 `data/history.db`）记录登录用户上传/
   翻译（单用户上限 100 条 FIFO）；`GET /api/history` 匿名 401；历史回载走 `GET /api/documents`
   ——服务重启后内存丢失会按落盘 uploads 文件重解析（MinerU 缓存优先，零云端调用）。
   已知限制：uploads 文件被 `cleanup_uploads.sh` 清理后，对应历史条目回载 404（前端提示重新上传）。

## 回滚 SOP（≤5 分钟）

```bash
ssh user@your-server '
  # current 指回上一版本目录 + restart
  sudo ln -sfn /opt/geminus/releases/<prev-version> /opt/geminus/current
  sudo systemctl restart geminus-server
'
# releases/ 保留最近 3 版，超出清理最旧
```

## 额度手动重置（如需）

```sql
-- ssh user@your-server: sudo -u geminus sqlite3 /opt/geminus/data/auth_quota.db
-- v2.5 起额度 24 小时滚动窗口自动重置；手动清零仅用于运维豁免
DELETE FROM page_usage;                             -- 全部主体
DELETE FROM page_usage WHERE actor = 'ip:x.x.x.x';  -- 指定游客 IP
DELETE FROM page_usage WHERE actor = 'user:alice';  -- 指定登录用户
```
