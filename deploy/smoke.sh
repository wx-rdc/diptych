#!/usr/bin/env bash
# 部署后冒烟（deploy_plan 阶段 4 验收的脚本化子集）：
# 健康检查 / 静态资源 / 登录 / 匿名限流（模拟 X-Real-IP）/ 上传解析 / SSE。
# 用法：
#   BASE=http://127.0.0.1:39321 USERS_FILE=.secrets/preset_users.txt deploy/smoke.sh
#   BASE=https://<域名> ...            # 经 nginx 的公网验收（不注入 X-Real-IP，走真实 IP）
# 说明：
#   - 直连后端时用 X-Real-IP 头模拟公网客户端（生产由 nginx 覆盖式注入）；
#   - 匿名限流段会消耗模拟 IP 各自的少量页数额度（SQLite 持久，不重置）。
set -euo pipefail

BASE="${BASE:-http://127.0.0.1:39321}"
USERS_FILE="${USERS_FILE:-.secrets/preset_users.txt}"
UPLOAD_PDF="${UPLOAD_PDF:-test/fixtures/pdf/simple.pdf}"
QUOTA_IP="${QUOTA_IP:-203.0.113.99}"
# 非白名单扩展名测试件（不依赖脚本自身路径，兼容发布包布局）
SMOKE_EVIL="$(mktemp /tmp/geminus_smoke_evil.XXXXXX.sh)"

pass=0; fail=0
ok()   { echo "✅ $1"; pass=$((pass+1)); }
bad()  { echo "❌ $1"; fail=$((fail+1)); }
check() { # desc expected actual
  if [ "$2" = "$3" ]; then ok "$1"; else bad "$1（期望 $2，实际 $3）"; fi
}

echo "== 1. 健康检查 =="
check "GET /api/health → 200" 200 "$(curl -s -o /dev/null -w '%{http_code}' "$BASE/api/health")"

echo "== 2. 静态资源（SPA 入口）=="
check "GET / → 200" 200 "$(curl -s -o /dev/null -w '%{http_code}' "$BASE/")"
curl -s "$BASE/" | grep -q '<div id="root">' && ok "index.html 含 SPA 挂载点" || bad "index.html 异常"

echo "== 3. 认证 =="
ME=$(curl -s "$BASE/api/auth/me")
echo "$ME" | grep -q '"authenticated":false' && ok "匿名 me → authenticated:false" || bad "匿名 me 异常: $ME"

FIRST_USER=$(head -1 "$USERS_FILE" | cut -f1)
FIRST_PASS=$(head -1 "$USERS_FILE" | cut -f2)
JAR=$(mktemp)
CODE=$(curl -s -o /tmp/geminus_smoke_login.json -w '%{http_code}' -c "$JAR" \
  -H 'content-type: application/json' \
  -d "{\"username\":\"$FIRST_USER\",\"password\":\"$FIRST_PASS\"}" \
  "$BASE/api/auth/login")
check "登录 $FIRST_USER → 200" 200 "$CODE"
ME=$(curl -s -b "$JAR" "$BASE/api/auth/me")
echo "$ME" | grep -q "\"username\":\"$FIRST_USER\"" && ok "登录后 me → $FIRST_USER" || bad "登录后 me 异常: $ME"

CODE=$(curl -s -o /dev/null -w '%{http_code}' -H 'content-type: application/json' \
  -H "X-Real-IP: 203.0.113.98" \
  -d '{"username":"nosuchuser","password":"wrong"}' "$BASE/api/auth/login")
check "错误口令 → 401" 401 "$CODE"

echo "== 4. 匿名限流（模拟 IP $QUOTA_IP，上限默认 5 页）=="
# 大文件（>5 页）匿名上传 → 429 预检拦截（multipart 路径 = 公网真实语义；
# JSON file_path 分支默认被服务器禁用，不用于限流验证）
LARGE_PDF="${LARGE_PDF:-test/fixtures/pdf/large.pdf}"
CODE=$(curl -s -o /dev/null -w '%{http_code}' -H "X-Real-IP: $QUOTA_IP" \
  -F "file=@$LARGE_PDF" -F "parser=builtin" "$BASE/api/parse")
check "匿名上传 ${LARGE_PDF##*/}（>5 页）→ 429" 429 "$CODE"

# 登录用户同文件 → 200（不受限）
CODE=$(curl -s -o /dev/null -w '%{http_code}' -b "$JAR" -H "X-Real-IP: $QUOTA_IP" \
  -F "file=@$LARGE_PDF" -F "parser=builtin" "$BASE/api/parse")
check "登录用户上传同文件 → 200（不受限）" 200 "$CODE"

# JSON file_path 分支门控（生产默认 403；本地 ALLOW_LOCAL=1 时跳过）
CODE=$(curl -s -o /dev/null -w '%{http_code}' -H 'content-type: application/json' \
  -d '{"file_path":"/etc/passwd","parser":"builtin"}' "$BASE/api/parse")
if [ "$CODE" = "403" ]; then ok "JSON 本地路径分支 → 403（生产禁用）"; else ok "JSON 本地路径分支 → $CODE（本地开发放行）"; fi

echo "== 5. multipart 上传 =="
CODE=$(curl -s -o /dev/null -w '%{http_code}' -b "$JAR" \
  -F "file=@$UPLOAD_PDF" -F "parser=builtin" "$BASE/api/parse")
check "登录用户上传 simple.pdf → 200" 200 "$CODE"

CODE=$(curl -s -o /dev/null -w '%{http_code}' \
  -F "file=@$SMOKE_EVIL;filename=evil.sh" "$BASE/api/parse")
check "上传非白名单扩展名 → 400" 400 "$CODE"
rm -f "$SMOKE_EVIL"

echo "== 6. SSE（翻译进度流不被缓冲）=="
DOC_ID=$(curl -s -b "$JAR" -F "file=@$UPLOAD_PDF" -F "parser=builtin" "$BASE/api/parse" | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
curl -s -N --max-time 120 -b "$JAR" -H 'content-type: application/json' \
  -d "{\"doc_id\":\"$DOC_ID\",\"source_lang\":\"en\",\"target_lang\":\"zh\"}" \
  "$BASE/api/translate" > /tmp/geminus_smoke_sse.txt || true
SSE_HEAD=$(head -c 200 /tmp/geminus_smoke_sse.txt)
grep -q "event:" /tmp/geminus_smoke_sse.txt && ok "翻译 SSE 有事件流出" || bad "SSE 无事件: $SSE_HEAD"

echo "== 6.5 权限收口（服务器开启 GEMINUS_REQUIRE_LOGIN=1 时自动生效，v2.4）=="
if curl -s "$BASE/api/auth/me" | grep -q '"require_login":true'; then
  RIP="203.0.113.$((RANDOM % 200 + 2))"   # 每次随机 IP，避免匿名配额累积误报
  BODY=$(curl -s -H "X-Real-IP: $RIP" -F "file=@$UPLOAD_PDF" -F "parser=builtin" "$BASE/api/parse")
  DOC=$(echo "$BODY" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("id",""))' 2>/dev/null || true)
  [ -n "$DOC" ] && ok "匿名上传 → 200 外壳（含 doc_id）" || bad "匿名上传异常: $BODY"
  echo "$BODY" | grep -q '"blocks"' && bad "匿名上传泄露 IR（blocks）" || ok "匿名上传不返回 IR"
  CODE=$(curl -s -o /dev/null -w '%{http_code}' "$BASE/api/documents/$DOC")
  check "匿名 GET /api/documents → 401" 401 "$CODE"
  CODE=$(curl -s -o /dev/null -w '%{http_code}' -H 'content-type: application/json' \
    -d "{\"doc_id\":\"$DOC\",\"source_lang\":\"en\",\"target_lang\":\"zh\"}" "$BASE/api/translate")
  check "匿名 POST /api/translate → 401" 401 "$CODE"
  CODE=$(curl -s -o /dev/null -w '%{http_code}' -b "$JAR" "$BASE/api/history")
  check "登录 GET /api/history → 200" 200 "$CODE"
else
  ok "跳过权限收口段（require_login=0，本地/回归模式）"
fi

echo "== 7. 静态资产完整性（v2.3 P0 防回归）=="
# 7.1 index.html 引用的全部构建产物可访问
ASSET_URLS=$(curl -s "$BASE/" | grep -oE '(src|href)="/assets/[^"]+"' | grep -oE '/assets/[^"]+' | sort -u)
ASSET_N=$(echo "$ASSET_URLS" | grep -c assets || true)
if [ "${ASSET_N:-0}" -ge 1 ]; then ok "index.html 引用 ≥1 个 hash 资产"; else bad "index.html 未引用任何资产"; fi
for u in $ASSET_URLS; do
  code=$(curl -s -o /dev/null -w '%{http_code}' "$BASE$u")
  check "GET ${u#/assets/} → 200" 200 "$code"
done

# 7.2 部署目录内全部 .mjs 经公网入口（nginx）返回 200 + JS MIME
#     （worker 已 inline 时通常无 .mjs，此段防未来外部 worker/模块资产回归）
WEB_ROOT="${WEB_ROOT:-/opt/geminus/current/web}"
if [ -d "$WEB_ROOT/assets" ] && [ -n "${PUBLIC_BASE:-}" ]; then
  MJS_LIST=$(ls "$WEB_ROOT/assets/"*.mjs 2>/dev/null || true)
  if [ -n "$MJS_LIST" ]; then
    for f in $MJS_LIST; do
      name="$(basename "$f")"
      head=$(curl -sI "$PUBLIC_BASE/assets/$name")
      code=$(echo "$head" | head -1 | grep -oE '[0-9]{3}')
      ctype=$(echo "$head" | grep -i '^content-type:' | tr -d '\r' | awk '{print tolower($2)}')
      check "HEAD $name → 200" 200 "$code"
      case "$ctype" in
        text/javascript|application/javascript|application/ecmascript) ok "$name MIME=$ctype" ;;
        *) bad "$name MIME 非 JS 类型: $ctype" ;;
      esac
    done
  else ok "无外部 .mjs 资产（worker 已 inline 打包，P0 根治态）"; fi
else
  ok "跳过公网 MIME 校验（PUBLIC_BASE 未设或 WEB_ROOT 不存在）"
fi

echo
echo "结果：$pass 通过 / $fail 失败"
[ "$fail" -eq 0 ]
