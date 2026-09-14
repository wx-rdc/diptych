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

---

## v2.5：统一额度重构——24h 滚动窗口 · 游客/登录同口径 · 额度内部分处理

- **日期**：2026-09-12
- **问题**（用户反馈）：
  1. 免费额度不会在 24 小时后重置（`used_pages` 全历史累加，永不恢复）；
  2. 提交 11 页 PDF 提示「需要 22 页」额度——计量口径让用户认为识别与翻译被分开计算；
  3. 超页文件被整体拒绝（前端本地预检拦截上传 + 后端解析预检 402），无法在额度内试用；
  4. 后端超发处理：翻译循环逐批熔断虽止损 LLM 调用，但 MinerU 云端识别仍全册消耗。

### 方案（用户需求 1:1 落地）

| 需求 | 实现 |
|------|------|
| ① 统一免费/登录额度管理 | `QuotaActor`（Guest(ip) / User(name)）统一计费主体；同一张 `page_usage` 表、同一套扣减/熔断/幂等逻辑 |
| ② 游客 5 页 / 24h，登录 1000 页 / 24h | `GEMINUS_ANON_PAGE_LIMIT=5` / `GEMINUS_USER_PAGE_LIMIT=1000`；`used_pages` 只统计 `charged_at > now-24h` 的扣减（滚动窗口逐页恢复，`reset_at` 返回最早恢复点） |
| ③ 超页文件只处理额度内页数 | 解析不再按页数 402：剩余 0 页才 402；否则 `cap = min(文件页数, 剩余)`——MinerU 在线识别前 `pdf::crop_pdf` 预裁剪到前 cap 页（builtin/DOCX 本地解析便宜不裁，翻译循环按批截断） |
| ④ 识别+翻译 1 页 = 1 计数单位 | 唯一扣费点是翻译循环 `commit_chunk`（每批 1 页）；解析/查看零扣减；批级幂等键 `(actor, hash, batch)`——断点续传/缓存秒回不重复扣；幂等限定在窗口内（滑出窗口 = 新计费周期，`REPLACE` 覆盖旧键） |
| ⑤ 后端不超发处理 | MinerU 只送裁剪件（集成测试断言上游收到的 PDF 页数 == 剩余额度）；翻译循环控制点 ③ 逐批熔断，`limit_reached` 事件携带 `translated_pages`（已译部分保留可读） |

### 关键变更

- `quota.rs`：`anon_page_usage`（ip 主键）→ `page_usage`（actor 主键，key 前缀 `ip:` / `user:` 隔离命名空间）；旧表数据自动迁移（旧记录带 charged_at 滑出窗口 = 一次性重置，修复「永不重置」存量问题）；7 天前记录开库顺手清理
- `lib.rs`：`quota_actor()` 统一主体解析（登录用户名 / 匿名 IP / 本地直连不计）；`allowed_pages` + `crop_pdf_for_quota`（裁剪件名沿用原标题主干 + 短 uuid，标题不劣化）；MinerU 缓存命中/离线重放为零 API 路径不裁剪（用全量 IR，翻译仍按额度截断）；`/api/quota` 游客与登录统一 finite 口径（+`reset_at`）；402 与 `limit_reached` 载荷 +`user`/`translated_pages`/`reset_at`
- `geminus-core/src/pdf/mod.rs`：`crop_pdf(src, keep, dest)`（lopdf 删页另存）
- 前端：删除 `useQuotaGuard`（本地拦截不复存在——超页文件直接上传，部分处理由服务端负责）；`useAutoPipeline` 移除 precheck/blocked 阶段；`QuotaExceededCard` 统一单 variant（24h 恢复 + 已译 N 页可读 + 登录 1000 页/天引导；登录用户文案不含登录按钮）；`QuotaIndicator` 登录态也渲染（剩余 N/1000 页）；Toolbar 登录区加入指示器

### 计划偏差记录

1. **「22 页」根因不可精确复现**（HEAD 代码口径为 11）：按用户语义定性为「识别/翻译分开计量」的观感问题 + v2.4「查看+翻译」文案口径——新方案以「单计量单位 + 解析永不扣费」结构性消除该类问题。
2. **DOCX 页数估算不裁剪**：DOCX 无真实页数，本地解析成本可忽略，翻译按估算批截断即达标；仅 PDF（MinerU 云端成本）做预裁剪。
3. **裁剪件缓存碎片化**：不同 cap 值产生不同哈希、各自缓存（游客 cap ∈ [1,5] 有界）；全量缓存命中时用全量 IR 不裁剪——优先复用已有解析成果。

### 验证

- Rust **327** 全绿（新增：24h 窗口滑动/批键窗口化重扣/主体隔离/用户 1000 页上限/两代旧表迁移 + MinerU 裁剪上传集成 + 单文档内部分处理熔断）
- 前端 **168** 全绿（删除 useQuotaGuard 4 项；新增登录态指示器/部分处理卡片语义）
- clippy `--all-targets` 0 警告；`tsc --noEmit` + `vite build` 通过
- 本地冒烟 **29/29**（隔离实例，含 v2.5 额度段）；实机验证：游客译 2 页 PDF 恰扣 2 页
  （remaining=3、reset_at 下发），扣减回拨 24h 后满额恢复；缓存秒回零扣费路径复验

## v2.5.1 补丁：MinerU 伪上/下标噪声清洗

- **日期**：2026-09-13 | **分支**：`main`

### 现象与根因

- 现象：`test/_local/14_EffTrans_Mamba.pdf`（arXiv Mamba 论文，Linux Libertine TeX 字体）经
  MinerU 解析后全篇大量字符渲染为下标。
- 复现（云端实测）：该文档 MinerU 输出含 **7545 个 `<sub>`**（正常文档 0–20 个），
  形如 `Se<sub>qu</sub>ence Modelin<sub>g</sub>`——VLM 把正文普通字母误判为上/下标；
  前端 v2.3 B1 的 `sup/sub` HTML 白名单如实渲染，症状放大。builtin 本地解析器不受影响
  （输出零下标字符，字号均匀）。
- 伴随损伤（同一 VLM 识别质量问题，本补丁不解决）：下行字母/连字字形丢失
  （"language"→"lan ua e"、"effective"→"efective"）。

### 方案（adapter 层防御，v1/v2 数据源同构）

| 层级 | 判定 | 动作 |
|------|------|------|
| 条目级 | 长内容（≥2 字符）标签 ≥3 个，或被包裹字符占比 ≥8% | 该条目去包裹（剥标签保内容） |
| 文档级 | 噪声条目占文本条目 ≥10% | 全文档去包裹（合法脚注标记一并放弃——病态文档保文本优先） |

- 合法标记（作者脚注 `<sup>1</sup>`、化学式 `<sub>2</sub>`）内容为单字符：长内容信号天然豁免；
  单字符标签包裹占比上限 ~7.7%，占比信号也天然豁免（双保险）。
- 清洗在 `adapt_v1`/`adapt_v2` 文本入口统一生效（含 title/ref_text/algorithm/题注 segs），
  IR 之后的翻译输入、导出、前端渲染全部受益；已缓存 IR 不回溯（`reparse_from_dir` 重放自动获益）。

### 验证

- 新增 4 测试：阈值单元（脚注/化学式豁免、三形态触发、去包裹、未闭合标签跳过）、
  v1 孤立噪声条目（其余条目脚注保留）、v1 文档级升级（边缘残留条目一并清理）、
  v2 segs 清洗（equation_inline 资源不受影响）
- 真实数据回归：6 份已缓存正常文档清洗后 0 变化；Mamba 全文 363 条目 → 残留标签 **0**
- Rust **331** 全绿；clippy `--all-targets` 0 警告
