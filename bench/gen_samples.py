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


def main():
    os.makedirs(OUT, exist_ok=True)
    rnd = random.Random(7)
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
    for name in sorted(os.listdir(OUT)):
        print(f"{name}: {os.path.getsize(os.path.join(OUT, name)) / 1e6:.1f} MB")


if __name__ == "__main__":
    sys.exit(main())
