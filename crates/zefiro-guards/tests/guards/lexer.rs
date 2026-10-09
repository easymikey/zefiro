// GUARD: a small Rust tokenizer and item pass for the conventions guard.

pub(crate) const TEST: u8 = 1;
pub(crate) const WIDGET: u8 = 2;
pub(crate) const IMPL: u8 = 4;
pub(crate) const TRANSITION: u8 = 8;

const WIDGET_TRAITS: &[&str] =
    &["Widget", "StatefulWidget", "WidgetRef", "StatefulWidgetRef"];

pub(crate) struct Token {
    pub(crate) text: String,
    pub(crate) line: usize,
}

pub(crate) struct Item {
    pub(crate) kind: String,
    pub(crate) name: String,
    pub(crate) via: String,
    pub(crate) at: usize,
    pub(crate) head: usize,
    pub(crate) line: usize,
    pub(crate) scope: u8,
}

impl Item {
    pub(crate) fn has(&self, flag: u8) -> bool {
        self.scope & flag != 0
    }

    pub(crate) fn is_type(&self) -> bool {
        matches!(self.kind.as_str(), "struct" | "enum" | "trait" | "type")
    }
}

pub(crate) struct File {
    pub(crate) path: String,
    pub(crate) tokens: Vec<Token>,
    pub(crate) items: Vec<Item>,
}

impl File {
    pub(crate) fn parse(path: &str, content: &str) -> Self {
        let tokens = tokenize(content);
        let items = items(&tokens, path);
        Self {
            path: path.to_owned(),
            tokens,
            items,
        }
    }

    pub(crate) fn tx(&self, index: usize) -> &str {
        text_at(&self.tokens, index)
    }

    pub(crate) fn matching_close(&self, open: usize) -> usize {
        let mut depth = 0_i32;
        let mut closed = |at: usize| {
            depth += match self.tx(at) {
                "(" | "[" | "{" => 1,
                ")" | "]" | "}" => -1,
                _ => 0,
            };
            depth == 0
        };
        (open..self.tokens.len())
            .find(|at| closed(*at))
            .unwrap_or(self.tokens.len())
    }

    pub(crate) fn test_tokens(&self) -> Vec<bool> {
        let base = self.path.contains("test_support");
        let (mut stack, mut head) = (Vec::new(), 0);
        let mut out = Vec::with_capacity(self.tokens.len());
        for (at, token) in self.tokens.iter().enumerate() {
            out.push(base || stack.contains(&true));
            match token.text.as_str() {
                "{" => {
                    let header = &self.tokens[head..at];
                    let tests = header.iter().any(|word| word.text == "tests");
                    stack.push(tests || header_scope(header) & TEST != 0);
                }
                "}" => {
                    stack.pop();
                }
                ";" => {}
                _ => continue,
            }
            head = at + 1;
        }
        out
    }
}

pub(crate) fn is_word(text: &str) -> bool {
    text.starts_with(|first: char| first.is_alphabetic() || first == '_')
}

fn text_at(tokens: &[Token], index: usize) -> &str {
    tokens.get(index).map_or("", |token| token.text.as_str())
}

fn alnum(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn block_end(c: &[char], from: usize) -> usize {
    let (mut depth, mut at) = (0_i32, from);
    while at < c.len() {
        let step = match (c[at], c.get(at + 1)) {
            ('/', Some('*')) => 1,
            ('*', Some('/')) => -1,
            _ => 0,
        };
        depth += step;
        at += 1 + usize::from(step != 0);
        if depth == 0 {
            break;
        }
    }
    at
}

fn string_end(c: &[char], from: usize) -> Option<usize> {
    let mut at = from + usize::from(c[from] == 'b');
    let raw = c.get(at) == Some(&'r');
    let mut hashes = 0;
    while raw && c.get(at + 1 + hashes) == Some(&'#') {
        hashes += 1;
    }
    at += usize::from(raw) + hashes;
    if c.get(at) != Some(&'"') {
        return None;
    }
    at += 1;
    while at < c.len() {
        let closing = c[at + 1..].iter().take(hashes).all(|x| *x == '#');
        match c[at] {
            '\\' if !raw => at += 1,
            '"' if closing => return Some(at + 1 + hashes),
            _ => {}
        }
        at += 1;
    }
    Some(c.len())
}

fn lex_at(c: &[char], from: usize) -> (usize, Option<String>) {
    let (ch, next) = (c[from], c.get(from + 1).copied().unwrap_or('\0'));
    let scan = |start: usize, keep: &dyn Fn(char) -> bool| {
        (start..c.len()).find(|at| !keep(c[*at])).unwrap_or(c.len())
    };
    if ch.is_whitespace() {
        (from + 1, None)
    } else if ch == '/' && next == '/' {
        (scan(from, &|x| x != '\n'), None)
    } else if ch == '/' && next == '*' {
        (block_end(c, from), None)
    } else if let Some(end) = string_end(c, from) {
        (end, Some("\"\"".to_owned()))
    } else if ch == '\'' && next == '\\' {
        (scan(from + 3, &|x| x != '\'') + 1, None)
    } else if ch == '\'' && c.get(from + 2) == Some(&'\'') {
        (from + 3, None)
    } else if ch == '\'' || alnum(ch) {
        let end = scan(from + 1, &alnum);
        (end, Some(c[from..end].iter().collect()))
    } else {
        (from + 1, Some(ch.to_string()))
    }
}

fn tokenize(source: &str) -> Vec<Token> {
    let c: Vec<char> = source.chars().collect();
    let (mut from, mut line, mut out) = (0, 1, Vec::new());
    while from < c.len() {
        let (end, text) = lex_at(&c, from);
        let end = end.min(c.len());
        out.extend(text.map(|text| Token { text, line }));
        line += c[from..end].iter().filter(|x| **x == '\n').count();
        from = end;
    }
    out
}

fn header_scope(header: &[Token]) -> u8 {
    let words: Vec<&str> = header.iter().map(|token| token.text.as_str()).collect();
    let test = if words.contains(&"test") { TEST } else { 0 };
    let Some(at) = words
        .iter()
        .position(|word| matches!(*word, "impl" | "trait" | "fn"))
    else {
        return test;
    };
    let rest = &words[at..];
    let widget = rest
        .iter()
        .position(|word| *word == "for")
        .is_some_and(|stop| {
            rest[..stop].iter().any(|word| WIDGET_TRAITS.contains(word))
        });
    test | match rest[0] {
        "impl" if widget => WIDGET | IMPL,
        "impl" | "trait" => IMPL,
        _ if rest.get(1) == Some(&"transition") => TRANSITION,
        _ => 0,
    }
}

fn target_after(tokens: &[Token], from: usize) -> &str {
    let mut at = from;
    while matches!(text_at(tokens, at), "&" | "mut" | "dyn")
        || text_at(tokens, at).starts_with('\'')
    {
        at += 1;
    }
    let mut name = text_at(tokens, at);
    while text_at(tokens, at + 1) == ":" && text_at(tokens, at + 2) == ":" {
        at += 3;
        name = text_at(tokens, at);
    }
    name
}

fn impl_pair(tokens: &[Token], from: usize) -> Option<(&str, &str)> {
    let (mut depth, mut via, mut at) = (0_i32, "", from + 1);
    loop {
        match text_at(tokens, at) {
            "" | "{" | ";" | "where" => return None,
            "<" => depth += 1,
            ">" if text_at(tokens, at - 1) != "-" => depth -= 1,
            "for" if depth == 0 && text_at(tokens, at + 1) != "<" => {
                return Some((via, target_after(tokens, at + 1)));
            }
            word if depth == 0 && is_word(word) => via = word,
            _ => {}
        }
        at += 1;
    }
}

fn takes_self_field(tokens: &[Token], at: usize) -> bool {
    let call = ["(", "&", "mut", "self", "."]
        .iter()
        .enumerate()
        .all(|(offset, text)| text_at(tokens, at + 1 + offset) == *text);
    call && match text_at(tokens, at.wrapping_sub(1)) {
        ":" => text_at(tokens, at.wrapping_sub(3)) == "mem",
        "." => false,
        _ => true,
    }
}

fn item_at(tokens: &[Token], at: usize) -> Option<Item> {
    let (kind, name, via) = match text_at(tokens, at) {
        "struct" | "enum" | "trait" | "type" | "fn"
            if is_word(text_at(tokens, at + 1)) =>
        {
            (text_at(tokens, at), text_at(tokens, at + 1), "")
        }
        "impl" => {
            let (via, target) = impl_pair(tokens, at)?;
            ("impl", target, via)
        }
        "take" if takes_self_field(tokens, at) => ("take", text_at(tokens, at + 6), ""),
        _ => return None,
    };
    Some(Item {
        kind: kind.to_owned(),
        name: name.to_owned(),
        via: via.to_owned(),
        at,
        head: 0,
        line: tokens[at].line,
        scope: 0,
    })
}

fn items(tokens: &[Token], path: &str) -> Vec<Item> {
    let base = if path.contains("test_support") {
        TEST
    } else {
        0
    };
    let (mut stack, mut head, mut out) = (Vec::new(), 0, Vec::new());
    for (at, token) in tokens.iter().enumerate() {
        let scope = stack.iter().fold(base, |all, one| all | one);
        match token.text.as_str() {
            "{" => stack.push(header_scope(&tokens[head..at])),
            "}" => {
                stack.pop();
            }
            ";" => {}
            _ => {
                let found = item_at(tokens, at);
                let wanted =
                    found.filter(|item| item.kind != "take" || scope & TRANSITION != 0);
                out.extend(wanted.map(|item| Item {
                    head,
                    scope,
                    ..item
                }));
                continue;
            }
        }
        head = at + 1;
    }
    out
}
