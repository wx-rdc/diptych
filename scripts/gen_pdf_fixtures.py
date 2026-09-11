#!/usr/bin/env python3
"""生成 geminus PDF 测试夹具（docs/v1_plan.md 任务 T15）。

输出 test/fixtures/pdf/ 下 10 个夹具（见 v1_plan.md §四）。
字体策略：
  - Vera（内嵌 TTF → /Type0 Identity-H + ToUnicode CMap）为主体，贴近真实论文；
  - headings.pdf 中一段用 Helvetica（Base-14，无 ToUnicode）验证 WinAnsi 回退解码。
坐标策略：正文 11pt/行距 15，段间距 15+14=29 ≈ 1.93×行距 > 1.4 段落阈值；
标题 24/15/13pt 对应 H1/H2/H3（阈值 1.6/1.3/1.15 × 正文 11pt）。

用法：
  python scripts/gen_pdf_fixtures.py                    # 生成全部夹具
  python scripts/gen_pdf_fixtures.py --pages 500 --out /tmp/huge.pdf
"""
import argparse
import io
import os

import reportlab
from PIL import Image as PilImage
from reportlab.lib.pagesizes import letter
from reportlab.lib.styles import ParagraphStyle
from reportlab.lib.units import inch
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.pdfgen import canvas as pdfcanvas
from reportlab.platypus import (
    Image,
    PageBreak,
    Paragraph,
    SimpleDocTemplate,
    Spacer,
    Table,
    TableStyle,
)

OUT_DIR = os.path.join(os.path.dirname(__file__), "..", "test", "fixtures", "pdf")
_FONTS = os.path.join(os.path.dirname(reportlab.__file__), "fonts")
pdfmetrics.registerFont(TTFont("Vera", os.path.join(_FONTS, "Vera.ttf")))
pdfmetrics.registerFont(TTFont("Vera-Bold", os.path.join(_FONTS, "VeraBd.ttf")))

PAGE_W, PAGE_H = letter  # 612 x 792 pt
MARGIN = 54
BODY_SIZE = 11
LEADING = 15
PARA_GAP = 14  # 段后间距：段落间 29pt / 行距 15pt ≈ 1.93 > 1.4 阈值

BODY = ParagraphStyle("body", fontName="Vera", fontSize=BODY_SIZE, leading=LEADING, spaceAfter=PARA_GAP)
H1 = ParagraphStyle("h1", fontName="Vera-Bold", fontSize=24, leading=28, spaceAfter=10)
H2 = ParagraphStyle("h2", fontName="Vera-Bold", fontSize=15, leading=19, spaceAfter=8)
H3 = ParagraphStyle("h3", fontName="Vera-Bold", fontSize=13, leading=17, spaceAfter=6)
HELV = ParagraphStyle("helv", fontName="Helvetica", fontSize=BODY_SIZE, leading=LEADING, spaceAfter=PARA_GAP)

SIMPLE_PARAS = [
    (
        "Machine learning is a subset of artificial intelligence that focuses on "
        "algorithms learning patterns from data. Modern systems rely on large "
        "corpora and gradient based optimization methods."
    ),
    (
        "Natural language processing studies the interaction between computers "
        "and human language. It spans syntax, semantics, and pragmatic inference "
        "across many application domains."
    ),
    (
        "Computer vision enables machines to interpret visual scenes from digital "
        "images. Convolutional networks drove remarkable progress on recognition "
        "benchmarks during the last decade."
    ),
    (
        "Reinforcement learning trains agents to make sequences of decisions by "
        "maximizing cumulative reward signals. Applications include game playing, "
        "robotics, and recommendation systems."
    ),
    (
        "Transfer learning reuses knowledge from a source task to improve learning "
        "on a related target task. Pretrained language models are a prominent "
        "example of this paradigm in practice."
    ),
]

HEADING_FLOWABLES = [
    ("Chapter One: Introduction", H1),
    (
        "This chapter introduces the research background and motivation. "
        "We review prior work and position our contributions clearly.",
        BODY,
    ),
    ("Background and Related Work", H2),
    (
        "Early approaches relied on hand crafted features and shallow models. "
        "Deep architectures replaced these pipelines with end to end training.",
        BODY,
    ),
    (
        "This paragraph is set in Helvetica, a base fourteen font without an "
        "embedded ToUnicode map, to exercise the fallback decoding path.",
        HELV,
    ),
    ("Experimental Setup", H3),
    (
        "We follow standard evaluation protocols and report mean scores over "
        "three random seeds for every experiment in the paper.",
        BODY,
    ),
    ("Our main contributions are summarized below:", BODY),
    ("• Fast algorithm with linear time complexity in practice", BODY),
    ("• Novel loss function that stabilizes early training", BODY),
    ("• Comprehensive ablation across four benchmark datasets", BODY),
    ("1. First numbered item about data preprocessing", BODY),
    ("2. Second numbered item about model architecture", BODY),
    (
        "A large vertical gap follows this paragraph to simulate an explicit "
        "empty block separator in the source document.",
        BODY,
    ),
]


def make_jpeg(width: int, height: int, rgb: tuple) -> bytes:
    """PIL 生成带竖条纹的 JPEG（DCTDecode，真实图片编码路径）。"""
    img = PilImage.new("RGB", (width, height), rgb)
    px = img.load()
    accent = (255 - rgb[0], 255 - rgb[1], 255 - rgb[2])
    for i in range(width):
        if i % 7 == 0:
            for j in range(height):
                if j % 3 == 0:
                    px[i, j] = accent
    buf = io.BytesIO()
    img.save(buf, format="JPEG", quality=90)
    return buf.getvalue()


def jpeg_path(name: str, width: int, height: int, rgb: tuple) -> str:
    path = os.path.join(OUT_DIR, name)
    with open(path, "wb") as f:
        f.write(make_jpeg(width, height, rgb))
    return path


def build_simple(path: str) -> None:
    build_single_column(path, [Paragraph(t, BODY) for t in SIMPLE_PARAS])


def build_headings(path: str) -> None:
    story = [Paragraph(text, style) for text, style in HEADING_FLOWABLES]
    story.append(Spacer(1, 56))  # 大间距 → 空段落
    story.append(
        Paragraph(
            "After the deliberate gap, the document continues with closing "
            "remarks and acknowledgements for the reviewers.",
            BODY,
        )
    )
    build_single_column(path, story)


def build_with_image(path: str) -> None:
    img1 = jpeg_path("img_red.jpg", 160, 100, (200, 60, 60))
    img2 = jpeg_path("img_blue.jpg", 120, 120, (60, 90, 200))
    story = [
        Paragraph(
            "Figure experiments start with a description of the visual dataset "
            "and the acquisition protocol used to collect all samples.",
            BODY,
        ),
        Image(img1, width=2.2 * inch, height=1.375 * inch),
        Paragraph(
            "The first figure above shows the red stimulus distribution over "
            "the calibration trials of the main experiment.",
            BODY,
        ),
        PageBreak(),
        Paragraph(
            "On this second page a differently colored figure is presented "
            "before the closing analysis paragraphs of the section.",
            BODY,
        ),
        Image(img2, width=1.6 * inch, height=1.6 * inch),
        Paragraph(
            "The blue square figure presents the control condition results "
            "that complete the analysis section.",
            BODY,
        ),
    ]
    build_single_column(path, story)


def build_with_table(path: str) -> None:
    data = [
        ["Method", "Accuracy", "F1"],
        ["Baseline", "78.4", "77.1"],
        ["Ours", "84.2", "83.6"],
    ]
    table = Table(data, colWidths=[1.6 * inch] * 3)
    table.setStyle(
        TableStyle(
            [
                ("GRID", (0, 0), (-1, -1), 0.6, (0, 0, 0)),
                ("FONTNAME", (0, 0), (-1, 0), "Vera-Bold"),
                ("FONTNAME", (0, 1), (-1, -1), "Vera"),
                ("FONTSIZE", (0, 0), (-1, -1), 10),
            ]
        )
    )
    story = [
        Paragraph(
            "Table one summarizes the quantitative comparison between the "
            "baseline method and our proposed approach on the test split.",
            BODY,
        ),
        table,
        Paragraph(
            "Numbers in the TABLEMARKER table are averaged over three runs "
            "with different random seeds for statistical reliability.",
            BODY,
        ),
    ]
    build_single_column(path, story)


def build_with_formula(path: str) -> None:
    # reportlab 无数学模式：公式以文本绘制（MVP 明确不做公式识别，仅验证文本流不崩溃）
    formula_style = ParagraphStyle(
        "formula", fontName="Vera", fontSize=14, leading=18, spaceAfter=PARA_GAP, leftIndent=1.2 * inch
    )
    story = [
        Paragraph(
            "The energy momentum relation is perhaps the most famous equation "
            "in special relativity theory.",
            BODY,
        ),
        Paragraph("FORMULAMARKER E = mc<super>2</super>", formula_style),
        Paragraph(
            "A ratio of sums appears frequently in normalization layers of "
            "modern neural architectures.",
            BODY,
        ),
        Paragraph("FORMULAMARKER (a + b) / (c + d)", formula_style),
        Paragraph(
            "Both expressions above are typeset as ordinary text runs inside "
            "the portable document file.",
            BODY,
        ),
    ]
    build_single_column(path, story)


def draw_wrapped(c, text: str, x: float, y_top: float, width: float,
                 font: str = "Vera", size: float = BODY_SIZE,
                 leading: float = LEADING) -> float:
    """按宽度手动断行绘制段落（顶左原点坐标系），返回消耗的高度（含段距）。"""
    c.setFont(font, size)
    words = text.split()
    lines, cur = [], ""
    for w in words:
        trial = f"{cur} {w}".strip()
        if pdfmetrics.stringWidth(trial, font, size) <= width:
            cur = trial
        else:
            lines.append(cur)
            cur = w
    if cur:
        lines.append(cur)
    y = y_top - size
    for line in lines:
        c.drawString(x, y, line)
        y -= leading
    return len(lines) * leading + PARA_GAP


def two_col_geometry() -> tuple[float, float]:
    """返回 (左栏 x, 右栏 x)。"""
    gutter = 24
    col_w = (PAGE_W - 2 * MARGIN - gutter) / 2
    return MARGIN, MARGIN + col_w + gutter


TWO_COL_BODY = [
    # 左栏（页 1）
    (
        "Abstract. We study few shot classification where a learner must "
        "recognize novel categories from a handful of labelled examples. "
        "Our approach builds a metric space using prototypical "
        "representations learned end to end."
    ),
    (
        " LEFTCOL-MARKER Few shot learning remains challenging because novel "
        "categories contain very few training samples. Metric based methods "
        "classify query points by distance to class prototypes in an "
        "embedding space computed by a shared nonlinear encoder network."
    ),
    (
        "Prototypical networks learn a metric space in which classification "
        "is performed by computing distances to prototype representations "
        "of each class, enabling rapid generalization to new classes not "
        "present during training."
    ),
    (
        "The matching network framework attends over labelled samples with "
        "an attention kernel over the embeddings of the support set, while "
        "prototypical networks use class level mean representations that "
        "reduce variance considerably."
    ),
    (
        "We evaluate on the standard Omniglot and miniImageNet benchmarks "
        "following the episode construction protocol of prior work, with "
        "five way and one or five shot evaluation tasks and fifteen query "
        "points per class."
    ),
    (
        "Results show consistent improvements over baseline methods across "
        "all shot counts and evaluation episodes, confirming that simple "
        "prototype averaging is a strong and data efficient inductive "
        "bias for few shot recognition problems."
    ),
    (
        "Training uses the Adam optimizer with a cosine learning rate "
        "schedule over twenty thousand meta training steps sampled from "
        "the union of base training categories in every single episode."
    ),
    (
        "Data augmentation includes random crops, color jitter, and "
        "horizontal flips applied only to the support set during meta "
        "training so the query distribution stays untouched for honest "
        "evaluation of generalization."
    ),
    # 右栏（页 1）
    (
        " RIGHTCOL-MARKER Hyperparameters were selected on a held out "
        "validation split of training classes rather than on the novel "
        "test categories, keeping the protocol free of test set "
        "information leakage throughout all experiments."
    ),
    (
        "The embedding dimension is set to six hundred forty and the "
        "convolutional backbone is initialized from pretrained weights "
        "then fine tuned jointly with the metric objective during the "
        "meta training procedure."
    ),
    (
        "Episode construction samples five ways with one or five shots "
        "and fifteen query points per class, matching the protocol used "
        "by prior work so comparisons remain directly reproducible by "
        "independent laboratories."
    ),
    (
        "Statistical significance is assessed with bootstrap confidence "
        "intervals over one thousand resamples of the evaluation "
        "episodes, reporting the median and ninety five percent bounds "
        "for every table entry."
    ),
    (
        "Reproducibility statements list all random seeds, framework "
        "versions, and hardware configurations used for every table in "
        "the paper so others can verify the reported numbers exactly."
    ),
    (
        "Error analysis groups mistakes by visual similarity tiers and "
        "shows most confusions occur within fine grained families such "
        "as alphabet scripts or bird species subgroups of the data."
    ),
    # 页 2 左栏
    (
        " LEFTCOL2-MARKER Visualization of the learned spaces uses "
        "stochastic neighbor embedding projections with perplexity "
        "thirty and reveals coherent clusters corresponding to the "
        "novel categories held out from the training split."
    ),
    (
        "Sensitivity analysis varies the number of training episodes, "
        "the embedding dimensionality, and the distance metric among "
        "euclidean, cosine, and learned mahalanobis variants of the "
        "prototypical classification rule."
    ),
    (
        "Limitations include the modest scale of the benchmarks and the "
        "assumption that support and query distributions are drawn from "
        "the same underlying generative process during evaluation."
    ),
    (
        "Future work will explore hierarchical prototypes that capture "
        "part whole relationships among categories and temporal "
        "extensions of the metric space for streaming recognition."
    ),
    # 页 2 右栏
    (
        " RIGHTCOL2-MARKER Ethics considerations address potential dual "
        "use of classification technology and the composition of "
        "evaluation cohorts with respect to demographic representation "
        "criteria required by modern review guidelines."
    ),
    (
        "Acknowledgements. The authors thank the anonymous reviewers "
        "for constructive feedback and colleagues for discussions that "
        "shaped the final presentation of the experimental sections."
    ),
    (
        "In summary, prototypical networks provide a simple, efficient, "
        "and interpretable approach to few shot learning that continues "
        "to serve as a standard baseline in the research literature."
    ),
]


def build_two_column(path: str) -> None:
    """canvas 双栏排版：每栏 10 段，跨 2 页。"""
    left_x, right_x = two_col_geometry()
    gutter = 24
    col_w = (PAGE_W - 2 * MARGIN - gutter) / 2
    top = PAGE_H - MARGIN

    c = pdfcanvas.Canvas(path, pagesize=letter)
    col_x, y = left_x, top
    for para in TWO_COL_BODY:
        if y < MARGIN + LEADING * 5:  # 栏满：右栏或换页
            if col_x == left_x:
                col_x, y = right_x, top
            else:
                c.showPage()
                col_x, y = left_x, top
        y -= draw_wrapped(c, para, col_x, y, col_w)
    c.showPage()
    c.save()


MIXED_BODY = [
    # 页 2 左栏
    (
        " MIXEDLEFT-MARKER The body of this document is set in two "
        "columns while the title spans the full width of the page "
        "above them. This arrangement is typical of academic "
        "conference proceedings typeset with standard style files."
    ),
    (
        "Extended discussion of the rendering pipeline covers font "
        "subsetting, stream compression, and the coordinate "
        "transforms that place every glyph precisely on the page."
    ),
    (
        "Column balancing on the final page is achieved by measuring "
        "remaining vertical space and redistributing trailing "
        "flowables so both columns end within one baseline."
    ),
    (
        "Widow and orphan control prevents single lines of a "
        "paragraph from appearing alone at the top or bottom of a "
        "column, improving readability of dense scientific text."
    ),
    (
        "The typesetter supports floating figures and captions "
        "although only in column figures are exercised by this "
        "fixture document for layout regression purposes."
    ),
    (
        "Baseline comparisons include nearest neighbour classifiers "
        "in raw pixel space, linear probes on frozen features, and "
        "per episode finetuning of all layers for reference."
    ),
    (
        "Sensitivity analysis varies the number of episodes, the "
        "embedding dimensionality, and the distance metric among "
        "euclidean, cosine, and learned mahalanobis variants."
    ),
    (
        "Error analysis groups mistakes by visual similarity tiers "
        "and shows most confusions occur within fine grained "
        "families such as alphabet scripts or bird species."
    ),
    # 页 2 右栏（含图）
    (
        " MIXEDRIGHT-MARKER Figures may float within a column. The "
        "green image below illustrates an in column figure with its "
        "caption placed immediately underneath the artwork in this "
        "right column of the second page of the document."
    ),
    # （图占位由调用方插入）
    (
        " MIXEDLEFT2-MARKER The third page continues the two column "
        "body with discussion paragraphs that fill both columns "
        "naturally through the automatic flow mechanism described "
        "earlier in the typesetting engine documentation."
    ),
    (
        "Visualization of learned spaces uses stochastic neighbor "
        "embedding projections with perplexity thirty and reveals "
        "coherent clusters corresponding to the novel categories."
    ),
    (
        " RIGHTCOL2-MARKER Finally we conclude that mixed layouts "
        "are handled by detecting spanning elements before reading "
        "order sorting is applied to remaining column blocks."
    ),
    (
        "Together these mechanisms produce a compact yet realistic "
        "sample of mixed layout academic typography suitable for "
        "unit testing reading order detection across pages."
    ),
    (
        "Margin kerning and ligature formation refine the spacing "
        "between individual glyph pairs beyond what uniform character "
        "advance widths alone can achieve for high quality output."
    ),
    (
        "Stream objects are compressed with the flate algorithm by "
        "default which reduces file size substantially for text heavy "
        "documents while keeping random access to pages intact."
    ),
    (
        "Cross references may be stored as traditional tables or as "
        "compressed cross reference streams depending on the producer "
        "version and the compatibility targets of the document."
    ),
    (
        "Metadata in the extended format carries bibliographic fields "
        "such as title, author, subject, and keywords for indexing by "
        "external catalogue systems and desktop search engines."
    ),
    (
        "Outline entries form a hierarchical table of contents with "
        "destination references that survive deep copying of pages "
        "between documents under conforming implementations."
    ),
    (
        "Annotation subtypes include text notes, highlights, and file "
        "attachments, none of which participate in the main content "
        "stream parsing performed by the extraction pipeline."
    ),
    (
        "Finally the trailer dictionary ties the object graph together "
        "with the root catalog reference and encryption parameters if "
        "present, completing the structural overview of the format."
    ),
]


def build_mixed_layout(path: str) -> None:
    """页 1 跨栏标题（cover）→ 页 2/3 双栏正文，右栏嵌图。"""
    img = jpeg_path("img_green.jpg", 140, 90, (60, 180, 90))
    left_x, right_x = two_col_geometry()
    gutter = 24
    col_w = (PAGE_W - 2 * MARGIN - gutter) / 2
    top = PAGE_H - MARGIN

    c = pdfcanvas.Canvas(path, pagesize=letter)

    # ── 页 1：跨栏标题 ──
    c.setFont("Vera-Bold", 24)
    c.drawString(MARGIN, top - 24, "MIXEDTITLE-MARKER A Study of Mixed Layout Documents")
    c.setFont("Vera", 11)
    c.drawCentredString(PAGE_W / 2, top - 60, "Anonymous Authors — Institution — email at example dot org")
    c.showPage()

    # ── 页 2/3：双栏 ──
    col_x, y = left_x, top
    for i, para in enumerate(MIXED_BODY):
        if y < MARGIN + LEADING * 6:
            if col_x == left_x:
                col_x, y = right_x, top
            else:
                c.showPage()
                col_x, y = left_x, top
        y -= draw_wrapped(c, para, col_x, y, col_w)
        # MIXEDRIGHT 段后插图（右栏）
        if "MIXEDRIGHT-MARKER" in para:
            img_h = 1.2 * inch
            c.drawImage(img, col_x, y - img_h + 10, width=1.9 * inch, height=img_h)
            y -= img_h + 20
            c.setFont("Vera", 9)
            c.drawString(col_x, y, "Figure 1: An example column width figure.")
            y -= 24
    c.showPage()
    c.save()


def build_empty(path: str) -> None:
    c = pdfcanvas.Canvas(path, pagesize=letter)
    c.showPage()
    c.save()


def build_single_image(path: str) -> None:
    img = jpeg_path("img_solo.jpg", 220, 140, (120, 120, 30))
    c = pdfcanvas.Canvas(path, pagesize=letter)
    c.drawImage(img, (PAGE_W - 220) / 2, (PAGE_H - 140) / 2, width=220, height=140)
    c.showPage()
    c.save()


def build_large(path: str, pages: int) -> None:
    c = pdfcanvas.Canvas(path, pagesize=letter)
    for i in range(pages):
        y = PAGE_H - MARGIN
        c.setFont("Vera-Bold", 13)
        c.drawString(MARGIN, y, f"LARGEPAGE {i + 1} Benchmark Corpus Section")
        c.setFont("Vera", BODY_SIZE)
        y -= 2 * LEADING
        for j in range(12):
            c.drawString(
                MARGIN,
                y,
                f"Paragraph {i}-{j}: repeated filler sentence about scalable "
                "parsing performance over very long documents for measurement.",
            )
            y -= LEADING
        c.showPage()
    c.save()


def build_single_column(path: str, story) -> None:
    SimpleDocTemplate(
        path, pagesize=letter, leftMargin=MARGIN, rightMargin=MARGIN, topMargin=MARGIN, bottomMargin=MARGIN
    ).build(story)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pages", type=int, default=100, help="large.pdf 页数")
    parser.add_argument("--out", default=None, help="large 输出路径（默认 test/fixtures/pdf/large.pdf）")
    args = parser.parse_args()

    os.makedirs(OUT_DIR, exist_ok=True)
    # 先落盘 JPEG（platypus Image 需要文件路径）
    jpeg_path("img_red.jpg", 160, 100, (200, 60, 60))
    jpeg_path("img_blue.jpg", 120, 120, (60, 90, 200))
    jpeg_path("img_green.jpg", 140, 90, (60, 180, 90))
    jpeg_path("img_solo.jpg", 220, 140, (120, 120, 30))

    build_simple(os.path.join(OUT_DIR, "simple.pdf"))
    build_headings(os.path.join(OUT_DIR, "headings.pdf"))
    build_with_image(os.path.join(OUT_DIR, "with_image.pdf"))
    build_with_table(os.path.join(OUT_DIR, "with_table.pdf"))
    build_with_formula(os.path.join(OUT_DIR, "with_formula.pdf"))
    build_two_column(os.path.join(OUT_DIR, "two_column.pdf"))
    build_mixed_layout(os.path.join(OUT_DIR, "mixed_layout.pdf"))
    build_empty(os.path.join(OUT_DIR, "empty.pdf"))
    build_single_image(os.path.join(OUT_DIR, "single_image.pdf"))
    out = args.out or os.path.join(OUT_DIR, "large.pdf")
    build_large(out, args.pages)

    print(f"OK: fixtures written to {OUT_DIR} (large={out}, pages={args.pages})")


if __name__ == "__main__":
    main()
