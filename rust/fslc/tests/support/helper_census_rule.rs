// SPDX-License-Identifier: Apache-2.0

//! The #1171 crate-root dependency rule.

#[path = "product_source.rs"]
mod product_source;

/// Product tokens with comments and direct test-only items blanked.
fn product_lines(source: &str) -> Vec<(usize, String)> {
    let wrapped;
    let (source, skip) = if syn::parse_file(source).is_ok() {
        (source, 0)
    } else {
        // Any unparseable input takes this fallback, not just fixtures. It
        // supports statement fragments, including controls without terminators.
        let statements = source
            .lines()
            .map(|line| {
                if line.trim_start().starts_with("crate::") && !line.ends_with(';') {
                    format!("{line};")
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        wrapped = format!("fn census_fixture() {{\n{statements}\n}}");
        (wrapped.as_str(), 1)
    };
    product_source::product_source(source)
        .lines()
        .skip(skip)
        .enumerate()
        .map(|(index, line)| (index + 1, line.to_owned()))
        .collect()
}

/// Every path in `source` that reaches an item of the crate root.
pub(crate) fn root_dependencies(source: &str, modules: &[String]) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (line_number, line) in product_lines(source) {
        let line = line.replace("pub(super)", "");
        for (offset, _) in line.match_indices("super::") {
            let preceded_by_ident = line[..offset]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
            if !preceded_by_ident {
                found.push((line_number, line[offset..].trim().to_owned()));
            }
        }
        for (offset, _) in line.match_indices("crate::") {
            let preceded_by_ident = line[..offset]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
            if preceded_by_ident {
                continue;
            }
            let rest = &line[offset + "crate::".len()..];
            let segment = rest
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .next()
                .unwrap_or_default();
            let names_a_module = modules.iter().any(|module| module == segment)
                && rest[segment.len()..].starts_with("::");
            if !names_a_module {
                found.push((line_number, line[offset..].trim().to_owned()));
            }
        }
    }
    found
}
