// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

/// Extract FSL source from a Markdown document by in-place blanking.
///
/// Lines outside ` ```fsl ` fenced code blocks are replaced with empty lines so
/// that byte offsets, line numbers, and column positions in the blanked output
/// correspond 1:1 to the original document.  Multiple fsl blocks are treated as
/// one compilation unit (definitions can be split across sections).
///
/// Returns `None` when the source contains no ` ```fsl ` fences, meaning it is
/// not a literate FSL document — the caller should reject it or treat it as
/// plain text rather than feeding an empty string to the parser.
#[must_use]
pub fn extract_literate_fsl(source: &str) -> Option<String> {
    let mut inside_fsl = false;
    let mut inside_other_fence = false;
    let mut found_fsl_fence = false;
    let mut output = String::with_capacity(source.len());

    let lines_iter: Vec<&str> = source.split('\n').collect();
    let line_count = lines_iter.len();
    for (i, line) in lines_iter.into_iter().enumerate() {
        let trimmed = line.trim();
        let is_last = i + 1 == line_count;
        if inside_fsl {
            if trimmed == "```" || trimmed.starts_with("``` ") {
                inside_fsl = false;
            } else {
                output.push_str(line);
            }
        } else if inside_other_fence {
            if trimmed == "```" || trimmed.starts_with("``` ") {
                inside_other_fence = false;
            }
        } else if is_fsl_fence(trimmed) {
            found_fsl_fence = true;
            inside_fsl = true;
        } else if trimmed.starts_with("```") {
            inside_other_fence = true;
        }
        if !is_last {
            output.push('\n');
        }
    }

    if found_fsl_fence { Some(output) } else { None }
}

fn is_fsl_fence(trimmed: &str) -> bool {
    trimmed == "```fsl" || trimmed.starts_with("```fsl ")
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
}
