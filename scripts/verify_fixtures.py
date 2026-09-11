#!/usr/bin/env python3
"""校验 PDF 夹具并输出预期结果 JSON（v1_plan.md T15.3）。

对每个夹具用 PyMuPDF 独立校验：页数、图片数、必须包含的关键词、
双栏阅读顺序标记的坐标关系。输出 test/fixtures/pdf/expected.json，
Rust 集成测试读取该文件做跨实现断言（两个独立 PDF 实现提取一致）。
"""
import json
import os

import fitz

OUT_DIR = os.path.join(os.path.dirname(__file__), "..", "test", "fixtures", "pdf")

SPEC = {
    "simple.pdf": {"pages": 1, "images": 0, "must_contain": ["Machine learning", "Natural language", "Computer vision", "Reinforcement learning", "Transfer learning"]},
    "headings.pdf": {"pages": 1, "images": 0, "must_contain": ["Chapter One", "Helvetica", "Experimental Setup", "numbered item"]},
    "with_image.pdf": {"pages": 2, "images": 2, "must_contain": ["Figure experiments", "red stimulus", "control condition"]},
    "with_table.pdf": {"pages": 1, "images": 0, "must_contain": ["TABLEMARKER", "Method", "Accuracy", "Baseline", "Ours"]},
    "with_formula.pdf": {"pages": 1, "images": 0, "must_contain": ["FORMULAMARKER", "special relativity"]},
    "two_column.pdf": {"pages": 2, "images": 0, "must_contain": ["LEFTCOL-MARKER", "RIGHTCOL-MARKER", "Abstract"]},
    "mixed_layout.pdf": {"pages": 3, "images": 1, "must_contain": ["MIXEDTITLE-MARKER", "MIXEDLEFT-MARKER", "MIXEDRIGHT-MARKER"]},
    "empty.pdf": {"pages": 1, "images": 0, "must_contain": []},
    "single_image.pdf": {"pages": 1, "images": 1, "must_contain": []},
    "large.pdf": {"pages": 100, "images": 0, "must_contain": ["LARGEPAGE 1", "LARGEPAGE 50", "LARGEPAGE 100"]},
}


def verify() -> dict:
    result = {}
    for name, spec in SPEC.items():
        path = os.path.join(OUT_DIR, name)
        if not os.path.exists(path):
            raise SystemExit(f"缺少夹具: {path}")
        doc = fitz.open(path)
        try:
            pages = doc.page_count
            images = sum(len(doc[i].get_images(full=True)) for i in range(pages))
            text = "\n".join(doc[i].get_text() for i in range(pages))
            normalized = " ".join(text.split())
            missing = [kw for kw in spec["must_contain"] if kw not in normalized]
            assert pages == spec["pages"], f"{name}: 页数 {pages} != {spec['pages']}"
            assert images == spec["images"], f"{name}: 图片 {images} != {spec['images']}"
            assert not missing, f"{name}: 缺少关键词 {missing}"

            # 附加坐标事实（供 Rust 侧阅读顺序断言）
            facts = {}
            if name == "two_column.pdf":
                left = locate(doc, "LEFTCOL-MARKER")
                right = locate(doc, "RIGHTCOL-MARKER")
                assert left and right and left["page"] == right["page"] == 0
                assert left["x0"] < right["x0"], "左栏标记应在右栏左侧"
                facts = {"leftcol_page": left["page"], "rightcol_page": right["page"],
                         "leftcol_x": left["x0"], "rightcol_x": right["x0"]}
            if name == "mixed_layout.pdf":
                title = locate(doc, "MIXEDTITLE-MARKER")
                left = locate(doc, "MIXEDLEFT-MARKER")
                facts = {"title_page": title["page"], "title_x0": title["x0"],
                         "body_first_page": left["page"], "leftcol_x": left["x0"]}

            result[name] = {"pages": pages, "images": images, "must_contain": spec["must_contain"], **facts}
            print(f"✓ {name}: {pages} 页, {images} 图")
        finally:
            doc.close()
    return result


def locate(doc: "fitz.Document", needle: str) -> dict | None:
    """返回关键词首次出现的页码与 x0（PyMuPDF 顶点在左上）。"""
    for i in range(doc.page_count):
        rects = doc[i].search_for(needle)
        if rects:
            r = rects[0]
            return {"page": i, "x0": r.x0, "y0": r.y0}
    return None


def main() -> None:
    result = verify()
    out = os.path.join(OUT_DIR, "expected.json")
    with open(out, "w", encoding="utf-8") as f:
        json.dump(result, f, ensure_ascii=False, indent=2)
    print(f"OK: {out}")


if __name__ == "__main__":
    main()
