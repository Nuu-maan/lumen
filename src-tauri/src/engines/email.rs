use super::markup;
use serde::Serialize;
use std::io::Read;
use std::path::Path;

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Email {
    pub subject: String,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub date: String,
    pub html: String,
    pub attachments: Vec<Attachment>,
}

#[derive(Serialize)]
pub struct Attachment {
    pub index: usize,
    pub name: String,
    pub size: usize,
}

pub struct Part {
    pub name: String,
    pub data: Vec<u8>,
}

const MAX_INLINE_IMAGE: usize = 8 << 20;

pub(crate) fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn with_inline_images(html: &str, inline: &[(String, String, &[u8])]) -> String {
    let mut out = html.to_owned();
    for (id, mime, data) in inline {
        if data.len() > MAX_INLINE_IMAGE || id.is_empty() {
            continue;
        }
        let uri = format!("data:{mime};base64,{}", base64(data));
        for quote in ['"', '\'', '('] {
            let close = if quote == '(' { ')' } else { quote };
            out = out.replace(&format!("{quote}{id}{close}"), &format!("{quote}{uri}{close}"));
        }
    }
    out
}

fn body_from(html: Option<String>, text: Option<String>) -> String {
    match (html, text) {
        (Some(h), _) if !h.trim().is_empty() => markup::sandboxed(&h),
        (_, Some(t)) => markup::plain(&t),
        _ => markup::plain(""),
    }
}

pub fn read(path: &Path, is_msg: bool) -> Result<(Email, Vec<Part>), String> {
    if is_msg {
        read_msg(path)
    } else {
        let raw = std::fs::read(path).map_err(|e| e.to_string())?;
        read_mime(&raw)
    }
}

fn read_mime(raw: &[u8]) -> Result<(Email, Vec<Part>), String> {
    let msg = mail_parser::MessageParser::default().parse(raw).ok_or("This message couldn't be parsed.")?;
    let list = |l: Option<mail_parser::AddressList>| {
        l.map(|l| {
            l.mailboxes()
                .map(|m| match (m.name(), m.address()) {
                    (Some(n), Some(a)) => format!("{n} <{a}>"),
                    (Some(n), None) => n.to_owned(),
                    (None, Some(a)) => a.to_owned(),
                    (None, None) => String::new(),
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
    };
    let mut inline = Vec::new();
    let mut parts = Vec::new();
    let mut attachments = Vec::new();
    let decoded: Vec<_> = msg
        .attachments()
        .map(|p| {
            let mime = p.content_type().map(|c| format!("{}/{}", c.ctype(), c.subtype().unwrap_or("")));
            let cid = p.content_id().map(|c| format!("cid:{}", c.trim_matches(['<', '>'])));
            (cid.or_else(|| p.content_location().map(str::to_owned)), mime, p.attachment_name().map(str::to_owned), p.decoded().into_owned())
        })
        .collect();
    for (i, (reference, mime, name, data)) in decoded.iter().enumerate() {
        if let (Some(reference), Some(mime)) = (reference, mime) {
            if mime.starts_with("image/") {
                inline.push((reference.clone(), mime.clone(), data.as_slice()));
                if name.is_none() {
                    continue;
                }
            }
        }
        let name = name.clone().unwrap_or_else(|| format!("attachment-{}", i + 1));
        attachments.push(Attachment { index: parts.len(), name: name.clone(), size: data.len() });
        parts.push(Part { name, data: data.clone() });
    }
    let html = msg.body_html(0).map(|h| with_inline_images(&h, &inline));
    let email = Email {
        subject: msg.subject().unwrap_or_default().to_owned(),
        from: list(msg.from()),
        to: list(msg.to()),
        cc: list(msg.cc()),
        date: msg.date().map(|d| d.to_rfc3339()).unwrap_or_default(),
        html: body_from(html, msg.body_text(0).map(|t| t.into_owned())),
        attachments,
    };
    Ok((email, parts))
}

fn read_msg(path: &Path) -> Result<(Email, Vec<Part>), String> {
    let mut comp = cfb::open(path).map_err(|e| e.to_string())?;
    let stream = |comp: &mut cfb::CompoundFile<std::fs::File>, name: &str| -> Option<Vec<u8>> {
        let mut buf = Vec::new();
        comp.open_stream(name).ok()?.read_to_end(&mut buf).ok()?;
        Some(buf)
    };
    let utf16 = |b: Vec<u8>| String::from_utf16_lossy(&b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()).trim_end_matches('\0').to_owned();
    let prop = |comp: &mut cfb::CompoundFile<std::fs::File>, root: &str, tag: &str| -> Option<String> {
        stream(comp, &format!("{root}/__substg1.0_{tag}001F"))
            .map(utf16)
            .or_else(|| stream(comp, &format!("{root}/__substg1.0_{tag}001E")).map(|b| String::from_utf8_lossy(&b).trim_end_matches('\0').to_owned()))
    };
    let subject = prop(&mut comp, "", "0037").unwrap_or_default();
    let sender = prop(&mut comp, "", "0C1A").unwrap_or_default();
    let sender_addr = prop(&mut comp, "", "0C1F").or_else(|| prop(&mut comp, "", "5D01")).unwrap_or_default();
    let to = prop(&mut comp, "", "0E04").unwrap_or_default();
    let cc = prop(&mut comp, "", "0E03").unwrap_or_default();
    let text = prop(&mut comp, "", "1000");
    let html = stream(&mut comp, "/__substg1.0_10130102").map(|b| String::from_utf8_lossy(&b).into_owned());
    let headers = prop(&mut comp, "", "007D").unwrap_or_default();
    let date = headers
        .lines()
        .find_map(|l| l.strip_prefix("Date:").map(|d| d.trim().to_owned()))
        .unwrap_or_default();

    let attach_dirs: Vec<String> = comp
        .read_root_storage()
        .filter(|e| e.is_storage() && e.name().starts_with("__attach_version1.0_"))
        .map(|e| e.path().to_string_lossy().replace('\\', "/"))
        .collect();
    let mut parts = Vec::new();
    let mut attachments = Vec::new();
    let mut inline = Vec::new();
    for dir in attach_dirs {
        let Some(data) = stream(&mut comp, &format!("{dir}/__substg1.0_37010102")) else { continue };
        let name = prop(&mut comp, &dir, "3707").or_else(|| prop(&mut comp, &dir, "3704")).unwrap_or_else(|| format!("attachment-{}", parts.len() + 1));
        let cid = prop(&mut comp, &dir, "3712");
        let mime = prop(&mut comp, &dir, "370E").unwrap_or_default();
        if let Some(cid) = cid.filter(|_| mime.starts_with("image/")) {
            inline.push((format!("cid:{cid}"), mime, data.clone()));
            continue;
        }
        attachments.push(Attachment { index: parts.len(), name: name.clone(), size: data.len() });
        parts.push(Part { name, data });
    }
    let inline_refs: Vec<_> = inline.iter().map(|(c, m, d)| (c.clone(), m.clone(), d.as_slice())).collect();
    let from = match (sender.is_empty(), sender_addr.is_empty()) {
        (false, false) if sender != sender_addr => format!("{sender} <{sender_addr}>"),
        (false, _) => sender,
        _ => sender_addr,
    };
    let email = Email {
        subject,
        from,
        to,
        cc,
        date,
        html: body_from(html.map(|h| with_inline_images(&h, &inline_refs)), text),
        attachments,
    };
    Ok((email, parts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn eml_with_html_and_attachment() {
        let raw = b"From: Ana <ana@example.com>\r\nTo: bo@example.com\r\nSubject: Report\r\nDate: Tue, 1 Jul 2025 10:00:00 +0000\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=XX\r\n\r\n--XX\r\nContent-Type: text/html\r\n\r\n<p>Hello <b>there</b></p>\r\n--XX\r\nContent-Type: text/plain\r\nContent-Disposition: attachment; filename=notes.txt\r\n\r\nsome notes\r\n--XX--\r\n";
        let (email, parts) = read_mime(raw).unwrap();
        assert_eq!(email.subject, "Report");
        assert_eq!(email.from, "Ana <ana@example.com>");
        assert!(email.html.contains("<b>there</b>"));
        assert!(email.html.contains("Content-Security-Policy"));
        assert_eq!(email.attachments.len(), 1);
        assert_eq!(parts[0].name, "notes.txt");
    }
}
