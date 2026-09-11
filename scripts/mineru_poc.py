#!/usr/bin/env python3
"""MinerU 精准解析 API PoC（v2 T1）：批量签名上传 → 轮询 → 下载解压 → 结构分析。

用法：python3 scripts/mineru_poc.py [文件1.pdf 文件2.pdf ...]（默认 5 个代表性夹具）
Token 从 .env.local 的 MINERU_API_KEY 读取。
"""
import io
import json
import os
import sys
import time
import zipfile
from pathlib import Path

import requests

ROOT = Path(__file__).resolve().parent.parent
TOKEN = ""
for line in (ROOT / ".env.local").read_text().splitlines():
    if line.startswith("MINERU_API_KEY="):
        TOKEN = line.split("=", 1)[1].strip().strip('"').strip("'")
if not TOKEN:
    sys.exit("MINERU_API_KEY 未在 .env.local 中找到")

BASE = "https://mineru.net/api/v4"
OUT = Path("/tmp/mineru_poc")
HEADERS = {"Content-Type": "application/json", "Authorization": f"Bearer {TOKEN}"}

DEFAULT_FILES = [
    "simple.pdf",
    "with_formula.pdf",
    "with_table.pdf",
    "with_image.pdf",
    "two_column.pdf",
]


def main():
    names = sys.argv[1:] or DEFAULT_FILES
    paths = [ROOT / "test/fixtures/pdf" / n for n in names]
    for p in paths:
        if not p.exists():
            sys.exit(f"文件不存在: {p}")

    # 1. 获取批量上传签名 URL
    resp = requests.post(
        f"{BASE}/file-urls/batch",
        headers=HEADERS,
        json={"enable_formula": True, "enable_table": True,
              "model_version": "vlm", "files": [{"name": p.name, "is_ocr": False, "data_id": p.stem}
                                                for p in paths]},
        timeout=30,
    )
    body = resp.json()
    print("1) file-urls/batch →", resp.status_code, json.dumps(body, ensure_ascii=False)[:300])
    if body.get("code") != 0:
        sys.exit(f"获取上传 URL 失败: {body}")
    batch_id = body["data"]["batch_id"]
    # 实测：file_urls 是与请求 files 顺序对应的 URL 字符串数组（非对象数组）
    upload_urls = dict(zip((p.name for p in paths), body["data"]["file_urls"]))
    print(f"   batch_id={batch_id}")

    # 2. PUT 上传（实测：不得携带 Content-Type 头，否则 OSS 签名校验 403）
    for p in paths:
        r = requests.put(upload_urls[p.name], data=p.read_bytes(), timeout=120)
        print(f"2) PUT {p.name} → {r.status_code}")
        r.raise_for_status()

    # 3. 轮询批量结果
    deadline = time.time() + 900
    while time.time() < deadline:
        r = requests.get(f"{BASE}/extract-results/batch/{batch_id}", headers=HEADERS, timeout=30)
        body = r.json()
        if body.get("code") != 0:
            sys.exit(f"轮询失败: {body}")
        results = body["data"]["extract_result"]
        states = {x["file_name"]: x["state"] for x in results}
        prog = {x["file_name"]: x.get("extract_progress") for x in results}
        print(f"3) poll → {states} {prog}")
        if all(s in ("done", "failed") for s in states.values()):
            break
        time.sleep(5)
    else:
        sys.exit("轮询超时（15 分钟）")

    # 4. 下载解压
    OUT.mkdir(parents=True, exist_ok=True)
    for x in results:
        if x["state"] == "failed":
            print(f"4) {x['file_name']} FAILED: {x.get('err_msg')}")
            continue
        zr = requests.get(x["full_zip_url"], timeout=120)
        zr.raise_for_status()
        dest = OUT / x["file_name"].rsplit(".", 1)[0]
        dest.mkdir(exist_ok=True)
        with zipfile.ZipFile(io.BytesIO(zr.content)) as z:
            z.extractall(dest)
        print(f"4) {x['file_name']} → {dest}/")
        for f in sorted(dest.rglob("*")):
            print(f"   - {f.relative_to(dest)} ({f.stat().st_size}B)")


if __name__ == "__main__":
    main()
