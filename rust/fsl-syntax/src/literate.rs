// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

/// Extract FSL source from a Markdown document by in-place blanking.
///
/// Lines outside ` ```fsl ` fenced code blocks are replaced with empty lines so
/// that byte offsets, line numbers, and column positions in the blanked output
/// correspond 1:1 to the original document.  Multiple fsl blocks are treated as
/// one compilation unit (definitions can be split across sections).
///
/// Fence detection follows the `CommonMark` fenced-code-block grammar: an opening
/// fence is a line whose trimmed form starts with a run of three or more
/// backticks or tildes; the block is an fsl block iff the first
/// whitespace-separated token of the info string (the text after the run) is
/// exactly `fsl`. A closing fence is a line whose trimmed form is a run of the
/// same character, at least as long as the opening run, followed by nothing
/// else. While inside a fence, lines are only checked for closing — a line
/// that looks like an opening fence is ordinary content of the current block,
/// never a nested fence. An unterminated fence runs to end of file.
///
/// Returns `None` when the source contains no ` ```fsl ` fences, meaning it is
/// not a literate FSL document — the caller should reject it or treat it as
/// plain text rather than feeding an empty string to the parser.
#[must_use]
pub fn extract_literate_fsl(source: &str) -> Option<String> {
    enum FenceKind {
        Fsl,
        Other,
    }

    struct OpenFence {
        ch: char,
        len: usize,
        kind: FenceKind,
    }

    let mut open: Option<OpenFence> = None;
    let mut found_fsl_fence = false;
    let mut output = String::with_capacity(source.len());

    let lines_iter: Vec<&str> = source.split('\n').collect();
    let line_count = lines_iter.len();
    for (i, line) in lines_iter.into_iter().enumerate() {
        let trimmed = line.trim();
        let is_last = i + 1 == line_count;
        if let Some(fence) = &open {
            let closes = closing_fence_len(trimmed, fence.ch).is_some_and(|len| len >= fence.len);
            if closes {
                open = None;
            } else if matches!(fence.kind, FenceKind::Fsl) {
                output.push_str(line);
            }
        } else if let Some((ch, len, info)) = opening_fence(trimmed) {
            let is_fsl = info.split_whitespace().next() == Some("fsl");
            if is_fsl {
                found_fsl_fence = true;
            }
            open = Some(OpenFence {
                ch,
                len,
                kind: if is_fsl {
                    FenceKind::Fsl
                } else {
                    FenceKind::Other
                },
            });
        }
        if !is_last {
            output.push('\n');
        }
    }

    if found_fsl_fence { Some(output) } else { None }
}

/// Detect an opening fence: a run of three or more identical backtick or tilde
/// characters at the start of the trimmed line. Returns the fence character,
/// the run length, and the info string (the remainder of the trimmed line).
fn opening_fence(trimmed: &str) -> Option<(char, usize, &str)> {
    let ch = trimmed.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let len = trimmed
        .chars()
        .take_while(|&candidate| candidate == ch)
        .count();
    if len < 3 {
        return None;
    }
    // `ch` is single-byte ASCII, so the byte offset equals the char count above.
    Some((ch, len, &trimmed[len..]))
}

/// Detect a closing fence for the given fence character: the trimmed line
/// (already stripped of surrounding whitespace by the caller) consists
/// entirely of that character, with no info string. Returns the run length so
/// the caller can compare it against the opening run length.
fn closing_fence_len(trimmed: &str, ch: char) -> Option<usize> {
    if trimmed.is_empty() || !trimmed.chars().all(|candidate| candidate == ch) {
        return None;
    }
    Some(trimmed.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_a_single_fsl_block() {
        let doc = "\
# Title

Some prose.

```fsl
spec Toggle {
  state active: Bool = false
}
```

More prose.
";
        let blanked = extract_literate_fsl(doc).expect("should detect fsl fence");
        assert!(blanked.contains("spec Toggle {"));
        assert!(blanked.contains("state active: Bool = false"));
        // Prose lines are blank.
        let lines: Vec<&str> = blanked.lines().collect();
        assert_eq!(lines[0], "", "title line should be blank");
        assert_eq!(lines[2], "", "prose line should be blank");
        // The fsl content lines have the same line numbers as in the doc.
        let doc_lines: Vec<&str> = doc.lines().collect();
        let spec_line = doc_lines
            .iter()
            .position(|line| line.contains("spec Toggle"))
            .unwrap();
        assert_eq!(lines[spec_line], doc_lines[spec_line]);
    }

    #[test]
    fn preserves_line_count() {
        let doc = "# H1\n\n```fsl\nspec S {}\n```\n\n# H2\n";
        let blanked = extract_literate_fsl(doc).unwrap();
        assert_eq!(
            blanked.lines().count(),
            doc.lines().count(),
            "line count must be identical"
        );
    }

    #[test]
    fn multiple_blocks_form_one_compilation_unit() {
        let doc = "\
# States

```fsl
spec Multi {
  state x: Int = 0
```

# Actions

```fsl
  action inc { x' = x + 1 }
}
```
";
        let blanked = extract_literate_fsl(doc).unwrap();
        assert!(blanked.contains("state x: Int = 0"));
        assert!(blanked.contains("action inc"));
    }

    #[test]
    fn returns_none_without_fsl_fences() {
        assert!(extract_literate_fsl("# Just a readme\n\nNo fsl here.\n").is_none());
    }

    #[test]
    fn ignores_non_fsl_fenced_blocks() {
        let doc = "```python\nprint('hello')\n```\n";
        assert!(extract_literate_fsl(doc).is_none());
    }

    #[test]
    fn non_fsl_fence_does_not_interfere_with_fsl_fence() {
        let doc = "\
```python
print('hello')
```

```fsl
spec S {}
```
";
        let blanked = extract_literate_fsl(doc).unwrap();
        assert!(blanked.contains("spec S {}"));
        // Python block lines are blank.
        let lines: Vec<&str> = blanked.lines().collect();
        assert_eq!(lines[1], "", "python code should be blanked");
    }

    #[test]
    fn four_backtick_fence_containing_a_triple_backtick_fsl_example_extracts_only_real_fsl_blocks()
    {
        // A four-backtick "other" fence whose body demonstrates a ```fsl example must
        // not let that inner triple-backtick text terminate the four-backtick fence
        // early, and must not treat the inner ` ```fsl ` line as a real opening fence.
        let doc = "\
# Spec

```fsl
spec Counter {
  state { n: 0..3 }
  init { n = 0 }
  action inc() { n = n + 1 }
```

Example (four-backtick fence, inner three backticks are literal):

````text
```fsl
example only
```
````

```fsl
  invariant Low { n < 2 }
}
```
";
        let blanked = extract_literate_fsl(doc).expect("should detect fsl fences");
        assert!(!blanked.contains("example only"));
        assert!(!blanked.contains("````text"));
        let extracted = blanked
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            extracted,
            "spec Counter {\n  state { n: 0..3 }\n  init { n = 0 }\n  action inc() { n = n + 1 }\n  invariant Low { n < 2 }\n}"
        );
    }

    #[test]
    fn tilde_fenced_non_fsl_block_is_ignored_and_does_not_treat_an_inner_fsl_line_as_opening() {
        let doc = "\
~~~
```fsl
spec Ignored {}
```
~~~

```fsl
spec Real {}
```
";
        let blanked = extract_literate_fsl(doc).expect("should detect the real fsl fence");
        assert!(!blanked.contains("Ignored"));
        assert!(blanked.contains("spec Real {}"));
    }

    #[test]
    fn tilde_fsl_fence_is_recognized() {
        let doc = "~~~fsl\nspec Tilde {}\n~~~\n";
        let blanked = extract_literate_fsl(doc).expect("~~~fsl should open an fsl fence");
        assert!(blanked.contains("spec Tilde {}"));
    }

    #[test]
    fn backtick_run_with_trailing_text_inside_an_fsl_block_stays_content() {
        let doc = "\
```fsl
spec S {
``` foo
}
```
";
        let blanked = extract_literate_fsl(doc).expect("should detect fsl fence");
        assert!(blanked.contains("``` foo"));
        assert!(blanked.contains("spec S {"));
        assert!(blanked.contains('}'));
    }
}
