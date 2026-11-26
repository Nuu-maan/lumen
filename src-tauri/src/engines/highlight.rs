use std::sync::LazyLock;
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxSet};

pub const MAX_BYTES: usize = 2 << 20;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

const CLASSES: &[(&str, u8)] = &[
    ("comment", 1),
    ("string", 2),
    ("constant.numeric", 3),
    ("constant", 4),
    ("keyword", 5),
    ("storage.type.function", 5),
    ("storage.type", 7),
    ("storage", 5),
    ("entity.name.function", 6),
    ("support.function", 6),
    ("meta.function-call", 6),
    ("entity.name.type", 7),
    ("entity.name.class", 7),
    ("support.type", 7),
    ("support.class", 7),
    ("entity.name.tag", 8),
    ("entity.other.attribute-name", 9),
    ("variable.parameter", 10),
    ("punctuation.definition.string", 2),
    ("punctuation.definition.comment", 1),
];

fn class_of(stack: &ScopeStack, cache: &mut Vec<(Scope, u8)>) -> u8 {
    for scope in stack.as_slice().iter().rev() {
        let class = match cache.iter().find(|(s, _)| s == scope) {
            Some(&(_, c)) => c,
            None => {
                let name = scope.build_string();
                let c = CLASSES.iter().find(|(prefix, _)| name.starts_with(prefix)).map_or(0, |&(_, c)| c);
                cache.push((*scope, c));
                c
            }
        };
        if class != 0 {
            return class;
        }
    }
    0
}

pub type LineTokens = Vec<(u32, u8)>;

pub fn highlight(text: &str, extension: &str, first_line: &str) -> Option<Vec<LineTokens>> {
    let set = &*SYNTAXES;
    let syntax = set.find_syntax_by_extension(extension).or_else(|| set.find_syntax_by_first_line(first_line))?;
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut cache = Vec::new();
    let mut out = Vec::new();
    for line in text.split_inclusive('\n') {
        let ops = state.parse_line(line, set).ok()?;
        let end = line.trim_end_matches(['\n', '\r']).len();
        let mut tokens: LineTokens = Vec::new();
        let mut push = |from: usize, to: usize, class: u8| {
            let chars = line.get(from.min(end)..to.min(end)).map_or(0, |s| s.encode_utf16().count()) as u32;
            match tokens.last_mut() {
                _ if chars == 0 => {}
                Some(last) if last.1 == class => last.0 += chars,
                _ => tokens.push((chars, class)),
            }
        };
        let mut pos = 0;
        for (at, op) in ops {
            push(pos, at, class_of(&stack, &mut cache));
            pos = at;
            let _ = stack.apply(&op);
        }
        push(pos, end, class_of(&stack, &mut cache));
        out.push(tokens);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_keywords_strings_and_comments_get_classes() {
        let lines = highlight("fn main() {\n    let s = \"hi\"; // note\n}\n", "rs", "").unwrap();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].iter().any(|&(_, c)| c == 5));
        assert!(lines[1].iter().any(|&(_, c)| c == 2));
        assert!(lines[1].iter().any(|&(_, c)| c == 1));
        let total: u32 = lines[1].iter().map(|t| t.0).sum();
        assert_eq!(total as usize, "    let s = \"hi\"; // note".chars().count());
    }

    #[test]
    fn unknown_extension_without_shebang_is_not_highlighted() {
        assert!(highlight("plain words", "zzz", "plain words").is_none());
    }
}
