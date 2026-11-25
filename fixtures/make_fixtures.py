import base64
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import zipfile
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
REF = os.path.join(HERE, "reference")

NS = (
    'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
    'xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" '
    'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" '
    'xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" '
    'xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" '
    'xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" '
    'xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" '
    'xmlns:xlink="http://www.w3.org/1999/xlink" '
    'xmlns:dc="http://purl.org/dc/elements/1.1/" '
    'office:version="1.3" office:mimetype="application/vnd.oasis.opendocument.text"'
)


def fodt(styles, master, body):
    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n<office:document %s>'
        "<office:automatic-styles>%s</office:automatic-styles>"
        "<office:master-styles>%s</office:master-styles>"
        "<office:body><office:text>%s</office:text></office:body></office:document>" % (NS, styles, master, body)
    )


PAGE = '<style:page-layout style:name="pm"><style:page-layout-properties fo:page-width="21cm" fo:page-height="29.7cm" fo:margin="2cm"/><style:header-style/><style:footer-style/></style:page-layout>'
MASTER = '<style:master-page style:name="Standard" style:page-layout-name="pm"/>'


def cell(text, style="c", span=None):
    attrs = ' table:style-name="%s"' % style
    if span:
        attrs += ' table:number-columns-spanned="%d"' % span
    return '<table:table-cell%s office:value-type="string"><text:p>%s</text:p></table:table-cell>' % (attrs, text)


def tables_doc():
    styles = PAGE + (
        '<style:style style:name="c" style:family="table-cell"><style:table-cell-properties fo:border="0.5pt solid #000000" fo:padding="0.1cm"/></style:style>'
        '<style:style style:name="h" style:family="table-cell"><style:table-cell-properties fo:border="0.5pt solid #000000" fo:background-color="#d9e2f3" fo:padding="0.1cm"/></style:style>'
        '<style:style style:name="s" style:family="table-cell"><style:table-cell-properties fo:border="1.5pt double #c00000" fo:background-color="#fff2cc" fo:padding="0.1cm"/></style:style>'
    )
    head = '<table:table-header-rows><table:table-row>%s</table:table-row></table:table-header-rows>' % "".join(cell(h, "h") for h in ["Item", "Q1", "Q2", "Q3"])
    rows = "".join("<table:table-row>%s</table:table-row>" % "".join(cell(v) for v in ["Row %d" % i, str(i * 3), str(i * 5), str(i * 7)]) for i in range(1, 70))
    merged = "<table:table-row>%s<table:covered-table-cell/><table:covered-table-cell/>%s</table:table-row>" % (cell("Merged across three columns", "s", 3), cell("End", "s"))
    nested_inner = '<table:table table:name="inner"><table:table-column table:number-columns-repeated="2"/><table:table-row>%s</table:table-row><table:table-row>%s</table:table-row></table:table>' % (cell("a") + cell("b"), cell("c") + cell("d"))
    nested = '<table:table-row><table:table-cell table:style-name="c">%s</table:table-cell>%s</table:table-row>' % (nested_inner, cell("Nested table on the left") + cell("x") + cell("y"))
    body = (
        "<text:h text:outline-level=\"1\">Complex tables</text:h>"
        "<text:p>The header row repeats on every page; one row merges three cells; one cell holds a nested table.</text:p>"
        '<table:table table:name="t1"><table:table-column table:number-columns-repeated="4"/>%s%s%s%s</table:table>' % (head, merged, nested, rows)
    )
    return fodt(styles, MASTER, body)


def png(width, height, rgb):
    raw = b"".join(b"\x00" + bytes(rgb) * width for _ in range(height))
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def layout_doc():
    styles = (
        '<style:page-layout style:name="pm"><style:page-layout-properties fo:page-width="21cm" fo:page-height="29.7cm" fo:margin="2cm"/>'
        '<style:header-style><style:header-footer-properties fo:min-height="0.8cm"/></style:header-style>'
        '<style:footer-style><style:header-footer-properties fo:min-height="0.8cm"/></style:footer-style></style:page-layout>'
        '<style:style style:name="cols" style:family="section"><style:section-properties><style:columns fo:column-count="2" fo:column-gap="0.8cm"/></style:section-properties></style:style>'
        '<style:style style:name="box" style:family="graphic"><style:graphic-properties fo:border="1pt solid #2f5496" fo:background-color="#deebf7" style:wrap="parallel" fo:padding="0.2cm"/></style:style>'
        '<style:style style:name="img" style:family="graphic"><style:graphic-properties style:wrap="right" style:horizontal-pos="left" style:vertical-pos="top"/></style:style>'
    )
    master = (
        '<style:master-page style:name="Standard" style:page-layout-name="pm">'
        "<style:header><text:p>Lumen layout fixture</text:p></style:header>"
        '<style:footer><text:p>Page <text:page-number text:select-page="current">1</text:page-number> of <text:page-count>1</text:page-count></text:p></style:footer>'
        "</style:master-page>"
    )
    words = "Layout fidelity depends on fonts, columns, anchors and notes all agreeing. " * 6
    image = base64.b64encode(png(120, 80, (224, 145, 47))).decode()
    body = (
        '<text:h text:outline-level="1">Sections, notes and frames</text:h>'
        "<text:p>A footnote<text:note text:note-class=\"footnote\"><text:note-citation>1</text:note-citation><text:note-body><text:p>Footnote text at the bottom of the page.</text:p></text:note-body></text:note>"
        " and an endnote<text:note text:note-class=\"endnote\"><text:note-citation>i</text:note-citation><text:note-body><text:p>Endnote text at the end.</text:p></text:note-body></text:note>.</text:p>"
        '<text:p><draw:frame draw:style-name="img" text:anchor-type="paragraph" svg:width="3cm" svg:height="2cm"><draw:image><office:binary-data>%s</office:binary-data></draw:image></draw:frame>%s</text:p>'
        '<text:p><draw:frame draw:style-name="box" text:anchor-type="paragraph" svg:width="7cm" svg:height="2.5cm" svg:x="9cm" svg:y="0cm"><draw:text-box><text:p>A text box anchored to this paragraph.</text:p></draw:text-box></draw:frame>%s</text:p>'
        '<text:section text:style-name="cols" text:name="two">%s</text:section>'
    ) % (image, words, words, "".join("<text:p>%s</text:p>" % words for _ in range(6)))
    return fodt(styles, master, body)


def scripts_doc():
    styles = PAGE + (
        '<style:style style:name="rtl" style:family="paragraph"><style:paragraph-properties style:writing-mode="rl-tb" fo:text-align="end"/></style:style>'
        '<style:style style:name="missing" style:family="text"><style:text-properties style:font-name="Imaginary Sans" fo:font-family="\'Imaginary Sans\'"/></style:style>'
    )
    changes = (
        "<text:tracked-changes>"
        '<text:changed-region text:id="c1"><text:insertion><office:change-info><dc:creator>Ana</dc:creator><dc:date>2026-01-01T10:00:00</dc:date></office:change-info></text:insertion></text:changed-region>'
        '<text:changed-region text:id="c2"><text:deletion><office:change-info><dc:creator>Ana</dc:creator><dc:date>2026-01-01T10:00:00</dc:date></office:change-info><text:p>removed words </text:p></text:deletion></text:changed-region>'
        "</text:tracked-changes>"
    )
    body = (
        changes
        + '<text:h text:outline-level="1">Scripts, fonts and tracked changes</text:h>'
        '<text:p text:style-name="rtl">هذا نص عربي يُكتب من اليمين إلى اليسار.</text:p>'
        '<text:p text:style-name="rtl">זהו טקסט בעברית.</text:p>'
        "<text:p>中文文本：文档查看器必须正确显示汉字。日本語のテキストも表示されます。한국어 텍스트.</text:p>"
        '<text:p><text:span text:style-name="missing">This run asks for a font that is not installed.</text:span></text:p>'
        '<text:p>Tracked changes shown as final: <text:change-start text:change-id="c1"/>inserted text <text:change-end text:change-id="c1"/><text:change text:change-id="c2"/>stays readable.</text:p>'
    )
    return fodt(styles, MASTER, body)


XLSX_STYLES = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<numFmts count="2"><numFmt numFmtId="164" formatCode="&quot;$&quot;#,##0.00"/><numFmt numFmtId="165" formatCode="yyyy-mm-dd"/></numFmts>
<fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><name val="Calibri"/></font></fonts>
<fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFD9E2F3"/></patternFill></fill></fills>
<borders count="2"><border/><border><left style="thin"/><right style="thin"/><top style="thin"/><bottom style="thin"/></border></borders>
<cellStyleXfs count="1"><xf/></cellStyleXfs>
<cellXfs count="5"><xf/><xf fontId="1" fillId="2" borderId="1" applyFont="1" applyFill="1" applyBorder="1"/><xf numFmtId="164" borderId="1" applyNumberFormat="1" applyBorder="1"/><xf numFmtId="10" borderId="1" applyNumberFormat="1" applyBorder="1"/><xf numFmtId="165" borderId="1" applyNumberFormat="1" applyBorder="1"/></cellXfs>
<dxfs count="1"><dxf><font><color rgb="FF9C0006"/></font><fill><patternFill><bgColor rgb="FFFFC7CE"/></patternFill></fill></dxf></dxfs>
</styleSheet>"""


def xlsx_numbers(path):
    rows = ['<row r="1"><c r="A1" s="1" t="inlineStr"><is><t>Budget by region</t></is></c></row>',
            '<row r="2">%s</row>' % "".join('<c r="%s2" s="1" t="inlineStr"><is><t>%s</t></is></c>' % (c, h) for c, h in zip("ABCD", ["Region", "Spend", "Growth", "Review date"]))]
    data = [("North", 125000.5, 0.124, 46022), ("South", 98000, -0.031, 46053), ("East", 143250.75, 0.287, 46081), ("West", 67000, -0.12, 46112)]
    for i, (region, spend, growth, date) in enumerate(data, start=3):
        rows.append('<row r="%d"><c r="A%d" t="inlineStr"><is><t>%s</t></is></c><c r="B%d" s="2"><v>%s</v></c><c r="C%d" s="3"><v>%s</v></c><c r="D%d" s="4"><v>%s</v></c></row>' % (i, i, region, i, spend, i, growth, i, date))
    sheet = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
        '<cols><col min="1" max="1" width="18" customWidth="1"/><col min="2" max="4" width="16" customWidth="1"/></cols>'
        "<sheetData>%s</sheetData>"
        '<mergeCells count="1"><mergeCell ref="A1:D1"/></mergeCells>'
        '<conditionalFormatting sqref="C3:C6"><cfRule type="cellIs" dxfId="0" priority="1" operator="lessThan"><formula>0</formula></cfRule></conditionalFormatting>'
        "</worksheet>" % "".join(rows)
    )
    workbook = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
        '<sheets><sheet name="Budget" sheetId="1" r:id="rId1"/></sheets>'
        '<definedNames><definedName name="_xlnm.Print_Area" localSheetId="0">Budget!$A$1:$D$6</definedName></definedNames></workbook>'
    )
    ct = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>'
        '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'
        '<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
        '<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>'
    )
    rels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>'
    wb_rels = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>'
        '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>'
    )
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", ct)
        z.writestr("_rels/.rels", rels)
        z.writestr("xl/workbook.xml", workbook)
        z.writestr("xl/_rels/workbook.xml.rels", wb_rels)
        z.writestr("xl/styles.xml", XLSX_STYLES)
        z.writestr("xl/worksheets/sheet1.xml", sheet)


def scanned_pdf(path):
    w, h = 1240, 1754
    pixels = bytearray(b"\xf4\xf1\xea" * (w * h))
    for line in range(30):
        y0 = 180 + line * 48
        length = 900 if line % 5 else 520
        for y in range(y0, y0 + 14):
            for x in range(150, 150 + length, 3):
                if (x // 37 + y // 7) % 4:
                    i = (y * w + x) * 3
                    pixels[i:i + 3] = b"\x2a\x2a\x2a"
    data = zlib.compress(bytes(pixels), 6)
    content = b"q 595 0 0 842 0 0 cm /Im1 Do Q"
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /XObject << /Im1 4 0 R >> >> /Contents 5 0 R >>",
        b"<< /Type /XObject /Subtype /Image /Width %d /Height %d /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length %d >>\nstream\n" % (w, h, len(data)) + data + b"\nendstream",
        b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream",
    ]
    with open(path, "wb") as f:
        f.write(b"%PDF-1.7\n")
        offsets = []
        for i, o in enumerate(objs, 1):
            offsets.append(f.tell())
            f.write(b"%d 0 obj\n" % i + o + b"\nendobj\n")
        xref = f.tell()
        f.write(b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1) + b"".join(b"%010d 00000 n \n" % o for o in offsets))
        f.write(b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, xref))


def soffice():
    found = shutil.which("soffice") or next((p for p in [r"C:\Program Files\LibreOffice\program\soffice.exe", "/Applications/LibreOffice.app/Contents/MacOS/soffice"] if os.path.exists(p)), None)
    if not found:
        sys.exit("LibreOffice is required to build the fixtures")
    return found


def convert(src, fmt, outdir, tmp):
    profile = "file:///" + os.path.join(tmp, "profile").replace(os.sep, "/").lstrip("/")
    subprocess.run([soffice(), "-env:UserInstallation=" + profile, "--headless", "--convert-to", fmt, "--outdir", outdir, src], check=True, capture_output=True, timeout=300)


def main():
    os.makedirs(REF, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        for name, build in [("tables", tables_doc), ("layout", layout_doc), ("scripts", scripts_doc)]:
            src = os.path.join(tmp, name + ".fodt")
            with open(src, "w", encoding="utf-8") as f:
                f.write(build())
            convert(src, "docx", HERE, tmp)
        xlsx_numbers(os.path.join(HERE, "numbers.xlsx"))
        shutil.copy(os.path.join(HERE, "..", "bench", "samples", "deck.pptx"), os.path.join(HERE, "deck.pptx"))
        scanned_pdf(os.path.join(HERE, "scanned.pdf"))
        shutil.copy(os.path.join(HERE, "scanned.pdf"), os.path.join(REF, "scanned.pdf"))
    for name in sorted(os.listdir(HERE)) + ["reference/" + n for n in sorted(os.listdir(REF))]:
        path = os.path.join(HERE, name)
        if os.path.isfile(path):
            print("%-28s %8.1f KB" % (name, os.path.getsize(path) / 1024))


if __name__ == "__main__":
    main()
