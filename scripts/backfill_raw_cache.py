#!/usr/bin/env python3
"""回填 MinerU raw 缓存：把 /tmp 中残留的解压 chunk 目录按 IR 特征匹配到
data/mineru_cache/<file_hash>/，使 schema bump 后可离线重放（零 API）。
匹配特征：mineru_parses.ir_json 的 title / 首个段落文本 vs chunk 目录 content_list 首条目文本。"""
import json, glob, hashlib, shutil, sqlite3, sys, os

DB = "data/translate_cache.db"
CACHE_ROOT = "data/mineru_cache"
TMP_GLOB = "/tmp/geminus_mineru_*/"


def first_text(chunk_dir):
    f = next(iter(sorted(glob.glob(os.path.join(chunk_dir, "*content_list.json")))), None)
    if not f:
        return None
    try:
        d = json.load(open(f))
        return d[0].get("text", "")[:80] if d else None
    except Exception:
        return None


def main():
    conn = sqlite3.connect(DB)
    rows = conn.execute("SELECT file_hash, ir_json FROM mineru_parses").fetchall()
    # 收集候选 chunk 目录组（含 chunk_NN 完整解压结果的临时目录）
    candidates = []
    for d in sorted(glob.glob(TMP_GLOB)):
        chunks = sorted(glob.glob(os.path.join(d, "chunk_*")))
        chunks = [c for c in chunks if glob.glob(os.path.join(c, "*content_list.json"))]
        if not chunks:
            continue
        candidates.append(chunks)
    for file_hash, ir_json in rows:
        ir = json.loads(ir_json)
        title = ir.get("title", "")
        first_ir = next((b.get("content", [{}])[0].get("text", "")
                         for b in ir.get("blocks", [])
                         if b.get("type") == "paragraph" and b.get("content")), "")
        dest = os.path.join(CACHE_ROOT, file_hash)
        if os.path.isdir(dest):
            print(f"[skip] {title}: 已有 raw 缓存")
            continue
        for chunks in candidates:
            ft = first_text(chunks[0]) or ""
            hit = (title and title in os.path.basename(chunks[0])) or \
                  (first_ir[:40] and first_ir[:40] in ft)
            if not hit:
                continue
            os.makedirs(dest, exist_ok=True)
            meta = {"chunks": [{"dir": os.path.basename(c)} for c in chunks]}
            json.dump(meta, open(os.path.join(dest, "raw_meta.json"), "w"))
            for c in chunks:
                shutil.copytree(c, os.path.join(dest, os.path.basename(c)),
                                dirs_exist_ok=True)
            print(f"[ok] {title} <- {chunks[0]} ({len(chunks)} 片)")
            break
        else:
            print(f"[miss] {title}: /tmp 无匹配 chunk")
    # 附加核对：test/ 下 PDF 的内容哈希与缓存目录的对应关系
    for f in glob.glob("test/*.pdf"):
        h = hashlib.sha256(open(f, "rb").read()).hexdigest()
        if os.path.isdir(os.path.join(CACHE_ROOT, h)):
            print(f"      {f} -> {h[:12]}")


if __name__ == "__main__":
    main()
