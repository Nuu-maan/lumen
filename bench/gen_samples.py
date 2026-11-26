import os, random, sys, zlib

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "samples")


def write_pdf(path, pages):
    objs = []

    def add(body):
        objs.append(body)
        return len(objs)

    font = add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    pages_id = len(objs) + 1 + 0
    objs.append(None)
    kids = []
    for content, image in pages:
        resources = b"<< /Font << /F1 %d 0 R >>" % font
        if image:
            w, h, data = image
            img = add(b"<< /Type /XObject /Subtype /Image /Width %d /Height %d /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length %d >>\nstream\n" % (w, h, len(data)) + data + b"\nendstream")
            resources += b" /XObject << /Im1 %d 0 R >>" % img
        resources += b" >>"
        stream = zlib.compress(content)
        c = add(b"<< /Length %d /Filter /FlateDecode >>\nstream\n" % len(stream) + stream + b"\nendstream")
        kids.append(add(b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 612 792] /Resources %s /Contents %d 0 R >>" % (pages_id, resources, c)))
    objs[pages_id - 1] = b"<< /Type /Pages /Kids [%s] /Count %d >>" % (b" ".join(b"%d 0 R" % k for k in kids), len(kids))
    catalog = add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages_id)
    with open(path, "wb") as f:
        f.write(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
        offsets = []
        for i, body in enumerate(objs, 1):
            offsets.append(f.tell())
            f.write(b"%d 0 obj\n" % i + body + b"\nendobj\n")
        xref = f.tell()
        f.write(b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1))
        for off in offsets:
            f.write(b"%010d 00000 n \n" % off)
        f.write(b"trailer\n<< /Size %d /Root %d 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, catalog, xref))


WORDS = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua".split()


def text_page(n, rnd):
    lines = [b"BT /F1 18 Tf 72 730 Td (Chapter %d) Tj ET" % n]
    y = 700
    while y > 60:
        words = " ".join(rnd.choice(WORDS) for _ in range(14))
        lines.append(b"BT /F1 10 Tf 72 %d Td (%s) Tj ET" % (y, words.encode()))
        y -= 13
    lines.append(b"0.85 0.4 0.1 rg 72 40 468 2 re f")
    return b"\n".join(lines)

def write_log(path, size):
    line = b"2026-11-25T10:00:00.000Z INFO  worker-%02d request id=%08d path=/api/v1/items status=200 took=%dms\n"
    block = b"".join(line % (i % 16, i, i % 900) for i in range(20000))
    with open(path, "wb") as f:
        written = 0
        while written < size:
            f.write(block)
            written += len(block)


MARKDOWN = """# Lumen sample

A **Markdown** file with a table:

| Format | Engine |
|---|---|
| PDF | MuPDF |
| DOCX | LibreOffice |

- [x] lists
- [ ] tasks

```rust
fn main() {}
```

<script>document.body.innerHTML = 'scripts must not run'</script>
"""

HTML = """<!doctype html><html><body style="font-family:sans-serif"><h1>HTML sample</h1>
<p id="p">Scripts are blocked.</p><script>document.getElementById('p').textContent = 'SCRIPT RAN'</script>
<img src="https://example.com/tracker.png" alt="remote image (blocked)"></body></html>
"""

EML = b"""From: Ana Silva <ana@example.com>\r
To: Bo Chen <bo@example.com>\r
Subject: Quarterly numbers\r
Date: Tue, 1 Jul 2025 10:00:00 +0000\r
MIME-Version: 1.0\r
Content-Type: multipart/mixed; boundary=XX\r
\r
--XX\r
Content-Type: text/html; charset=utf-8\r
\r
<h2>Hi Bo,</h2><p>The numbers are attached.</p>\r
--XX\r
Content-Type: text/csv\r
Content-Disposition: attachment; filename=numbers.csv\r
\r
quarter,revenue\r
Q1,120\r
Q2,135\r
--XX--\r
"""

SVG = """<svg xmlns="http://www.w3.org/2000/svg" width="200" height="120"><rect width="200" height="120" fill="#e0912f"/>
<text x="20" y="70" font-size="32" fill="#fff">Lumen</text><script>alert(1)</script></svg>
"""

RUST = """use std::io;

fn main() -> io::Result<()> {
    let name = "Lumen";
    println!("hello {name}");
    Ok(())
}
"""


def write_text_samples():
    import zipfile

    write_log(os.path.join(OUT, "huge-500mb.log"), 500 * 1000 * 1000)
    for name, data in [("readme.md", MARKDOWN), ("page.html", HTML), ("logo.svg", SVG), ("main.rs", RUST * 50)]:
        with open(os.path.join(OUT, name), "w", encoding="utf-8", newline="\n") as f:
            f.write(data)
    with open(os.path.join(OUT, "message.eml"), "wb") as f:
        f.write(EML)
    with zipfile.ZipFile(os.path.join(OUT, "bundle.zip"), "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("docs/readme.md", "# Inside a zip\n")
        z.writestr("src/main.rs", RUST)
        z.write(os.path.join(OUT, "text-100p.pdf"), "report.pdf")


CT_XLSX = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
<Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>
</Types>"""

ROOT_RELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="%s"/>
</Relationships>"""

WORKBOOK = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="Orders" sheetId="1" r:id="rId1"/><sheet name="Summary" sheetId="2" r:id="rId2"/></sheets></workbook>"""

WORKBOOK_RELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/>
<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
</Relationships>"""

STYLES = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts>
<fills count="1"><fill><patternFill patternType="none"/></fill></fills>
<borders count="1"><border/></borders>
<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
<cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="14" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/></cellXfs>
</styleSheet>"""

CT_DOCX = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"""


def cell(ref, value, style=0):
    if isinstance(value, str):
        return '<c r="%s" t="inlineStr"><is><t>%s</t></is></c>' % (ref, value)
    return '<c r="%s"%s><v>%s</v></c>' % (ref, ' s="%d"' % style if style else "", value)


def write_xlsx(path, rows):
    import zipfile

    cols = "ABCDEFGH"
    header = ["Order", "Date", "Region", "Product", "Units", "Price", "Total", "Status"]
    regions = ["North", "South", "East", "West"]
    products = ["Widget", "Gadget", "Gizmo", "Doohickey"]
    parts = ['<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">',
             '<sheetViews><sheetView workbookViewId="0"><pane xSplit="1" ySplit="1" topLeftCell="B2" activePane="bottomRight" state="frozen"/></sheetView></sheetViews><sheetData>',
             '<row r="1">' + "".join(cell(c + "1", h) for c, h in zip(cols, header)) + "</row>"]
    for i in range(2, rows + 2):
        units = i % 50 + 1
        price = 2.5 + (i % 7)
        values = [i - 1, 45658 + i % 365, regions[i % 4], products[i % 4], units, price, round(units * price, 2), "Shipped" if i % 3 else "Open"]
        parts.append('<row r="%d">%s</row>' % (i, "".join(cell("%s%d" % (c, i), v, 1 if c == "B" else 0) for c, v in zip(cols, values))))
    parts.append("</sheetData></worksheet>")
    summary = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1">%s</row></sheetData></worksheet>' % (cell("A1", "Rows") + cell("B1", rows))
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", CT_XLSX)
        z.writestr("_rels/.rels", ROOT_RELS % "xl/workbook.xml")
        z.writestr("xl/workbook.xml", WORKBOOK)
        z.writestr("xl/_rels/workbook.xml.rels", WORKBOOK_RELS)
        z.writestr("xl/worksheets/sheet1.xml", "".join(parts))
        z.writestr("xl/worksheets/sheet2.xml", summary)
        z.writestr("xl/styles.xml", STYLES)


def write_docx(path, pages, rnd):
    import zipfile

    body = []
    for page in range(pages):
        body.append('<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Section %d</w:t></w:r></w:p>' % (page + 1))
        for _ in range(9):
            words = " ".join(rnd.choice(WORDS) for _ in range(60))
            body.append("<w:p><w:r><w:t>%s</w:t></w:r></w:p>" % words)
        rows = "".join("<w:tr>%s</w:tr>" % "".join("<w:tc><w:p><w:r><w:t>R%dC%d</w:t></w:r></w:p></w:tc>" % (r, c) for c in range(4)) for r in range(4))
        body.append('<w:tbl><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4"/><w:bottom w:val="single" w:sz="4"/><w:insideH w:val="single" w:sz="4"/><w:insideV w:val="single" w:sz="4"/></w:tblBorders></w:tblPr>%s</w:tbl>' % rows)
        body.append('<w:p><w:r><w:br w:type="page"/></w:r></w:p>')
    doc = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>%s</w:body></w:document>' % "".join(body)
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", CT_DOCX)
        z.writestr("_rels/.rels", ROOT_RELS % "word/document.xml")
        z.writestr("word/document.xml", doc)


FODP_HEAD = """<?xml version="1.0" encoding="UTF-8"?>
<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"
 xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0"
 xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0"
 xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" office:version="1.3" office:mimetype="application/vnd.oasis.opendocument.presentation">
<office:automatic-styles>
<style:page-layout style:name="PM1"><style:page-layout-properties fo:page-width="28cm" fo:page-height="15.75cm"/></style:page-layout>
<style:style style:name="big" style:family="paragraph"><style:text-properties fo:font-size="36pt" fo:font-weight="bold"/></style:style>
</office:automatic-styles>
<office:master-styles><style:master-page style:name="Default" style:page-layout-name="PM1"/></office:master-styles>
<office:body><office:presentation>
"""

FODP_SLIDE = """<draw:page draw:name="s%(n)d" draw:master-page-name="Default">
<draw:frame svg:x="2cm" svg:y="2cm" svg:width="24cm" svg:height="3cm"><draw:text-box><text:p text:style-name="big">%(title)s</text:p></draw:text-box></draw:frame>
<draw:frame svg:x="2cm" svg:y="6cm" svg:width="24cm" svg:height="8cm"><draw:text-box><text:p>%(body)s</text:p></draw:text-box></draw:frame>
<presentation:notes><draw:frame presentation:class="notes" svg:x="2cm" svg:y="14cm" svg:width="17cm" svg:height="10cm"><draw:text-box><text:p>%(note)s</text:p></draw:text-box></draw:frame></presentation:notes>
</draw:page>
"""


def write_deck(path):
    import shutil
    import subprocess
    import tempfile

    soffice = shutil.which("soffice") or next((p for p in [r"C:\Program Files\LibreOffice\program\soffice.exe", "/Applications/LibreOffice.app/Contents/MacOS/soffice"] if os.path.exists(p)), None)
    if not soffice:
        print("skipping deck.pptx: LibreOffice not found")
        return
    slides = [("Quarterly review", "Revenue grew 12% quarter over quarter.", "Open with the headline number."),
              ("Regions", "North and West led growth.", "Mention the new West office."),
              ("Next steps", "Hire two engineers. Ship the viewer.", "")]
    body = "".join(FODP_SLIDE % {"n": i, "title": t, "body": b, "note": n} for i, (t, b, n) in enumerate(slides))
    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "deck.fodp")
        with open(src, "w", encoding="utf-8") as f:
            f.write(FODP_HEAD + body + "</office:presentation></office:body></office:document>")
        profile = "file:///" + os.path.join(tmp, "profile").replace(os.sep, "/")
        subprocess.run([soffice, "-env:UserInstallation=" + profile, "--headless", "--convert-to", "pptx", "--outdir", tmp, src], check=True, capture_output=True, timeout=180)
        shutil.move(os.path.join(tmp, "deck.pptx"), path)


def write_office_samples(rnd):
    write_deck(os.path.join(OUT, "deck.pptx"))
    write_xlsx(os.path.join(OUT, "orders-100k.xlsx"), 100_000)
    write_docx(os.path.join(OUT, "report-20p.docx"), 20, rnd)
    with open(os.path.join(OUT, "orders.csv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("order,region,note\n1,North,\"multi\nline\"\n2,South,plain\n")


def main():
    os.makedirs(OUT, exist_ok=True)
    rnd = random.Random(7)
    if "--office-only" in sys.argv:
        write_office_samples(rnd)
        return
    if "--text-only" in sys.argv:
        write_text_samples()
        return
    write_pdf(os.path.join(OUT, "text-100p.pdf"), [(text_page(i + 1, rnd), None) for i in range(100)])
    pages = []
    for i in range(40):
        w, h = 1200, 900
        raw = bytes(rnd.getrandbits(8) for _ in range(w * h * 3)) if i == 0 else os.urandom(w * h * 3)
        image = (w, h, zlib.compress(raw, 1))
        pages.append((b"q 468 351 0 0 72 380 cm /Im1 Do Q\n" + text_page(i + 1, rnd), image))
    write_pdf(os.path.join(OUT, "images-100mb.pdf"), pages)
    write_text_samples()
    write_office_samples(rnd)
    for name in sorted(os.listdir(OUT)):
        print(f"{name}: {os.path.getsize(os.path.join(OUT, name)) / 1e6:.1f} MB")


if __name__ == "__main__":
    sys.exit(main())
