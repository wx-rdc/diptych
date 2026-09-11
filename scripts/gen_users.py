#!/usr/bin/env python3
"""生成 geminus 预设用户（部署安全加固）。

产出两份文件：
  1. 哈希文件（默认 config/users.json）——服务端 GEMINUS_USERS_FILE 读取，
     格式与 crates/geminus-server/src/auth.rs 的 PBKDF2 校验一致：
     pbkdf2_sha256$<迭代>$<salt_b64>$<hash_b64>
  2. 明文文件（默认 .secrets/preset_users.txt）——运维留底，权限 600，不入 git。

用法：
  python3 scripts/gen_users.py --count 3 --prefix reader
  python3 scripts/gen_users.py --users alice,bob --append   # 追加指定用户名
"""
import argparse
import base64
import hashlib
import json
import os
import secrets
import stat
import sys

DEFAULT_ITERATIONS = 600_000


def hash_password(password: str, iterations: int) -> str:
    salt = secrets.token_bytes(16)
    derived = hashlib.pbkdf2_hmac("sha256", password.encode(), salt, iterations)
    return "pbkdf2_sha256${}${}${}".format(
        iterations, base64.b64encode(salt).decode(), base64.b64encode(derived).decode()
    )


def gen_password() -> str:
    # 16 字符 urlsafe（≈96 bit 熵）
    return secrets.token_urlsafe(12)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--count", type=int, default=3, help="自动生成用户数（reader1..N）")
    ap.add_argument("--prefix", default="reader", help="自动生成用户名前缀")
    ap.add_argument("--users", default="", help="逗号分隔的指定用户名（与 --count 叠加）")
    ap.add_argument("--out-hash", default="config/users.json", help="哈希文件路径")
    ap.add_argument("--out-plain", default=".secrets/preset_users.txt", help="明文留底路径")
    ap.add_argument("--iterations", type=int, default=DEFAULT_ITERATIONS)
    ap.add_argument("--password-len", type=int, default=12, help="密码 token_urlsafe 长度")
    ap.add_argument("--append", action="store_true", help="追加到已有哈希文件（默认覆盖）")
    args = ap.parse_args()

    usernames = []
    if args.count > 0:
        # 编号衔接已有用户，避免 --append 时重名
        existing_nums = 0
        if args.append and os.path.exists(args.out_hash):
            with open(args.out_hash, encoding="utf-8") as f:
                existing_nums = sum(
                    1
                    for u in json.load(f).get("users", [])
                    if u["username"].startswith(args.prefix)
                    and u["username"][len(args.prefix) :].isdigit()
                )
        usernames += [f"{args.prefix}{existing_nums + i + 1}" for i in range(args.count)]
    if args.users:
        usernames += [u.strip() for u in args.users.split(",") if u.strip()]
    if not usernames:
        ap.error("未指定任何用户（--count 或 --users）")

    entries = []
    if args.append and os.path.exists(args.out_hash):
        with open(args.out_hash, encoding="utf-8") as f:
            entries = json.load(f).get("users", [])
    known = {e["username"] for e in entries}

    plain_lines = []
    for name in usernames:
        if name in known:
            print(f"跳过已存在用户: {name}", file=sys.stderr)
            continue
        password = gen_password()[: args.password_len] if args.password_len < 16 else gen_password()
        entries.append({"username": name, "password_hash": hash_password(password, args.iterations)})
        plain_lines.append(f"{name}\t{password}")

    os.makedirs(os.path.dirname(args.out_hash) or ".", exist_ok=True)
    with open(args.out_hash, "w", encoding="utf-8") as f:
        json.dump({"users": entries}, f, indent=2, ensure_ascii=False)
        f.write("\n")
    os.chmod(args.out_hash, stat.S_IRUSR | stat.S_IWUSR)

    os.makedirs(os.path.dirname(args.out_plain) or ".", exist_ok=True)
    with open(args.out_plain, "a", encoding="utf-8") as f:
        if plain_lines:
            f.write("\n".join(plain_lines) + "\n")
    os.chmod(args.out_plain, stat.S_IRUSR | stat.S_IWUSR)

    print(f"已生成 {len(plain_lines)} 个用户：哈希 → {args.out_hash}，明文 → {args.out_plain}（权限 600，勿入 git）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
