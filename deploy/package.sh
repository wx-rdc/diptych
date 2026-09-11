#!/usr/bin/env bash
# 打包发布工件：本地 release 构建（与目标服务器同架构 linux/x86_64）→ dist/geminus-release-<tag>.tar.gz
# 用法：deploy/package.sh [tag]（默认取 git describe --tags --abbrev=0）
set -euo pipefail
cd "$(dirname "$0")/.."

TAG="${1:-$(git describe --tags --abbrev=0 2>/dev/null || echo untagged)}"
STAGE="$(mktemp -d)/geminus"
OUT="dist/geminus-release-${TAG}.tar.gz"

echo "==> 构建 release 二进制（本地，目标机保持最小运行时）"
cargo build --release -p geminus-server

echo "==> 构建前端"
(cd app && pnpm install --prefer-offline >/dev/null && pnpm build)

echo "==> 组装发布包"
mkdir -p "$STAGE" "$(dirname "$OUT")"
cp target/release/geminus-server "$STAGE/"
cp -r app/dist "$STAGE/web"
cp deploy/geminus-server.service "$STAGE/"
cp deploy/nginx-geminus.conf.template deploy/geminus-locations.conf "$STAGE/"
cp deploy/smoke.sh deploy/cleanup_uploads.sh "$STAGE/"
cp .env.example "$STAGE/geminus.env.example"
printf '%s\n' "$TAG" > "$STAGE/VERSION"

tar -czf "$OUT" -C "$(dirname "$STAGE")" "$(basename "$STAGE")"
echo "==> 完成: $OUT"
# 列前 5 个文件确认包内容。注意用 sed（全量消费 stdin）而非 head：
# head 读满即退出会让 tar 触发 SIGPIPE，在 set -o pipefail 下炸掉整个发布（v2.4 实测 141）
tar -tzf "$OUT" | sed -n '1,5p'
