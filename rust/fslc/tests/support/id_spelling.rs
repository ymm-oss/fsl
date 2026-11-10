// SPDX-License-Identifier: Apache-2.0

/// Every `A-B-1` run the source spells without spaces (requirement,
/// acceptance, forbidden, scenario, rule, control, policy and goal IDs and
/// their `covers` / `satisfies` / `owns` / `require` references), in order.
pub fn compact_hyphen_runs(source: &str) -> Vec<String> {
    let tokens = fsl_syntax::lex(source).expect("lex");
    let component = |index: usize| {
        tokens.get(index).is_some_and(|token| {
            matches!(
                token.kind,
                fsl_syntax::TokenKind::Ident(_) | fsl_syntax::TokenKind::Int(_)
            )
        })
    };
    let touching =
        |left: usize, right: usize| tokens[left].span.end.offset == tokens[right].span.start.offset;
    let mut runs = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let mut end = index;
        while component(end)
            && matches!(tokens.get(end + 1).map(|token| &token.kind),
                Some(fsl_syntax::TokenKind::Symbol(symbol)) if symbol == "-")
            && component(end + 2)
            && touching(end, end + 1)
            && touching(end + 1, end + 2)
        {
            end += 2;
        }
        if end > index {
            runs.push(
                source[tokens[index].span.start.offset..tokens[end].span.end.offset].to_owned(),
            );
        }
        index = end + 1;
    }
    runs
}
