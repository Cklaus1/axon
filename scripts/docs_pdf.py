#!/usr/bin/env python3
"""Bundle the finalized Axon docs into one PDF.

Deliberately dependency-light (reportlab only): pandoc/LaTeX are not installed
on this host and requiring them would make the bundle un-regenerable. Mermaid
blocks are rendered as labelled source rather than images — a diagram that
silently vanished from the PDF would be worse than one shown as its definition,
and the .md files remain the place diagrams actually render.

Usage: python3 scripts/docs_pdf.py [OUT.pdf]
"""
import os
import re
import sys
from reportlab.lib.pagesizes import LETTER
from reportlab.lib.styles import getSampleStyleSheet, ParagraphStyle
from reportlab.lib.units import inch
from reportlab.lib import colors
from reportlab.platypus import (
    SimpleDocTemplate, Paragraph, Spacer, Preformatted, PageBreak, Table, TableStyle,
)

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Order matters: this is a reading order, not a directory listing.
DOCS = [
    ("ARCHITECTURE.md", "Architecture — how it is put together, and why"),
    ("COMPARISON.md", "Compared — against languages and sandboxing substrates"),
    ("CLAUDE.md", "Working notes — the curated command path"),
    ("AXON_REFERENCE.md", "Reference — the complete generated surface"),
    ("README.md", "README"),
    ("ROADMAP.md", "Roadmap"),
    ("governance/ARCHITECTURE_INVARIANTS.md", "Invariants — the rules no change may break"),
    ("governance/EXIT_CODES.md", "Exit codes"),
]

ss = getSampleStyleSheet()
BODY = ParagraphStyle("body", parent=ss["BodyText"], fontSize=9.2, leading=13, spaceAfter=5)
H1 = ParagraphStyle("h1", parent=ss["Heading1"], fontSize=17, spaceBefore=16, spaceAfter=9,
                    textColor=colors.HexColor("#1b3a5c"))
H2 = ParagraphStyle("h2", parent=ss["Heading2"], fontSize=13, spaceBefore=13, spaceAfter=6,
                    textColor=colors.HexColor("#2d6a4f"))
H3 = ParagraphStyle("h3", parent=ss["Heading3"], fontSize=10.8, spaceBefore=10, spaceAfter=4)
CODE = ParagraphStyle("code", parent=ss["Code"], fontSize=7.3, leading=9,
                      backColor=colors.HexColor("#f4f4f6"), borderPadding=5,
                      leftIndent=6, spaceBefore=4, spaceAfter=6)
QUOTE = ParagraphStyle("quote", parent=BODY, leftIndent=16, textColor=colors.HexColor("#444"),
                       borderPadding=3)


def esc(t: str) -> str:
    """Markdown inline -> reportlab markup.

    Code spans are tokenized FIRST and never re-processed. A naive
    escape-then-regex pass lets `**bold**` straddle a backtick boundary and emit
    interleaved tags (`<b>...</font>...</b>`), which reportlab rejects outright —
    one such cell in CLAUDE.md killed the whole bundle. Splitting on backticks
    means inline markup simply cannot cross a code boundary: at worst an asterisk
    survives literally, which is cosmetic rather than fatal.
    """
    def plain(x: str) -> str:
        x = x.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
        x = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", x)       # links -> their text
        x = re.sub(r"\*\*([^*]+)\*\*", r"<b>\1</b>", x)
        x = re.sub(r"(?<![\w*])\*([^*\n]+)\*(?![\w*])", r"<i>\1</i>", x)
        return x

    out = []
    for n, seg in enumerate(t.split("`")):
        if n % 2:                                             # inside backticks
            code = seg.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
            out.append(f'<font face="Courier" size="8">{code}</font>')
        else:
            out.append(plain(seg))
    return "".join(out)


# Paragraphs that reportlab still refuses are counted, not swallowed: a bundle
# that silently dropped content would look complete.
DEGRADED = []


def para(text: str, style) -> Paragraph:
    """Build a Paragraph, falling back to fully-escaped plain text if the
    markup is rejected — and recording that it happened."""
    try:
        return Paragraph(text, style)
    except Exception as e:                                    # noqa: BLE001
        DEGRADED.append(f"{type(e).__name__}: {str(e)[:90]}")
        flat = re.sub(r"<[^>]+>", "", text)
        flat = flat.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
        return Paragraph(flat, style)


def render(md: str, flow: list) -> None:
    lines = md.split("\n")
    i = 0
    table: list[list[str]] = []

    def flush_table() -> None:
        nonlocal table
        if not table:
            return
        # A markdown separator row (---|---) carries no data.
        rows = [r for r in table if not all(set(c.strip()) <= set("-: ") for c in r)]
        if rows:
            ncols = max(len(r) for r in rows)
            # A row taller than the page cannot be laid out at all — reportlab
            # raises LayoutError rather than splitting it, and CLAUDE.md's
            # phase-status table has cells running to thousands of characters.
            # Such a table is unreadable as a grid anyway, so render it as
            # labelled prose instead of failing or silently dropping it.
            widest = max((len(c) for r in rows for c in r), default=0)
            if widest > 400:
                header = rows[0] if len(rows) > 1 else []
                for r in rows[1:] if header else rows:
                    for n, cell in enumerate(r):
                        if not cell.strip():
                            continue
                        label = header[n].strip() if n < len(header) and header[n].strip() else ""
                        body = f"<b>{esc(label)}:</b> {esc(cell)}" if label else esc(cell)
                        flow.append(para(body, ParagraphStyle(
                            "kv", parent=BODY, leftIndent=10, fontSize=8, leading=11)))
                    flow.append(Spacer(1, 5))
                table = []
                return
            data = [[para(esc(c), ParagraphStyle("cell", parent=BODY, fontSize=7.4, leading=9))
                     for c in (r + [""] * (ncols - len(r)))] for r in rows]
            avail = 6.9 * inch
            t = Table(data, colWidths=[avail / ncols] * ncols, hAlign="LEFT")
            t.setStyle(TableStyle([
                ("GRID", (0, 0), (-1, -1), 0.25, colors.HexColor("#c8c8d0")),
                ("BACKGROUND", (0, 0), (-1, 0), colors.HexColor("#e8ecf2")),
                ("VALIGN", (0, 0), (-1, -1), "TOP"),
                ("LEFTPADDING", (0, 0), (-1, -1), 4),
                ("RIGHTPADDING", (0, 0), (-1, -1), 4),
                ("TOPPADDING", (0, 0), (-1, -1), 3),
                ("BOTTOMPADDING", (0, 0), (-1, -1), 3),
            ]))
            flow.append(t)
            flow.append(Spacer(1, 7))
        table = []

    while i < len(lines):
        ln = lines[i]
        if ln.startswith("```"):
            lang = ln[3:].strip()
            block: list[str] = []
            i += 1
            while i < len(lines) and not lines[i].startswith("```"):
                block.append(lines[i])
                i += 1
            i += 1
            flush_table()
            if lang == "mermaid":
                flow.append(para(
                    "<i>Diagram (Mermaid) — renders in the Markdown source:</i>", BODY))
            body = "\n".join(block)
            # Long code lines would run off the page; reportlab will not wrap
            # Preformatted, so truncation is visible rather than silent.
            body = "\n".join(l if len(l) <= 108 else l[:105] + "..." for l in body.split("\n"))
            flow.append(Preformatted(body, CODE))
            continue
        if ln.startswith("|") and ln.count("|") >= 2:
            table.append([c.strip() for c in ln.strip().strip("|").split("|")])
            i += 1
            continue
        flush_table()
        if ln.startswith("#### ") or ln.startswith("### "):
            flow.append(para(esc(ln.lstrip("# ")), H3))
        elif ln.startswith("## "):
            flow.append(para(esc(ln[3:]), H2))
        elif ln.startswith("# "):
            flow.append(para(esc(ln[2:]), H1))
        elif ln.startswith("> "):
            flow.append(para(esc(ln[2:]), QUOTE))
        elif re.match(r"^\s*[-*] ", ln):
            flow.append(para("• " + esc(re.sub(r"^\s*[-*] ", "", ln)),
                                  ParagraphStyle("li", parent=BODY, leftIndent=12)))
        elif re.match(r"^\s*\d+\. ", ln):
            flow.append(para(esc(ln.strip()),
                                  ParagraphStyle("li", parent=BODY, leftIndent=12)))
        elif ln.strip() in ("---", "***"):
            flow.append(Spacer(1, 9))
        elif ln.strip() == "":
            flow.append(Spacer(1, 3))
        elif ln.strip().startswith("<!--"):
            pass
        else:
            flow.append(para(esc(ln), BODY))
        i += 1
    flush_table()


def main() -> int:
    out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "axon-docs.pdf")
    flow: list = []

    flow.append(Spacer(1, 2.0 * inch))
    flow.append(Paragraph("Axon", ParagraphStyle("t", parent=ss["Title"], fontSize=38,
                                                 textColor=colors.HexColor("#1b3a5c"))))
    flow.append(Spacer(1, 8))
    flow.append(Paragraph("AI-written code, sandboxed by the compiler",
                          ParagraphStyle("st", parent=ss["Title"], fontSize=14,
                                         textColor=colors.HexColor("#2d6a4f"))))
    flow.append(Spacer(1, 26))
    included = [(p, t) for p, t in DOCS if os.path.exists(os.path.join(ROOT, p))]
    toc = "<br/>".join(f"{n}. {t}" for n, (_, t) in enumerate(included, 1))
    flow.append(Paragraph(toc, ParagraphStyle("toc", parent=BODY, alignment=1, leading=16)))
    flow.append(Spacer(1, 20))
    flow.append(Paragraph(
        "<i>Generated by scripts/docs_pdf.py. Mermaid diagrams appear as source; "
        "they render in the Markdown originals.</i>",
        ParagraphStyle("note", parent=BODY, alignment=1, fontSize=8,
                       textColor=colors.HexColor("#666"))))

    missing = [p for p, _ in DOCS if not os.path.exists(os.path.join(ROOT, p))]
    for path, title in included:
        flow.append(PageBreak())
        flow.append(Paragraph(title, H1))
        flow.append(Spacer(1, 5))
        with open(os.path.join(ROOT, path), encoding="utf-8") as fh:
            render(fh.read(), flow)

    def footer(canvas, doc):
        canvas.saveState()
        canvas.setFont("Helvetica", 7.5)
        canvas.setFillColor(colors.HexColor("#888"))
        canvas.drawString(0.8 * inch, 0.55 * inch, "Axon — architecture & reference")
        canvas.drawRightString(7.7 * inch, 0.55 * inch, str(doc.page))
        canvas.restoreState()

    SimpleDocTemplate(
        out, pagesize=LETTER,
        leftMargin=0.8 * inch, rightMargin=0.8 * inch,
        topMargin=0.75 * inch, bottomMargin=0.75 * inch,
        title="Axon — architecture & reference", author="Axon",
    ).build(flow, onFirstPage=footer, onLaterPages=footer)

    print(f"wrote {out} ({os.path.getsize(out) // 1024} KB, {len(included)} documents)")
    if DEGRADED:
        print(f"NOTE: {len(DEGRADED)} paragraph(s) fell back to plain text:")
        for d in DEGRADED[:5]:
            print(f"  {d}")
    if missing:
        # Naming what was skipped, not silently shipping a short bundle.
        print(f"SKIPPED (not found): {', '.join(missing)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
