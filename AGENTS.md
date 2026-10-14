# AGENTS.md — Diptych / geminus 项目知识库

> 本文件是给协作代理的项目上下文：结构、开发工作流、部署到远程服务器（opc01）的流程与踩坑口径。
> 内容以仓库内实际文件为准（每条后括注来源），改动代码或发布前先读这一份。
> 注意：本文件描述"怎么做"，具体版本状态请用命令实时查询（`git log` / `git tag` / `readlink current`）。

## 一、项目是什么

**Diptych**（内部代号 `geminus`）是版式保留、左右对照的 PDF / DOCX 双语翻译阅读器：公式、表格、算法块、
参考文献原样呈现，段落级联动高亮，由 LLM 驱动翻译。本仓库是 **Community Edition（Apache-2.0）**，
remote 为 `git@github.com:wx-rdc/diptych.git`（origin/main）；TwinDoc overlay 排版引擎、多租户计费、
云端术语表等属闭源 Diptych Pro，见 [OPEN_CORE.md](OPEN_CORE.md)。

公网验证环境：`https://x.wx-rdc.top`（节点 opc01，Ubuntu 22.04 + nginx，已启用 TLS，http 一律 301→https，
**POST 不跟随跳转 → 冒烟/验收/curl 一律写 https**）。

解析-翻译数据流：

```
上传 → POST /api/parse ─┬─ MinerU 云端 VLM（splitter→上传→轮询→下载→adapter→merger→repair）
                        └─ 内置 builtin 解析器（pdf/ 或 parser/ DOCX）
                     ↓ IR（Document/Block，含公式、表格、算法块、资源）
     POST /api/translate（SSE 逐批）→ 分块 → {{Mn}}/{{Pn}} 占位保护 → LLM → rebuild 回填 → SQLite 缓存
                     ↓
     前端双栏渲染：左 pdf.js 原文 canvas / 右语义流式译文，hover 联动高亮 + click 双侧居中
```

## 二、代码结构

| 层 | 位置 | 关键模块 |
|---|---|---|
| 纯业务核心（无 HTTP 壳） | `crates/geminus-core/src/` | `parser/`（DOCX OOXML→IR：document.rs/xml_tree.rs）、`pdf/`（builtin 解析：segmenter/extractor/layout/converter + `crop_pdf` 额度预裁剪）、`mineru/`（client/splitter/adapter/merger/raw/**repair**——U+FFFD 字形回填）、`translate/`（chunk/prompt/math_slot/math_guard/algorithm/**rebuild**——令牌校验与降级）、`cache.rs`（SQLite 翻译/解析缓存）、`model.rs`（IR 定义）、`config.rs`、`audit.rs`、`math/` |
| HTTP + SSE 壳 | `crates/geminus-server/src/` | `lib.rs`（axum 路由 + 编排，单文件最大）、`auth.rs`（预设用户 PBKDF2-SHA256 600k + HttpOnly 会话 Cookie + 登录限速）、`quota.rs`（统一页数额度，SQLite 24h 滚动窗口 + 批级幂等）、`access.rs`（`doc_access` 表：access_token 三选一防盗链 + TTL cron）、`history.rs`（登录用户文档历史）、`tasks.rs`（自实现 `CancelToken` + `TaskRegistry` + `page_batches`） |
| 前端 | `app/src/` | Vite + React 18 + TS + Tailwind；`components/`（Pane/BlockRenderer/PdfCompareView/PdfOriginalViewer/ParagraphToolbar/QuotaChip/StatusChip/ToolbarSlim/QuotaExceededCard…）、`hooks/`（`usePaneSync`——hover 高亮 + click 居中、`useAutoPipeline`——零点击流水线、`usePdfRenderer`、`useReadingFontSize`）、`services/`（backend/auth/sse/docToken/history/guestDocs）、`utils/`（anchor/geometry/inlineHtml/inlineMath/tableHtml） |
| 测试 | `crates/*/tests/`（Rust 集成）、`app/src/**/__tests__/`（vitest/jsdom）、`scripts/e2e_*.mjs`（Playwright 浏览器级） |
| 夹具 | `test/fixtures/`（docx / mineru 离线缓存 JSON+图 / pdf）、`data/mineru_cache/`（离线重放，零云端调用） |
| 部署工件 | `deploy/` |

API 面（`lib.rs` 的 `Router`，全部 `/api` 前缀）：`health`、`auth/login|logout|me`、`parse`（JSON 本地路径
\| multipart）、`translate`（SSE）、`translate/:doc_id/cancel`、`translate/paragraph`、`documents/:id`、
`history`、`quota`、`quota/usage`、`resource/:doc_id/:rid`、`original/:doc_id`、`config/mineru`（需登录）、
`debug/audit`、`debug/coverage`（覆盖度终验用）。

## 三、配置与两套权限口径

`.env.example` 是唯一权威清单（复制为 `.env.local`；真实密钥不入 git，服务器上用
`/opt/geminus/config/geminus.env`，600 权限）。本地开发 vs 生产的关键差异：

| 变量 | 本地开发 | 生产（opc01） | 影响 |
|---|---|---|---|
| `GEMINUS_REQUIRE_LOGIN` | `0`（匿名 + 配额旧行为，存量 E2E 兼容） | `0`（v2.4.1+ 默认：配额内匿名开放试用 + access_token 防盗链 + 熔断）；`=1` 为旧收口模式（匿名查看/翻译 401） | smoke.sh §6.5/§6.6 两段按此自动开关 |
| `GEMINUS_ALLOW_LOCAL_PATH` | `1`（JSON `file_path` 分支可用） | `0`（默认 403，防任意文件读取） | 公网只走 multipart |
| `GEMINUS_ANON_PAGE_LIMIT` / `GEMINUS_USER_PAGE_LIMIT` | E2E 常设 `100000` 放开 | `5` / `1000`（24h 滚动窗口） | 本地放开时限流断言必假通过 |
| `GEMINUS_STATIC_DIR` | `app/dist`（后端直接托管） | `web`（发布包内前端产物） | — |
| `GEMINUS_COOKIE_SECURE` | 不设 | `1`（HTTPS 下会话 Cookie 加 Secure） | — |

额度语义（v2.5 统一额度，别改口径）：**识别 + 翻译 1 页 = 1 计数单位；解析与查看永不扣费**；
超页文件不整体拒绝，只处理剩余额度内页数（MinerU 在线识别前先 `crop_pdf` 预裁剪，不浪费云端配额）；
唯一扣费点是翻译循环 `commit_chunk`（`BEGIN IMMEDIATE` 事务，批级幂等键 `(actor, content_hash, batch)`
→ 断点续传/缓存秒回不重复扣）；剩余 0 页才 402。本地直连（无代理头）不计额。

## 四、开发工作流

日常循环：

```bash
cp .env.example .env.local          # 填 DEEPSEEK_API_KEY（MINERU_API_KEY 可选）
cd app && pnpm install && pnpm build && cd ..   # 产物 app/dist，后端直接托管
cargo run -p geminus-server         # http://127.0.0.1:39321
(cd app && pnpm dev)                # 5173，/api 代理到 39321（改前端时用这个）
```

**规格驱动的迭代流程**（本仓库既有惯例，新迭代请沿用）：

1. 一份计划文档：`docs/superpowers/plans/<日期>-<主题>.md` —— 目标/架构/技术栈/文件动作表 +
   复选框步骤（`- [ ]`），每个步骤先写失败测试（TDD）。
2. 逐任务实现并勾选，E2E 断言同步改（不要留"计划说改但代码没改"的悬挂项）。
3. 全部门禁 + 冒烟绿 → 发布（见第五节）。
4. 回写 [docs/IMPLEMENTATION_LOG.md](docs/IMPLEMENTATION_LOG.md)：任务完成情况表、最终验证矩阵、
   **计划偏差记录**（这个仓库很看重偏差说明）、新增/变更清单。
5. 公网反馈类问题另存 `docs/_local/feedback_*.md`（`_local/` 被 gitignore，属私有）——
   **发布时不要 stage 它们**。

四个门禁（CI 与 `publish.sh` 同口径，缺一不算绿）：

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -D warnings    # CI 里是 --workspace -- -D warnings；--all-targets 覆盖 examples/tests
(cd app && pnpm test)                                  # vitest + jsdom
(cd app && pnpm build)                                 # tsc --noEmit && vite build
```

CI（[.github/workflows/ci.yml](.github/workflows/ci.yml)）只在 push main / PR 时跑这四个，**不含任何部署**。

浏览器级验证：`scripts/e2e_*.mjs`（Playwright，`NODE_PATH=$(npm root -g) node scripts/e2e_v2_4.mjs`）。
多数需要先起 release 服务器并放开 `GEMINUS_ANON_PAGE_LIMIT=100000 GEMINUS_ALLOW_LOCAL_PATH=1`；
`e2e_v2_4.mjs` 自起专项实例（端口 39322，db/uploads 隔离到 tempdir，`MINERU_API_KEY` 置空全走 builtin，
零云端调用）。选脚本的口径：按被测面挑最近的 e2e，改 UI 交互至少要跑 `e2e_v2_4.mjs`。

一次性工具：`scripts/gen_users.py`（生成 `config/users.json` PBKDF2 哈希 + `.secrets/preset_users.txt`
明文留底，**只留本地**）、`scripts/gen_pdf_fixtures.py`、`scripts/audit_alignment.mjs`、
`scripts/audit_coverage.mjs`、`scripts/backfill_raw_cache.py`。
调试探针：`crates/geminus-core/examples/*.rs`（`cargo run -p geminus-core --example <name> -- <args>`）。

## 五、部署到 opc01 的工作流

### 5.1 线上拓扑

```
浏览器 ──HTTPS──► nginx (opc01)
   ├── 静态：root /opt/geminus/current/web（/assets/ 长缓存 30d immutable）
   ├── /api/ 反代 ──► geminus-server（systemd，仅监听 127.0.0.1:39321）
   │        ├── SQLite：data/{translate_cache,auth_quota,history}.db + doc_access
   │        ├── uploads/（cron 每小时清 24h 前文件）
   │        └── 上游 MinerU 云端 API + LLM API
   └── /.well-known/acme-challenge/（80 与 443 两个 server 块都要有，见 5.5）
```

systemd 单元（`deploy/geminus-server.service`）：`User=geminus` 专用系统用户、
`EnvironmentFile=/opt/geminus/config/geminus.env`、`Restart=always`、`MemoryMax=2G`、
`ProtectSystem=full` + `ReadWritePaths=data/uploads/logs`、日志进 journal（`journalctl -t geminus-server`）。

### 5.2 发布（一条命令）

> **项目技能**：`.zcode/skills/geminus-deploy/SKILL.md` 是本流程的权威细则（发布/回滚/人工验收清单/故障表）。
> 注意 `/.zcode` 在 `.gitignore` 里，该技能属机器本地资产、不随 clone 分发——本文件是其仓库内替代说明。

```bash
# 前置：工作目录=项目根、分支 main、工作区干净（feedback_*.md 别 stage）
GEMINUS_SERVER=opc01 deploy/publish.sh <tag> --public-base=https://x.wx-rdc.top
# 门禁刚跑过时加 --skip-tests
# 回滚：GEMINUS_SERVER=opc01 deploy/publish.sh --rollback <上一个 tag>   （目标 ≤5 分钟）
```

`deploy/publish.sh` 的 8 段（出问题按段排查）：

1. 本地门禁（cargo test/clippy + 前端 test/build）。
2. `deploy/package.sh <tag>`：`cargo build --release -p geminus-server` + 前端构建 →
   `dist/geminus-release-<tag>.tar.gz`，包内**平铺**：`geminus-server` 二进制、`web/`、
   `geminus-server.service`、`geminus-locations.conf`、`nginx-geminus.conf.template`、`smoke.sh`、
   `cleanup_uploads.sh`、`geminus.env.example`、`VERSION`。**本地构建**（与目标机同架构 x86_64），
   目标机只留最小运行时。
3. scp → `opc01:/tmp/` → **sudo 移入** `releases/<tag>`（`/opt/geminus/releases` 归 `geminus` 用户，
   ssh 用户直写会 Permission denied）+ `chown -R geminus:geminus`。
4. `ln -sfn releases/<tag> current` 切链 → 同步 `geminus-locations.conf` 到 `/etc/nginx/snippets/` →
   `nginx -t` → reload → `systemctl restart geminus-server`。
5. 幂等铺放冒烟夹具（`/tmp/geminus_smoke/{simple,large}.pdf`）与凭据
   （`/opt/geminus/config/preset_users.txt`，ssh 用户属主 600）。
6. 冒烟第 1 段：后端直连 `BASE=http://127.0.0.1:39321`（smoke.sh 全 8 节，须 **0 失败**）。
7. 冒烟第 2 段（给了 `--public-base` 时）：经 nginx 公网入口，含静态资产 200 + `.mjs` JS MIME 断言。
8. `releases/` 自动只保留最近 3 版。

**tag 命名**沿用 `<版本>-<主题>-rcN`（例：`v2.8.2-clean-preview-rc1`）。发布后务必打 tag——
线上一度出现 `releases/<tag>` 有目录、git 里却找不到对应提交/tag 的脱节。

### 5.3 冒烟覆盖（`deploy/smoke.sh`，`结果：N 通过 / 0 失败` 才算过）

1 健康检查 · 2 SPA 入口 · 3 认证（匿名 me / 登录 / 错口令 401）· 4 统一页数额度（游客 5 页/24h、
解析不扣费 remaining 满额、登录 1000 页、JSON 本地路径门控）· 5 multipart 上传 + 非白名单扩展名 400 ·
6 SSE 有事件流出（进度流不被缓冲）· 6.5 权限收口（`require_login=1` 时自动生效：匿名外壳不泄露 IR、
查看/翻译 401、history 200）· 6.6 匿名试用与防盗链（access_token 下发、跨 IP 带 token 200、
无 token 403、`/api/quota/usage` 含 file_name、cancel 幂等 404）· 7 静态资产完整性
（index 引用资产全 200 + `.mjs` MIME，`PUBLIC_BASE` 未设时跳过）。

### 5.4 发布后人工验收（浏览器 `https://x.wx-rdc.top`）

1. 登录（凭据 `.secrets/preset_users.txt`）→ 上传 `test/fixtures/pdf/simple.pdf` → 左栏 pdf.js canvas 正常。
   注意 7.3MB 原文要等 20–60s 才出 canvas，别在 5s 内判失败。
2. 匿名零点击：登出后选择文件 → 不点任何按钮自动进双栏并开始翻译（顶栏「游客模式 · 额度内免登录 / 剩余 N 页」）；
   额度耗尽 → QuotaExceededCard（已译部分可读）；翻译中 [⏹ 停止] → [▶ 继续翻译]（续译秒回）；
   重新登录 → 「🕘 历史」条目秒开回载。
3. `curl -s https://x.wx-rdc.top/ | grep -c pdf.worker` → **0**（worker 已 inline 打包）。
4. 任一 `/assets/*.js` 响应头 `Content-Type: text/javascript`。
5. 大文档回归（`test/ieee_access_main_revised.pdf`）：右栏滚到底——参考文献 35 条可见（B3）、
   作者行上标（B1）、表格无字面 LaTeX（B2）。
6. 段落交互（v2.7 口径）：hover 左栏段落 → 右栏对应段落同步泛浅蓝底（**仅高亮，不驱动滚动**）；
   单栏滚动不影响另一栏；点击段落 → 双侧平滑居中对齐；hover 译文段落 → 段落**右外侧**垂直图标列
   （复制原文/复制译文/重译本段），不遮挡文字。
7. 服务端覆盖度终验：`ssh opc01 "curl -s 'http://127.0.0.1:39321/api/debug/coverage?doc_id=<id>'"`
   → 期望 `violations=[]` 且 char/area ≥ 0.98/0.95。

首次部署清单、HTTPS 签发/续期、额度手动重置 SQL 见 [deploy/README.md](deploy/README.md)。

### 5.5 踩坑口径（实测换来的，别再踩）

| 症状 | 根因与处置 |
|---|---|
| `scp: Connection closed` | `publish.sh` 默认值是占位符 `user@your-server`，**必须带 `GEMINUS_SERVER=opc01` 前缀**（发布与回滚都要） |
| 公网冒烟整段 301 假失败（JSONDecodeError / 上传"期望 200 实际 301"） | `--public-base` 写了 `http://`；公网已强制 https 且 POST 不跟随。改 https 重跑即可，服务本身没问题 |
| 发布退出码 0 但实际失败 | 脚本输出经管道（`\| tail`）时退出码被吞——**以关键行判读**：`==> 传输 <tag> → opc01`、`version: <tag>`、`结果：N 通过 / 0 失败` |
| 冒烟 §7 MIME 红 | `/etc/nginx/snippets/geminus-locations.conf` 没同步新版（须含 `types` 块 `text/javascript js mjs`）。nginx 的 `types` 块会整体覆盖继承，所以 `/assets/` 里必须自足列举 Vite 产物全部扩展名 |
| SSE 卡死（翻译进度不动） | `locations.conf` 的 `/api/` 段丢了 `proxy_buffering off`（另需 `proxy_cache off`、600s 超时、HTTP/1.1） |
| 匿名限流断言"必过" | 本地若以 `GEMINUS_ANON_PAGE_LIMIT=100000` 启动，该项必 200——**公网跑才是真验收** |
| 跨 IP 盗链断言不一致 | 直连后端伪造 `X-Real-IP` 能测出 403；经 nginx 时 `$remote_addr` 是**覆盖式注入**（不用 `$proxy_add_x_forwarded_for`，防伪造），伪造头无效 → 同 IP 命中 `uploader_ip` 放行是正确行为 |
| `tar` 一步 SIGPIPE 炸掉发布 | 别用 `head` 截 `tar -tzf`（读满即退出），用 `sed -n '1,5p'` 全量消费 stdin |
| `certbot renew --dry-run` 报 404 | ACME 挑战 location 必须同时存在于 80 和 443 两块：80 块的 server 级 `if return 301` 在 location 匹配前执行，LE 跟随 301 后由 443 块 webroot 命中 |
| MinerU 解析失败 | `journalctl -t geminus-server \| grep mineru`；`-60018` = 上游日额度耗尽（次日自愈），auto 模式回退 builtin 解析 |
| 历史条目回载 404 | uploads 文件被 `cleanup_uploads.sh`（默认 24h）清掉后无法回源，前端提示重新上传——已知限制 |
| 译文出现大量 `<sub>/<sup>` | MinerU VLM 把正文误判为上/下标（实测某 TeX 字体论文 7545 个 `<sub>`）→ adapter 层噪声清洗（条目级 + 文档级两级阈值）。译文出现 `U+FFFD` → `mineru/repair.rs` 用 builtin 解析做参考源逐段回填 |

## 六、实时状态怎么查（别把本文件当状态源）

```bash
git log --oneline -8 && git tag --sort=-creatordate | head -5      # 已发布版本
git status --short                                                  # 未提交/未跟踪
ssh opc01 'readlink /opt/geminus/current; systemctl is-active geminus-server; ls /opt/geminus/releases'
ls dist                                                             # 本地发布包（dist/ 不入 git）
```

## 七、边界与约定

- 密钥、`.env.local`、`config/`、`.secrets/`、`data/`、`uploads/`、`logs*`、`_local/`、`.zcode/`、`dist/`
  一律不入 git（见 `.gitignore`）。`docs/_local/` 是私有设计文档区，README 里的开源文档指向 `docs/`。
- 版权论文原文（`test/*.pdf` 顶层夹具）不入公开仓，E2E 需要时本地自备。
- commit message 用中文 conventional commits（`feat(ui):` / `fix(mineru):` / `docs(plan):` / `test(e2e):`），
  一个提交一个关注点；UI 改动同步改 e2e 断言，部署改动同步 `deploy/` 工件与 smoke.sh。
- 商标："Diptych" 名称与标识不随 Apache-2.0 授权，见 [TRADEMARK.md](TRADEMARK.md)。
