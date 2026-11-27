//! A small interpreter for Excel number formats, enough for the grid view: currency, thousands
//! separators, fixed and optional decimals, percent, literal text, colour tags and the
//! positive;negative;zero sections. Anything it doesn't understand returns None, and the caller
//! falls back to plain number formatting rather than guessing.

const BACKSLASH: char = 92 as char;

#[derive(Debug, PartialEq)]
enum Tok {
    Lit(String),
    Zero,
    Hash,
    Dot,
    Comma,
    Percent,
}

/// Splits a format code into its sections at `;`, ignoring semicolons inside quotes or after a backslash.
fn sections(code: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut start, mut quoted, mut escaped) = (0, false, false);
    for (i, ch) in code.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == BACKSLASH {
            escaped = true;
        } else if ch == '"' {
            quoted = !quoted;
        } else if ch == ';' && !quoted {
            out.push(&code[start..i]);
            start = i + 1;
        }
    }
    out.push(&code[start..]);
    out
}

/// The colour a `[Red]`-style tag names, as a hex string.
fn tag_color(tag: &str) -> Option<&'static str> {
    Some(match tag.to_ascii_lowercase().as_str() {
        "red" => "#ff0000",
        "blue" => "#0000ff",
        "green" => "#00b050",
        "magenta" => "#ff00ff",
        "cyan" => "#00b0f0",
        "yellow" => "#ffc000",
        "white" => "#ffffff",
        "black" => "#000000",
        _ => return None,
    })
}

fn push_lit(toks: &mut Vec<Tok>, s: &str) {
    match toks.last_mut() {
        Some(Tok::Lit(l)) => l.push_str(s),
        _ => toks.push(Tok::Lit(s.to_owned())),
    }
}

/// Tokenizes one section. Returns None for features the grid doesn't render (conditions, dates,
/// scientific notation, fractions, text placeholders).
fn tokenize(section: &str) -> Option<(Vec<Tok>, Option<&'static str>)> {
    let mut toks = Vec::new();
    let mut color = None;
    let mut chars = section.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '0' => toks.push(Tok::Zero),
            '#' => toks.push(Tok::Hash),
            '.' => toks.push(Tok::Dot),
            ',' => toks.push(Tok::Comma),
            '%' => toks.push(Tok::Percent),
            '"' => {
                let s: String = chars.by_ref().take_while(|&c| c != '"').collect();
                push_lit(&mut toks, &s);
            }
            '_' => {
                chars.next();
                push_lit(&mut toks, " ");
            }
            '*' => {
                chars.next();
            }
            '[' => {
                let tag: String = chars.by_ref().take_while(|&c| c != ']').collect();
                if let Some(c) = tag_color(&tag) {
                    color = Some(c);
                } else {
                    let currency = tag.strip_prefix('$')?;
                    push_lit(&mut toks, currency.split('-').next().unwrap_or_default());
                }
            }
            '?' | 'E' | 'e' | '/' | '@' | 'y' | 'm' | 'd' | 'h' | 's' => return None,
            c if c == BACKSLASH => push_lit(&mut toks, &chars.next()?.to_string()),
            c => push_lit(&mut toks, &c.to_string()),
        }
    }
    toks.iter().any(|t| matches!(t, Tok::Zero | Tok::Hash)).then_some((toks, color))
}

fn group(int: &str) -> String {
    let mut out = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn push_affix(out: &mut String, toks: &[Tok]) {
    for t in toks {
        match t {
            Tok::Lit(l) => out.push_str(l),
            Tok::Percent => out.push('%'),
            _ => {}
        }
    }
}

/// Formats `value` with an Excel number format. Returns the text and an optional colour tag.
pub fn format(value: f64, code: &str) -> Option<(String, Option<&'static str>)> {
    let code = code.trim();
    if code.is_empty() || code.eq_ignore_ascii_case("general") {
        return None;
    }
    let secs = sections(code);
    let (section, abs, sign) = match secs.len() {
        n if n >= 3 && value == 0.0 => (secs[2], 0.0, ""),
        n if n >= 2 && value < 0.0 => (secs[1], -value, ""),
        _ if value < 0.0 => (secs[0], -value, "-"),
        _ => (secs[0], value, ""),
    };
    let (toks, color) = tokenize(section)?;
    let first = toks.iter().position(|t| matches!(t, Tok::Zero | Tok::Hash | Tok::Dot))?;
    let last = toks.iter().rposition(|t| matches!(t, Tok::Zero | Tok::Hash))?;
    let number = &toks[first..=last];
    let (int_part, frac_part) = match number.iter().position(|t| *t == Tok::Dot) {
        Some(d) => (&number[..d], &number[d + 1..]),
        None => (number, &number[..0]),
    };
    let percents = toks.iter().filter(|t| **t == Tok::Percent).count() as i32;
    let decimals = frac_part.iter().filter(|t| matches!(t, Tok::Zero | Tok::Hash)).count();
    let required = frac_part.iter().take_while(|t| **t == Tok::Zero).count();
    let min_int = int_part.iter().filter(|t| **t == Tok::Zero).count();
    let grouped = int_part.contains(&Tok::Comma);

    let scaled = abs * 100f64.powi(percents);
    let fixed = format!("{scaled:.decimals$}");
    let (int, frac) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let frac = format!("{:0<required$}", frac.trim_end_matches('0'));
    let int = if int == "0" && min_int == 0 { String::new() } else { format!("{int:0>min_int$}") };
    let int = if grouped { group(&int) } else { int };

    let mut out = String::from(sign);
    push_affix(&mut out, &toks[..first]);
    out.push_str(&int);
    if !frac.is_empty() {
        out.push('.');
        out.push_str(&frac);
    }
    push_affix(&mut out, &toks[last + 1..]);
    Some((out, color))
}

#[cfg(test)]
mod tests {
    use super::format;

    fn f(v: f64, code: &str) -> String {
        format(v, code).map(|(s, _)| s).unwrap_or_else(|| "GENERAL".into())
    }

    #[test]
    fn currency_grouping_and_decimals() {
        assert_eq!(f(125000.5, r##""$"#,##0.00"##), "$125,000.50");
        assert_eq!(f(1234567.0, "#,##0"), "1,234,567");
        assert_eq!(f(3.14159, "0.00"), "3.14");
        assert_eq!(f(0.5, "0.0#"), "0.5");
        assert_eq!(f(0.555, "0.0#"), "0.56");
        assert_eq!(f(1234.5, "[$€-407] #,##0.00"), "€ 1,234.50");
    }

    #[test]
    fn percent() {
        assert_eq!(f(0.124, "0.00%"), "12.40%");
        assert_eq!(f(-0.031, "0.0%"), "-3.1%");
        assert_eq!(f(1.0, "0%"), "100%");
    }

    #[test]
    fn sections_and_colours() {
        let code = "#,##0.00_);[Red](#,##0.00)";
        assert_eq!(format(-1500.0, code), Some(("(1,500.00)".into(), Some("#ff0000"))));
        assert_eq!(format(1500.0, code), Some(("1,500.00 ".into(), None)));
        assert_eq!(f(0.0, "0;-0;0.0"), "0.0");
        assert_eq!(f(-2.0, "0;0"), "2");
    }

    #[test]
    fn unsupported_formats_fall_back() {
        for code in ["General", "", "0.00E+00", "# ?/?", "[>100]0", "yyyy-mm-dd", "@"] {
            assert_eq!(f(12.0, code), "GENERAL", "{code}");
        }
    }
}
