# Third-Party Notices / 第三方组件归属

Diptych Community Edition 依赖以下开源组件，感谢原作者。
各组件以其自身许可证条款分发；本仓库整体以 Apache-2.0 分发。

## 前端运行时（`app/`）

| 组件 | 许可证 | 版权 |
| --- | --- | --- |
| [pdf.js](https://github.com/mozilla/pdf.js) (`pdfjs-dist`) | Apache-2.0 | © Mozilla Foundation |
| [KaTeX](https://github.com/KaTeX/KaTeX) | MIT | © Khan Academy |
| [React](https://github.com/facebook/react) / React DOM | MIT | © Meta Platforms, Inc. |
| [DOMPurify](https://github.com/cure53/DOMPurify) | Apache-2.0 / MPL-2.0 | © Cure53 |

前端构建链（仅开发期使用，不随产物分发）：Vite、TypeScript、Tailwind CSS、
Vitest、Testing Library 等，均为 MIT。

## 后端（Rust，`crates/`）

主要 crate（完整清单见 `Cargo.lock`，均以 MIT 和/或 Apache-2.0 双许可分发，
版权归各自作者）：

- axum, tokio, tower, tower-http, tokio-stream, futures —— 异步 Web 框架与运行时
- reqwest —— HTTP 客户端（rustls TLS）
- serde / serde_json —— 序列化
- quick-xml, zip —— OOXML（DOCX）解析
- lopdf —— PDF 解析
- rusqlite（bundled SQLite）—— 缓存 / 配额 / 历史存储
- sha2, base64, uuid, thiserror, tempfile, dotenvy

## 字体

- **Noto Sans SC** —— SIL Open Font License 1.1，© Google LLC。
  允许随文档嵌入；不得单独售卖字体文件本身。
