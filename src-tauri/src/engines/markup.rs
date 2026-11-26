const CSP: &str = "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data:; style-src 'unsafe-inline'; font-src data:\">";

const READER_CSS: &str = "<style>
:root{color-scheme:light}
body{margin:0;padding:48px 56px 96px;background:#fff;color:#1f2328;font:16px/1.6 system-ui,'Segoe UI',-apple-system,sans-serif;overflow-wrap:break-word}
main{max-width:760px;margin:0 auto}
h1,h2{border-bottom:1px solid #d8dee4;padding-bottom:.3em}
h1,h2,h3,h4{line-height:1.25;margin:1.5em 0 .6em}
code,pre{font-family:ui-monospace,Consolas,'SF Mono',monospace;font-size:.88em}
code{background:#eff1f3;padding:.15em .35em;border-radius:4px}
pre{background:#f6f8fa;padding:14px 16px;border-radius:6px;overflow:auto;line-height:1.45}
pre code{background:none;padding:0}
blockquote{margin:0;padding:0 1em;color:#59636e;border-left:.25em solid #d1d9e0}
table{border-collapse:collapse;display:block;overflow:auto}
th,td{border:1px solid #d1d9e0;padding:6px 13px}
tr:nth-child(2n){background:#f6f8fa}
img{max-width:100%}
a{color:#0969da}
hr{border:0;border-top:1px solid #d1d9e0;margin:24px 0}
</style>";

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 16);
    for c in text.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

pub fn sandboxed(html: &str) -> String {
    let base = "<base target=\"_blank\">";
    match html.find(|c: char| !c.is_whitespace() && c != '\u{feff}') {
        Some(i) if html[i..].to_ascii_lowercase().starts_with("<!doctype") => {
            let end = html[i..].find('>').map(|e| i + e + 1).unwrap_or(i);
            format!("{}{CSP}{base}{}", &html[..end], &html[end..])
        }
        _ => format!("{CSP}{base}{html}"),
    }
}

pub fn markdown(source: &str) -> String {
    use pulldown_cmark::{html, Options, Parser};
    let parser = Parser::new_ext(
        source,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_FOOTNOTES,
    );
    let mut body = String::with_capacity(source.len() * 3 / 2);
    html::push_html(&mut body, parser);
    sandboxed(&format!("<!doctype html><html><head><meta charset=\"utf-8\">{READER_CSS}</head><body><main>{body}</main></body></html>"))
}

pub fn plain(text: &str) -> String {
    sandboxed(&format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">{READER_CSS}</head><body><main><pre style=\"white-space:pre-wrap;background:none;padding:0;font-size:14px\">{}</pre></main></body></html>",
        escape(text)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csp_goes_first_even_after_doctype() {
        let out = sandboxed("<!DOCTYPE html><html><script>x()</script>");
        assert!(out.starts_with("<!DOCTYPE html><meta http-equiv=\"Content-Security-Policy\""));
        let out = sandboxed("<p>hi</p>");
        assert!(out.starts_with(CSP));
    }

    #[test]
    fn markdown_renders_and_escapes() {
        let html = markdown("# Title\n\n| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<table>"));
        assert_eq!(escape("<a href='x'>&</a>"), "&lt;a href=&#39;x&#39;&gt;&amp;&lt;/a&gt;");
    }
}
