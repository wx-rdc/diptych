#!/usr/bin/env bash
# geminus 发布 SOP 脚本化（v2.3）：本地门禁（可选跳过）→ 打包 → 目标服务器铺开切链 →
# nginx 配置同步 → 重启 → 后端直连冒烟 → 公网资产/MIME 冒烟（--public-base=URL 时）。
# 服务器布局注意（2026-09-11 实测）：/opt/geminus/releases 归 geminus 用户，
# ssh 用户不可直写 → /tmp 中转 + sudo 移入；冒烟依赖凭据与 PDF 夹具（首次自动铺）。
#
# 用法：
#   deploy/publish.sh <tag>                              # 全流程（含本地门禁）
#   deploy/publish.sh <tag> --skip-tests                 # 门禁刚跑过时跳过
#   deploy/publish.sh <tag> --public-base=http://域名     # 附加公网 MIME/资产冒烟
#   deploy/publish.sh --rollback <tag>                   # 回滚到既有版本目录
set -euo pipefail
cd "$(dirname "$0")/.."

SERVER="${GEMINUS_SERVER:-user@your-server}"
REMOTE_ROOT="/opt/geminus"
SMOKE_USERS_REMOTE="$REMOTE_ROOT/config/preset_users.txt"

say() { echo "==> $*"; }

# ── 回滚（≤5 分钟）：current 指回旧版 + 重启 + 冒烟 ──
if [ "${1:-}" = "--rollback" ]; then
  TAG="${2:?用法: deploy/publish.sh --rollback <tag>}"
  say "回滚到 $TAG"
  ssh "$SERVER" "
    set -e
    [ -d $REMOTE_ROOT/releases/$TAG ] || { echo '版本不存在: $TAG'; ls $REMOTE_ROOT/releases; exit 1; }
    sudo ln -sfn $REMOTE_ROOT/releases/$TAG $REMOTE_ROOT/current
    sudo systemctl restart geminus-server
    sleep 2
    systemctl is-active geminus-server
  "
  say "回滚完成，冒烟校验"
  ssh "$SERVER" "BASE=http://127.0.0.1:39321 USERS_FILE=$SMOKE_USERS_REMOTE \
    UPLOAD_PDF=/tmp/geminus_smoke/simple.pdf LARGE_PDF=/tmp/geminus_smoke/large.pdf \
    bash $REMOTE_ROOT/current/smoke.sh" | tail -3
  exit 0
fi

TAG="${1:?用法: deploy/publish.sh <tag> [--skip-tests] [--public-base=URL]}"
SKIP_TESTS=0
PUBLIC_BASE=""
for arg in "$@"; do
  case "$arg" in
    --skip-tests) SKIP_TESTS=1 ;;
    --public-base=*) PUBLIC_BASE="${arg#*=}" ;;
  esac
done

# ── 1. 本地门禁（默认执行；--skip-tests 跳过）──
if [ "$SKIP_TESTS" -eq 0 ]; then
  say "本地门禁：cargo test/clippy + 前端 test/build"
  cargo test --workspace --quiet
  cargo clippy --workspace -- -D warnings
  (cd app && pnpm test >/dev/null 2>&1 && pnpm build >/dev/null 2>&1)
fi

# ── 2. 打包 + 传输 ──
deploy/package.sh "$TAG"
say "传输 $TAG → $SERVER"
scp -q "dist/geminus-release-$TAG.tar.gz" "$SERVER:/tmp/"

# ── 3. 铺开 + 切链 + nginx 同步 + 重启 ──
say "铺开 $TAG（/tmp 中转 + sudo，releases/ 归 geminus 用户）"
ssh "$SERVER" "
  set -e
  TAG=$TAG
  rm -rf /tmp/geminus-rel-\$TAG && mkdir -p /tmp/geminus-rel-\$TAG
  tar -xzf /tmp/geminus-release-\$TAG.tar.gz -C /tmp/geminus-rel-\$TAG
  sudo rm -rf $REMOTE_ROOT/releases/\$TAG
  sudo mv /tmp/geminus-rel-\$TAG/geminus $REMOTE_ROOT/releases/\$TAG
  sudo chown -R geminus:geminus $REMOTE_ROOT/releases/\$TAG
  sudo ln -sfn $REMOTE_ROOT/releases/\$TAG $REMOTE_ROOT/current
  # nginx location 片段随包同步（P0 MIME types 等），语法验证后才 reload
  sudo cp $REMOTE_ROOT/current/geminus-locations.conf /etc/nginx/snippets/geminus-locations.conf
  sudo nginx -t
  sudo systemctl reload nginx
  sudo systemctl restart geminus-server
  sleep 2
  echo \"service: \$(systemctl is-active geminus-server)\"
  echo \"version: \$(cat $REMOTE_ROOT/current/VERSION)\"
  # releases 保留最近 3 版，超出清最旧
  ls -dt $REMOTE_ROOT/releases/*/ | tail -n +4 | xargs -r sudo rm -rf
"

# ── 4. 冒烟夹具与凭据（幂等铺放；凭据属 ssh 用户 600）──
say "铺放冒烟夹具/凭据（幂等）"
scp -q test/fixtures/pdf/simple.pdf test/fixtures/pdf/large.pdf "$SERVER:/tmp/" || true
scp -q .secrets/preset_users.txt "$SERVER:/tmp/geminus_preset_users.txt"
ssh "$SERVER" "
  set -e
  mkdir -p /tmp/geminus_smoke
  mv -f /tmp/simple.pdf /tmp/large.pdf /tmp/geminus_smoke/ 2>/dev/null || true
  if [ ! -f $SMOKE_USERS_REMOTE ]; then
    sudo mv /tmp/geminus_preset_users.txt $SMOKE_USERS_REMOTE
    sudo chown \$(whoami):\$(whoami) $SMOKE_USERS_REMOTE
    sudo chmod 600 $SMOKE_USERS_REMOTE
  else
    rm -f /tmp/geminus_preset_users.txt
  fi
"

# ── 5. 冒烟：后端直连（全 7 节）──
say "冒烟（后端直连 127.0.0.1:39321）"
ssh "$SERVER" "BASE=http://127.0.0.1:39321 USERS_FILE=$SMOKE_USERS_REMOTE \
  UPLOAD_PDF=/tmp/geminus_smoke/simple.pdf LARGE_PDF=/tmp/geminus_smoke/large.pdf \
  bash $REMOTE_ROOT/current/smoke.sh" | tail -6

# ── 6. 冒烟：公网（--public-base 时；含资产 200 + .mjs MIME 断言）──
if [ -n "$PUBLIC_BASE" ]; then
  say "冒烟（公网 $PUBLIC_BASE，含 MIME 断言）"
  ssh "$SERVER" "BASE=$PUBLIC_BASE USERS_FILE=$SMOKE_USERS_REMOTE \
    UPLOAD_PDF=/tmp/geminus_smoke/simple.pdf LARGE_PDF=/tmp/geminus_smoke/large.pdf \
    PUBLIC_BASE=$PUBLIC_BASE WEB_ROOT=$REMOTE_ROOT/current/web \
    bash $REMOTE_ROOT/current/smoke.sh" | tail -8
fi

say "发布完成: $TAG（回滚: deploy/publish.sh --rollback <prev-tag>）"
