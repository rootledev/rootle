//! Span-preserving qualifier lexer. Semantics follow strop-picker's query
//! lexer (not strop-grammar's regex dialect): both quotes, only active-quote
//! and backslash escapes inside quotes, and quoted words stay literal.

use std::ops::Range;

#[derive(Debug)]
pub(super) struct Token {
    pub range: Range<usize>,
    pub text: String,
    pub quoted: bool,
    pub complete: bool,
}

pub(super) fn lex(input: &str) -> Vec<Token> {
    let mut chars = input.char_indices().peekable();
    let mut tokens = Vec::new();
    while let Some((start, first)) = chars.next() {
        if first.is_whitespace() {
            continue;
        }
        let mut token = Token {
            range: start..start + first.len_utf8(),
            text: String::new(),
            quoted: matches!(first, '\'' | '"'),
            complete: true,
        };
        let mut current = Some((start, first));
        let mut quote = None;
        while let Some((at, c)) = current {
            token.range.end = at + c.len_utf8();
            if let Some(active) = quote {
                if c == active {
                    quote = None;
                } else if c == '\\'
                    && chars
                        .peek()
                        .is_some_and(|(_, next)| *next == active || *next == '\\')
                {
                    let (end, escaped) = chars.next().expect("peeked escape");
                    token.text.push(escaped);
                    token.range.end = end + escaped.len_utf8();
                } else {
                    token.text.push(c);
                }
            } else if matches!(c, '\'' | '"') {
                quote = Some(c);
            } else {
                token.text.push(c);
            }
            if quote.is_none() && chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
                break;
            }
            current = chars.next();
        }
        token.complete = quote.is_none();
        tokens.push(token);
    }
    tokens
}

pub(super) fn qualifier(text: &str) -> Option<(&str, &str)> {
    let (key, value) = text.split_once(':')?;
    // Known keys keep their value verbatim; unknown-key diagnostics exclude
    // URLs, drive prefixes and C++ namespace literals, as strop does.
    (known(key)
        || (key.len() >= 3
            && key.chars().all(|c| c.is_ascii_alphabetic())
            && !value.starts_with("//")
            && !value.starts_with(':')))
    .then_some((key, value))
}

pub(super) fn known(key: &str) -> bool {
    matches!(
        key,
        "language"
            | "extension"
            | "path"
            | "repo"
            | "org"
            | "fpath"
            | "glob"
            | "hidden"
            | "ignored"
            | "case"
            | "text"
            | "regex"
            | "kind"
            | "type"
    )
}

/// Filename searches retain qualifier tokens verbatim; only free terms receive
/// the provider's path qualifier. This avoids `path:language:cpp` corruption.
pub(super) fn file_query(input: &str) -> String {
    let tokens = lex(input);
    let mut out = String::with_capacity(input.len() + tokens.len() * 5);
    let mut end = 0;
    for token in tokens {
        out.push_str(&input[end..token.range.start]);
        let raw = &input[token.range.clone()];
        let body = token.text.strip_prefix('-').unwrap_or(&token.text);
        if !token.quoted && (token.text == "NOT" || qualifier(body).is_some()) {
            out.push_str(raw);
        } else if !token.quoted && raw.starts_with('-') {
            out.push_str("-path:");
            out.push_str(&raw[1..]);
        } else {
            out.push_str("path:");
            out.push_str(raw);
        }
        end = token.range.end;
    }
    out.push_str(&input[end..]);
    out
}
