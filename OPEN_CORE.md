# Open Core 边界 / Open Core Boundary

Diptych 采用 **Open Core** 模式：核心翻译阅读能力在本仓库以 Apache-2.0 完全开源，
面向规模化运营的增值组件以 **Diptych Pro** 名义单独商业授权。

## ✅ 本仓库（Community Edition，Apache-2.0）

| 能力 | 位置 |
| --- | --- |
| PDF 内置解析（版面分段 / 图像提取） | `crates/geminus-core/src/pdf/` |
| DOCX 解析（OOXML → 结构化文档） | `crates/geminus-core/src/parser/` |
| MinerU VLM 云端解析适配 | `crates/geminus-core/src/mineru/` |
| LLM 翻译流水线（分块 / 数学槽位保护 / 重建） | `crates/geminus-core/src/translate/` |
| 双栏对照阅读器（React + pdf.js） | `app/src/` |
| 段落级 Hover 联动 / 锚点同步 | `app/src/hooks/useHoverDrivenSync.ts` |
| 登录 / 匿名配额 / 用户历史 | `crates/geminus-server/src/` |
| 自托管部署工件（systemd / nginx / 冒烟） | `deploy/` |

## 🔒 Diptych Pro（闭源，单独授权）

以下组件**不在**本仓库，任何发行版（含社区 fork）均不包含：

- **TwinDoc 版式保留引擎** —— 译文 overlay 精确排版、CJK 字体子集嵌入、
  autofit 溢出自适应（社区版右栏为语义流式渲染，非逐坐标 overlay）
- **多租户 / 配额计费 / 账号运营组件** —— 组织管理、用量计费、SSO 等
- **云端缓存与术语表同步服务** —— 跨设备翻译缓存、团队术语库

## 授权与联系

- 社区版：Apache-2.0，允许商用与自营服务（保留声明即可），见 [LICENSE](LICENSE)。
- 商业授权 / OEM / 私有化交付：📧 licensing@wx-rdc.top
