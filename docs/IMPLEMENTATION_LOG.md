# Geminus 实施日志

> 面向社区版的实现记录：按迭代章节记录任务完成情况、验证结果与计划偏差。
> （私有主仓的历史日志未随开源发布；本文件自 v2.4 社区版起建档。）

---

## v2.4（第二轮迭代）：零点击流水线 · 匿名配额熔断 · 任务控制流

- **日期**：2026-09-11 | **分支**：`feature/v2.4-zero-click-quota`
- **计划**：`docs/_local/v2_4_plan.md`（7 任务 / 里程碑 M0-M4）
- **定位**：在上一轮 v2.4（权限收口 + 历史 + Hover 同步，tag `v2.4-ux-rc1`）之上
  反转权限方向——匿名在配额内开放试用，移除强制登录；零点击自动化；翻译任务可取消。

### 任务完成情况

| 任务 | 内容 | 状态 | 验证 |
|------|------|------|------|
| T0 | 基线修复（私有文档缺失跳过）+ 分支 | ✅ | parser_integration 5/5 |
| T1 | 权限解耦：access_token 三选一防盗链；移除默认强制登录 | ✅ | access_control 7 项 + access.rs 单测 5 项 |
| T2 | 历史记录（上一轮已完成，本轮确认在位） | ✅（沿用） | permissions_history 5 项 |
| T3 | 零点击流水线：useQuotaGuard + useAutoPipeline；移除「打开/翻译」按钮；402 识别 | ✅ | hooks 13 项 + App 零点击语义 10 项 |
| T4 | Hover 同步（上一轮已完成，本轮确认在位） | ✅（沿用） | useHoverDrivenSync 10 项 |
| T5 | 配额三段式 + 任务控制流：批级实扣熔断 / cancel / 断点续传 / SSE 新事件 | ✅ | translate_control 5 项 + quota 单测 6 项（10 并发零超发）+ tasks 单测 5 项 |
| T6 | 软性引导 UI：QuotaIndicator / QuotaExceededCard / TranslateControl / 匿名 TTL cron | ✅ | 组件测试 10 项 + TTL 集成 1 项 |
| T7 | 全量回归 + 发布 | ✅（见下） | 门禁全绿 + 冒烟 25/25 |

### 最终验证矩阵（对照计划 §四）

| 维度 | 计划标准 | 实际 |
|------|----------|------|
| Rust 测试 | ≥ 300 全绿 | **321 全绿**（0 fail） |
| 前端测试 | ≥ 145 全绿 | **169 全绿** |
| E2E ≥ 62 场景 | playwright 全绿 | **偏差**：社区版无 playwright 基础设施（未随开源仓分发）→ 以 Rust HTTP 集成测试（access_control 7 + translate_control 5）+ App 零点击语义组件测试覆盖同等断言（熔断 SSE / cancel 生命周期 / 盗链 403 / 402 预检 / 断点续传 LLM 调用一一对应） |
| 配额事务 | 10 并发零超发 | ✅ `commit_chunk_no_oversell_under_concurrency` |
| 安全 | 盗链 403 / 402 不触 MinerU | ✅ 集成测试 + 冒烟 6.6 段 |
| 门禁 | clippy / build 0 警告 | ✅ `--workspace --all-targets -D warnings` 0 报错；release build 通过 |
| 冒烟 | 双段 + v2.4 增补 | ✅ 本地 25/25（含 402 结构化 / token 放行 / 盗链 / cancel 幂等） |

### 核心设计落地

1. **配额三段式**（计划 §2.1）：解析前预检（PDF lopdf 真实页数 / DOCX 估算×1.2 上浮 → 402 结构化 `{limit,used,required}`，MinerU 不被调用）；翻译循环逐批实扣（`BEGIN IMMEDIATE` 事务）；批间熔断检查（`limit_reached` SSE 事件）。查看原文 IR 免费。
2. **批级幂等键**：`anon_page_usage` 主键升级 `(ip, content_hash, batch)`（旧表自动迁移）——断点续传重译已付批零重复扣费；全 cached 批免费秒回（「缓存秒回不扣配额」语义）。
3. **任务控制流**：自实现 `CancelToken`（AtomicBool+Notify，无 tokio-util 依赖）+ `TaskRegistry`（同 doc 重注册防御性取消旧任务 / finally 移除防泄漏）；批循环三控制点（显式取消 / SSE 断开止损 / 熔断）；`POST /api/translate/:doc_id/cancel`（三选一校验）。
4. **零点击**：选择文件 → `useQuotaGuard` 本地预检（pdf.js 读页数，=remaining 放行 / +1 拦截）→ 自动解析 → 自动翻译（仅首次）；移除「打开 / 开始翻译」按钮与前端 requireAuth 拦截；本地路径输入保留 Enter 触发（开发分支）。
5. **防盗链**：上传响应顶层注入 `access_token`（UUID v4，token 首次生成不轮换——重传/回源不失效；uploader_ip 每次更新）；`GET /api/documents` `/api/resource` `/api/original` `POST /api/translate` `/paragraph` `/cancel` 统一三选一校验（Session / token / uploader_ip；直连本机放行=本地开发口径）。

### 计划偏差记录

1. **`ALTER TABLE documents` 不适用**：社区版 `documents` 为内存 HashMap（无 SQLite 表）→ 独立建 `doc_access` 表（doc_id / access_token / uploader_ip / is_anonymous / created_at）。
2. **playwright E2E 基础设施缺失**（见验证矩阵）→ Rust HTTP 集成测试 + 组件测试等价覆盖。
3. **翻译循环伪代码**（顺序 `queue.next()`）与实际 core 并发模型（JoinSet+Semaphore）不符 → server 层按页分批（`page_batches`：MinerU PDF 按 render_meta 页聚合 / 其他按估算页数等分，单批 1 页计费粒度），批内仍由 core 并发调度；批间 barrier 不减吞吐（Semaphore 才是瓶颈）。core 与 LLM/MinerU 客户端零改动 ✓。
4. **计划 E2E 断言「剩 3 页译 10 页」与预检语义内在矛盾**（10 页 > remaining=3 会在解析预检被 402 拦截）→ 熔断作为「解析免费+翻译扣费」模型的常规路径验证：先解析多文档（免费）→ 翻译消耗额度 → 第二篇中途熔断（`anon_translate_limit_reached_after_quota`）。
5. **`docs/IMPLEMENTATION_LOG.md` / `docs/visual-checklist.md` 未随社区版分发** → 本文件新建（v2.4 章节）；走查清单以组件测试 + 冒烟脚本固化。
6. **require_login 保留为部署开关**（`GEMINUS_REQUIRE_LOGIN=1` 旧行为，回归测试保护）；新语义为默认（0）。发布时服务器 env 需置 0 或删除该行。

### 新增/变更清单

- `crates/geminus-server/src/access.rs`（新）：doc_access 表 + token 校验 + TTL 清理
- `crates/geminus-server/src/tasks.rs`（新）：CancelToken + TaskRegistry + page_batches
- `crates/geminus-server/src/quota.rs`：commit_chunk（事务+批级幂等）/ remaining / 旧表迁移
- `crates/geminus-server/src/lib.rs`：三选一 guard、402 预检、翻译批循环重构、cancel 端点、
  `/api/quota`、匿名 TTL cron、解析不实扣、access_token 下发
- `app/src/services/docToken.ts`（新）/ `backend.ts`：token 留存与附带、QuotaLimitError /
  TranslateCancelledError / fetchQuota / cancelTranslate
- `app/src/hooks/useQuotaGuard.ts` / `useAutoPipeline.ts`（新）
- `app/src/components/QuotaIndicator.tsx` / `QuotaExceededCard.tsx` / `TranslateControl.tsx`（新）；
  Toolbar 移除「打开 / 开始翻译」按钮
- `app/src/App.tsx`：零点击流水线接线、熔断/暂停态、402→QuotaExceededCard
- `deploy/smoke.sh`：429→402 断言 + 6.6 匿名试用与防盗链段

---

## v2.4.1 补丁：游客额度体验闭环（公网反馈修复）

- **日期**：2026-09-12 | **tag**：`v2.4.1-zero-click-rc2`
- **问题**（公网实测）：
  1. 匿名提交超限 PDF 弹出额度卡片，点「仅查看已译部分」→ 界面无任何变化
  2. 「已使用 3 页」无法追溯——看不到这 3 页翻译了什么
  3. 额度指示只有数字，无去向明细

### 根因
1. **预检卡片来源混淆**：QuotaExceededCard 不区分「翻译熔断」（有已译部分）与「解析预检 402」
  （本文档根本没打开）——后者显示「仅查看已译部分」按钮是欺骗性的，点击仅关卡片；
2. **回访被二次拦截**：额度耗尽后重发翻译，开流前置 402 与批间熔断**均不豁免已付费批**，
  「已译部分可继续阅读（续译）」承诺落空；
3. **额度无去向**：匿名额度按 IP 静默记账，UI 无任何「翻译过什么」的呈现（匿名无历史，
  会话内 translatedMap 单文档覆盖即丢）；另发现前端本地预检 pdf.js 调用未配置 worker
  （worker 仅在 usePdfRenderer 中配置）——读页静默失败 → 放行到后端 402，加重混淆。

### 修复
| 项 | 内容 | 验证 |
|----|------|------|
| 后端 | 熔断检查点豁免已付费批（already_charged_batch）+ 开流前置 402 同豁免（has_any_charged_batch）→ 回访/续译已译文档全文缓存秒回（零新增 LLM 调用） | 集成：额度耗尽回访 complete、LLM 调用数不变；未付费新文档仍 402 |
| 后端 | doc_access 表增 file_name（自动迁移）+ `GET /api/quota/usage`：按 IP 返回匿名上传文档清单 | 集成：同 IP 列表 + IP 隔离 |
| 前端 | QuotaExceededCard 拆 variant：`limit`（熔断，承诺可读 + 「查看已译部分」）／`precheck`（明示「该文件未开始翻译」+ 剩余/需要页数 + 「换个文件试试」，不承诺已译可读） | 组件测试 4 项 |
| 前端 | localPdfPages 复用 loadPdfjs（带 inline worker 配置）+ fileBytes FileReader 兜底（jsdom/旧内核缺 File.arrayBuffer）——本地预检不再静默失效 | 集成：本地拦截（11>2）弹 precheck 卡片且零上传请求 |
| 前端 | sessionDocs（sessionStorage 游客翻译记录）+ GuestDocsPanel（本地记录 ∪ /api/quota/usage 合并去重）+ QuotaIndicator 可点（「剩余 N 页 · 去向 ↗」）——额度去向可追溯、一键回访（回访零配额消耗） | 组件测试 3 项 + App 集成 |

### 门禁
Rust **323** 全绿 / 前端 **175** 全绿 / clippy --all-targets 0 警告 / release build 通过；
本地冒烟补 usage 断言。
