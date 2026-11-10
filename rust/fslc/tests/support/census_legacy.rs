// SPDX-License-Identifier: Apache-2.0

//! Frozen pre-#1411 extractors, used only to calibrate the negative controls.
//! Do not route a product census through these deliberately incomplete scans.

pub fn trace_product(source: &str) -> String {
    source
        .split("#[cfg(test)]")
        .next()
        .expect("product body")
        .lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn outcome_product(source: &str) -> &str {
    source.split("#[cfg(test)]").next().expect("product body")
}

pub fn helper_product(source: &str) -> Vec<(usize, String)> {
    let mut lines = Vec::new();
    let mut in_test_module = false;
    let mut previous_cfg_test = false;
    for (index, line) in source.lines().enumerate() {
        if in_test_module {
            in_test_module = line != "}";
            continue;
        }
        if previous_cfg_test && line.starts_with("mod ") && line.ends_with('{') {
            in_test_module = true;
            previous_cfg_test = false;
            continue;
        }
        previous_cfg_test = line == "#[cfg(test)]";
        let code = line.split("//").next().unwrap_or_default();
        lines.push((index + 1, code.to_owned()));
    }
    lines
}

#[derive(Clone, Copy, PartialEq)]
enum Lex {
    Code,
    LineComment,
    BlockComment(usize),
    Str,
    RawStr(usize),
}

// Exact lexer from issue_1284_naming_owner::non_test_lines at e64d3fb1.
#[allow(clippy::too_many_lines)]
pub fn naming_product(source: &str) -> Vec<String> {
    let chars = source.chars().collect::<Vec<_>>();
    let mut lines = vec![String::new()];
    let mut lex = Lex::Code;
    let mut depth = 0_usize;
    let mut skipping: Option<(usize, bool)> = None;
    let mut pending_test = false;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        let next = chars.get(index + 1).copied();
        if ch == '\n' {
            let finished = lines.last().expect("current line").trim().to_owned();
            if lex == Lex::LineComment {
                lex = Lex::Code;
            }
            if skipping.is_none()
                && finished.starts_with("#[cfg(")
                && finished.contains("test")
                && !finished.contains("not(test")
            {
                pending_test = true;
                lines.last_mut().expect("current line").clear();
            }
            lines.push(String::new());
            index += 1;
            continue;
        }
        if pending_test && lex == Lex::Code && !ch.is_whitespace() && ch != '#' {
            skipping = Some((depth, false));
            pending_test = false;
        }
        let emit = skipping.is_none();
        let mut push = |text: &str| {
            if emit {
                lines.last_mut().expect("current line").push_str(text);
            }
        };
        match lex {
            Lex::LineComment => {}
            Lex::BlockComment(level) => {
                if ch == '*' && next == Some('/') {
                    lex = if level == 1 {
                        Lex::Code
                    } else {
                        Lex::BlockComment(level - 1)
                    };
                    index += 1;
                } else if ch == '/' && next == Some('*') {
                    lex = Lex::BlockComment(level + 1);
                    index += 1;
                }
            }
            Lex::Str => {
                if ch == '\\' {
                    push(&ch.to_string());
                    if let Some(escaped) = next.filter(|escaped| *escaped != '\n') {
                        push(&escaped.to_string());
                        index += 1;
                    }
                } else {
                    push(&ch.to_string());
                    if ch == '"' {
                        lex = Lex::Code;
                    }
                }
            }
            Lex::RawStr(hashes) => {
                push(&ch.to_string());
                if ch == '"' && (1..=hashes).all(|offset| chars.get(index + offset) == Some(&'#')) {
                    for _ in 0..hashes {
                        push("#");
                    }
                    index += hashes;
                    lex = Lex::Code;
                }
            }
            Lex::Code => {
                if ch == '/' && next == Some('/') {
                    lex = Lex::LineComment;
                    index += 1;
                } else if ch == '/' && next == Some('*') {
                    lex = Lex::BlockComment(1);
                    index += 1;
                } else if ch == '"' {
                    push("\"");
                    lex = Lex::Str;
                } else if ch == 'r'
                    && matches!(next, Some('"' | '#'))
                    && !chars
                        .get(index.wrapping_sub(1))
                        .is_some_and(|previous| previous.is_alphanumeric() || *previous == '_')
                {
                    let mut cursor = index + 1;
                    while chars.get(cursor) == Some(&'#') {
                        cursor += 1;
                    }
                    if chars.get(cursor) == Some(&'"') {
                        let hashes = cursor - index - 1;
                        push(&chars[index..=cursor].iter().collect::<String>());
                        lex = Lex::RawStr(hashes);
                        index = cursor;
                    } else {
                        push("r");
                    }
                } else if ch == '\'' {
                    let end = if next == Some('\\') {
                        (index + 2..chars.len()).find(|&cursor| chars[cursor] == '\'')
                    } else {
                        (chars.get(index + 2) == Some(&'\'')).then_some(index + 2)
                    };
                    if let Some(end) = end {
                        push(&chars[index..=end].iter().collect::<String>());
                        index = end;
                    } else {
                        push("'");
                    }
                } else {
                    push(&ch.to_string());
                    match ch {
                        '{' => {
                            depth += 1;
                            if let Some((_, opened)) = skipping.as_mut() {
                                *opened = true;
                            }
                        }
                        '}' => {
                            depth = depth.saturating_sub(1);
                            if skipping.is_some_and(|(start, opened)| opened && depth == start) {
                                skipping = None;
                            }
                        }
                        ';' if skipping
                            .is_some_and(|(start, opened)| !opened && depth == start) =>
                        {
                            skipping = None;
                        }
                        _ => {}
                    }
                }
            }
        }
        index += 1;
    }
    lines
}
