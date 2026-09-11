#!/usr/bin/env bash
# uploads 临时文件清理（cron 每小时调用；deploy_plan 阶段 3.5）
# 删除超过 TTL（默认 24 小时）的上传文件——解析完成后后端仅按 doc_id 内存引用，
# 原始 PDF 取自 uploads 路径，故保留一个会话量级的 TTL。
set -euo pipefail
UPLOADS_DIR="${1:-/opt/geminus/uploads}"
TTL_HOURS="${TTL_HOURS:-24}"
find "$UPLOADS_DIR" -type f -mmin +"$((TTL_HOURS * 60))" -delete
