// SPDX-License-Identifier: Apache-2.0

//! Escaping for external data slots, not for generated Markdown or claim grammar.

fn autolink_boundary(text: &str, index: usize, ch: char) -> Option<&'static str> {
    let before = &text[..index];
    let after = &text[index + ch.len_utf8()..];
    match ch {
        ':' if after.starts_with("//")
            && ["http", "https", "ftp"].iter().any(|scheme| {
                before
                    .get(before.len().saturating_sub(scheme.len())..)
                    .is_some_and(|tail| tail.eq_ignore_ascii_case(scheme))
            }) =>
        {
            Some("&#58;")
        }
        '.' if before
            .get(before.len().saturating_sub(3)..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case("www")) =>
        {
            Some("&#46;")
        }
        '@' if before.chars().next_back().is_some_and(|ch| {
            ch.is_ascii_alphanumeric() || ".!#$%&'*+/=?^_`{|}~-".contains(ch)
        }) && after
            .split(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, '.' | '-' | '_'))
            .next()
            .is_some_and(|domain| domain.contains('.') && !domain.starts_with('.')) =>
        {
            // A constant HTML boundary prevents GFM's email autolink extension;
            // entity/backslash @ still autolinks. Input HTML is never trusted.
            Some("<span>@</span>")
        }
        _ => None,
    }
}

pub(crate) fn prose(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    // Intraword underscores in canonical identifiers are not emphasis.
    let identifier = !text.ends_with('_')
        && text
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '.'));
    for (index, ch) in text.char_indices() {
        if let Some(boundary) = autolink_boundary(text, index, ch) {
            out.push_str(boundary);
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' if text[..index].ends_with("--") => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            '\n' => out.push_str("&#10;"),
            '_' if !identifier => out.push_str("\\_"),
            '\\' | '`' | '*' | '[' | ']' | '|' | '~' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

pub(crate) fn blockquote(text: &str) -> String {
    text.split('\n')
        .map(|line| {
            // Tab stops are physical columns after the emitted `> ` prefix.
            let column = line
                .chars()
                .take_while(|ch| matches!(ch, ' ' | '\t'))
                .fold(2, |column, ch| {
                    column + if ch == '\t' { 4 - column % 4 } else { 1 }
                });
            if column >= 6 {
                let literal = code(line);
                if literal.starts_with('`')
                    && line.starts_with(' ')
                    && line.ends_with(' ')
                    && line.bytes().any(|ch| ch != b' ')
                {
                    return format!("> ` {line} `");
                }
                return format!("> {literal}");
            }
            let escaped = prose(line);
            // Contain every line, including blank lines; prevent headings/lists
            // within the quotation from being mistaken for generated structure.
            let first = escaped.trim_start();
            let prefix = &escaped[..escaped.len() - first.len()];
            let digits = first.bytes().take_while(u8::is_ascii_digit).count();
            if (1..=9).contains(&digits)
                && first[digits..].starts_with(['.', ')'])
                && (first[digits + 1..].is_empty() || first[digits + 1..].starts_with([' ', '\t']))
            {
                return format!("> {prefix}{}\\{}", &first[..digits], &first[digits..]);
            }
            if first.starts_with(['#', '-', '+', '>']) {
                format!("> {prefix}\\{first}")
            } else {
                format!("> {escaped}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn code(text: &str) -> String {
    if !text.contains(['`', '|', '\r', '\n', '<', '>', '&']) {
        return format!("`{text}`");
    }
    let mut out = String::from("<code>");
    for (index, ch) in text.char_indices() {
        if let Some(boundary) = autolink_boundary(text, index, ch) {
            out.push_str(boundary);
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '`' => out.push_str("&#96;"),
            '|' => out.push_str("&#124;"),
            '*' => out.push_str("&#42;"),
            '_' => out.push_str("&#95;"),
            '[' => out.push_str("&#91;"),
            ']' => out.push_str("&#93;"),
            '!' if text[index + 1..].starts_with('[') => out.push_str("&#33;"),
            '\\' => out.push_str("&#92;"),
            '~' => out.push_str("&#126;"),
            '\r' => out.push_str("&#13;"),
            '\n' => out.push_str("&#10;"),
            _ => out.push(ch),
        }
    }
    out.push_str("</code>");
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn multiline_quote_is_contained_literal_data() {
        assert_eq!(
            super::blockquote("<!--\n\n# title\r\n- item\n```|"),
            "> &lt;!--\n> \n> \\# title&#13;\n> \\- item\n> \\`\\`\\`\\|"
        );
    }
}
