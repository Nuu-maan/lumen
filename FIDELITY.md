# Fidelity

Every fixture in `fixtures/` is opened through Lumen's own pipeline (LibreOffice worker → PDF → MuPDF) and compared page by page against `fixtures/reference/<name>.pdf`. Both are rasterized at 72 dpi with MuPDF. A pixel counts as different when any channel differs by more than 24. A page passes at ≤ 2 % different pixels. Diff images for failing pages land in `src-tauri/target/fidelity/` (changed pixels in red).

```sh
python fixtures/make_fixtures.py                              # rebuild the corpus (needs LibreOffice)
cargo run --release --bin fidelity -- --update-references     # only when replacing the LibreOffice baseline
cargo run --release --bin fidelity                            # convert, diff, and rewrite the table below
```

## Reference source

The references in this repository were exported by **LibreOffice**, through the same worker settings Lumen uses (tracked changes shown as final, macros off), not by Microsoft Office, because the development machine has no Office install. Until they're replaced, this table catches regressions in Lumen's pipeline (conversion settings, fonts, LibreOffice upgrades). It doesn't yet measure how close Lumen gets to what Word, Excel or PowerPoint show. To measure that, export each fixture to PDF from Microsoft Office (File → Save As → PDF), overwrite `fixtures/reference/<name>.pdf`, and run the command again. Tracked in the "Replace fidelity references with Microsoft Office exports" issue.

## Results

<!-- results -->
| Fixture | Pages (reference / Lumen) | Worst page mismatch | Result (≤ 2%) | Covers |
|---|---|---|---|---|
| tables.docx | 3 / 3 | 0.00% | pass | merged cells, nested table, repeated header rows, borders and shading |
| layout.docx | 3 / 3 | 0.00% | pass | two-column section, header/footer fields, footnote, endnote, text box, floating image |
| scripts.docx | 1 / 1 | 0.00% | pass | Arabic and Hebrew RTL, CJK, missing font, tracked changes shown as final |
| numbers.xlsx | 1 / 1 | 0.00% | pass | currency/percent/date formats, conditional formatting, merged title, print area |
| deck.pptx | 3 / 3 | 0.00% | pass | slide master, title and body placeholders, speaker notes |
| scanned.pdf | 1 / 1 | 0.00% | pass | image-only scanned page |
<!-- /results -->

## Not covered yet

Equations (OMML), charts, SmartArt and fonts embedded in the source document have no fixture yet, because the generator writes flat ODF and hand-built OOXML and can't author them. They're tracked in the "Fidelity fixtures for equations, charts, SmartArt and embedded fonts" issue.
