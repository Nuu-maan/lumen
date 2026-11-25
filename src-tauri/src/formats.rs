use serde::Serialize;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Mupdf,
    Office,
    Sheet,
    Text,
    Markdown,
    Image,
    Html,
    Email,
    Archive,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Document,
    Spreadsheet,
    Presentation,
    Drawing,
    Book,
    Image,
    Text,
    Other,
}

#[derive(Debug, Serialize)]
pub struct Format {
    pub id: &'static str,
    pub label: &'static str,
    pub exts: &'static [&'static str],
    pub engine: Engine,
    pub family: Family,
}

macro_rules! formats {
    ($($id:literal, $label:literal, [$($ext:literal),*], $engine:ident, $family:ident;)*) => {
        pub static FORMATS: &[Format] = &[
            $(Format { id: $id, label: $label, exts: &[$($ext),*], engine: Engine::$engine, family: Family::$family },)*
        ];
    };
}

formats! {
    "pdf", "PDF", ["pdf"], Mupdf, Document;
    "xps", "XPS", ["xps", "oxps"], Mupdf, Document;
    "epub", "EPUB", ["epub"], Mupdf, Book;
    "mobi", "MOBI", ["mobi", "azw", "azw3", "prc"], Mupdf, Book;
    "fb2", "FictionBook", ["fb2"], Mupdf, Book;
    "cbz", "Comic book", ["cbz"], Mupdf, Book;
    "tiff", "TIFF image", ["tif", "tiff"], Mupdf, Image;
    "docx", "Word document", ["docx", "docm", "dotx", "dotm"], Office, Document;
    "doc", "Word 97-2003 document", ["doc", "dot"], Office, Document;
    "rtf", "Rich Text", ["rtf"], Office, Document;
    "odt", "OpenDocument text", ["odt", "ott", "fodt"], Office, Document;
    "pages", "Pages document", ["pages"], Office, Document;
    "wpd", "WordPerfect document", ["wpd", "wp", "wp5", "wp6"], Office, Document;
    "hwp", "Hangul document", ["hwp"], Office, Document;
    "xlsx", "Excel workbook", ["xlsx", "xlsm", "xltx", "xltm"], Office, Spreadsheet;
    "xlsb", "Excel binary workbook", ["xlsb"], Office, Spreadsheet;
    "xls", "Excel 97-2003 workbook", ["xls", "xlt"], Office, Spreadsheet;
    "ods", "OpenDocument spreadsheet", ["ods", "ots", "fods"], Office, Spreadsheet;
    "numbers", "Numbers spreadsheet", ["numbers"], Office, Spreadsheet;
    "pptx", "PowerPoint presentation", ["pptx", "ppsx", "pptm", "potx", "ppsm"], Office, Presentation;
    "ppt", "PowerPoint 97-2003 presentation", ["ppt", "pps", "pot"], Office, Presentation;
    "odp", "OpenDocument presentation", ["odp", "otp", "fodp"], Office, Presentation;
    "key", "Keynote presentation", ["key"], Office, Presentation;
    "vsdx", "Visio drawing", ["vsdx", "vsdm"], Office, Drawing;
    "vsd", "Visio 2003 drawing", ["vsd"], Office, Drawing;
    "pub", "Publisher document", ["pub"], Office, Document;
    "csv", "CSV", ["csv"], Sheet, Spreadsheet;
    "tsv", "TSV", ["tsv", "tab"], Sheet, Spreadsheet;
    "md", "Markdown", ["md", "markdown", "mdown", "mkd"], Markdown, Text;
    "json", "JSON", ["json", "jsonc", "json5", "geojson", "ipynb"], Text, Text;
    "xml", "XML", ["xml", "xsd", "xsl", "xslt", "plist", "rss", "atom", "csproj", "props"], Text, Text;
    "yaml", "YAML", ["yaml", "yml"], Text, Text;
    "log", "Log file", ["log"], Text, Text;
    "code", "Source code", [
        "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "rb", "go", "java", "kt", "kts", "swift", "c", "h",
        "cc", "cpp", "cxx", "hpp", "hh", "cs", "fs", "php", "pl", "lua", "r", "scala", "sh", "bash", "zsh",
        "ps1", "bat", "cmd", "sql", "css", "scss", "less", "svelte", "vue", "toml", "ini", "cfg", "conf",
        "dockerfile", "makefile", "cmake", "gradle", "tex", "hs", "ml", "clj", "ex", "exs", "erl", "dart",
        "zig", "nim", "jl", "m", "mm", "d", "groovy", "diff", "patch", "proto", "graphql", "tf"
    ], Text, Text;
    "txt", "Plain text", ["txt", "text", "nfo", "readme", "me", "srt", "vtt"], Text, Text;
    "html", "HTML", ["html", "htm", "xhtml"], Html, Document;
    "mht", "Web archive", ["mht", "mhtml"], Email, Document;
    "eml", "Email message", ["eml"], Email, Document;
    "msg", "Outlook message", ["msg"], Email, Document;
    "png", "PNG image", ["png", "apng"], Image, Image;
    "jpeg", "JPEG image", ["jpg", "jpeg", "jfif", "jpe"], Image, Image;
    "gif", "GIF image", ["gif"], Image, Image;
    "webp", "WebP image", ["webp"], Image, Image;
    "avif", "AVIF image", ["avif"], Image, Image;
    "heic", "HEIC image", ["heic", "heif"], Image, Image;
    "bmp", "Bitmap image", ["bmp", "dib"], Image, Image;
    "ico", "Icon", ["ico", "cur"], Image, Image;
    "svg", "SVG image", ["svg"], Image, Image;
    "zip", "ZIP archive", ["zip", "jar", "apk", "nupkg", "whl"], Archive, Other;
}

pub fn by_id(id: &str) -> &'static Format {
    FORMATS.iter().find(|f| f.id == id).unwrap_or_else(|| panic!("unknown format id {id}"))
}

pub fn by_ext(ext: &str) -> Option<&'static Format> {
    let ext = ext.to_ascii_lowercase();
    FORMATS.iter().find(|f| f.exts.contains(&ext.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_extensions_are_unique() {
        let mut ids = std::collections::HashSet::new();
        let mut exts = std::collections::HashSet::new();
        for f in FORMATS {
            assert!(ids.insert(f.id), "duplicate id {}", f.id);
            for e in f.exts {
                assert!(exts.insert(*e), "duplicate extension {e}");
            }
        }
    }
}
