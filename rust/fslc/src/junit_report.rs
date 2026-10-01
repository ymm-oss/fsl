// SPDX-License-Identifier: Apache-2.0

//! Test counts from a `JUnit` XML report, for the `fslc chain` `[impl]` layer
//! (issue #1200).
//!
//! An exit code of 0 does not say that any test ran: a suite whose tests are
//! all skipped, or that collected none, exits 0 too. When the manifest names a
//! `report`, the chain reads the counts from it instead of trusting the exit
//! code alone. `JUnit` XML is the one machine-readable format every testgen
//! target's runner can write (`pytest --junitxml`, `vitest --reporter=junit`,
//! `swift test --xunit-output`, Gradle's `build/test-results`, `dart test`
//! via `tojunit`, `phpunit --log-junit`).
//!
//! The counts come from the `<testcase>` elements, not from the `tests` /
//! `skipped` attributes of `<testsuite>`: the attribute names and their
//! presence differ between runners, while every runner writes one
//! `<testcase>` per test and marks a skipped, failed, or erroring one with a
//! `<skipped>`, `<failure>`, or `<error>` child. This is a scanner for that
//! one shape, not a general XML parser; CDATA sections and comments are
//! removed first so captured test output cannot be mistaken for a marker.

/// The tests one or more `JUnit` reports record.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct JunitCounts {
    /// Every `<testcase>` element.
    pub tests: u64,
    /// Test cases with a `<skipped>` child.
    pub skipped: u64,
    /// Test cases with a `<failure>` child.
    pub failures: u64,
    /// Test cases with an `<error>` child.
    pub errors: u64,
}

impl JunitCounts {
    /// Test cases that ran: neither skipped nor never started.
    #[must_use]
    pub const fn executed(self) -> u64 {
        self.tests.saturating_sub(self.skipped)
    }

    /// Add another report's counts.
    #[must_use]
    pub const fn plus(self, other: Self) -> Self {
        Self {
            tests: self.tests + other.tests,
            skipped: self.skipped + other.skipped,
            failures: self.failures + other.failures,
            errors: self.errors + other.errors,
        }
    }
}

/// Remove every `open … close` span (CDATA sections, comments).
fn strip_spans(text: &str, open: &str, close: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(open) {
        out.push_str(&rest[..start]);
        match rest[start + open.len()..].find(close) {
            Some(end) => rest = &rest[start + open.len() + end + close.len()..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Whether `text` starts an element named exactly `name` (`<name`, then a
/// delimiter), so `<testcase` does not match `<testcases`.
fn element_at(text: &str, name: &str) -> bool {
    text.strip_prefix(name).is_some_and(|rest| {
        rest.chars()
            .next()
            .is_some_and(|next| next.is_whitespace() || next == '>' || next == '/')
    })
}

fn contains_element(body: &str, name: &str) -> bool {
    body.match_indices('<')
        .any(|(index, _)| element_at(&body[index + 1..], name))
}

/// Count the test cases of one `JUnit` XML document.
///
/// # Errors
///
/// Rejects a document with no `<testsuite>`/`<testsuites>` root and a
/// `<testcase>` whose start tag or element is never closed, since neither can
/// be counted truthfully.
pub fn junit_counts(text: &str) -> Result<JunitCounts, String> {
    let text = strip_spans(&strip_spans(text, "<![CDATA[", "]]>"), "<!--", "-->");
    if !text
        .match_indices('<')
        .any(|(index, _)| element_at(&text[index + 1..], "testsuite"))
        && !text
            .match_indices('<')
            .any(|(index, _)| element_at(&text[index + 1..], "testsuites"))
    {
        return Err("not a JUnit XML report: no <testsuite> or <testsuites> element".to_owned());
    }
    let mut counts = JunitCounts::default();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("<testcase") {
        let after = &rest[start + 1..];
        if !element_at(after, "testcase") {
            rest = &rest[start + "<testcase".len()..];
            continue;
        }
        let tag_end = after
            .find('>')
            .ok_or_else(|| "unterminated <testcase> start tag".to_owned())?;
        counts.tests += 1;
        if after[..tag_end].ends_with('/') {
            rest = &after[tag_end + 1..];
            continue;
        }
        let body_start = &after[tag_end + 1..];
        let body_end = body_start
            .find("</testcase>")
            .ok_or_else(|| "unterminated <testcase> element".to_owned())?;
        let body = &body_start[..body_end];
        if contains_element(body, "skipped") {
            counts.skipped += 1;
        }
        if contains_element(body, "failure") {
            counts.failures += 1;
        }
        if contains_element(body, "error") {
            counts.errors += 1;
        }
        rest = &body_start[body_end + "</testcase>".len()..];
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_pytest_style_skips_failures_and_passes() {
        let report = r#"<?xml version="1.0" encoding="utf-8"?>
<testsuites><testsuite name="pytest" errors="0" failures="1" skipped="1" tests="3">
<testcase classname="t" name="a" time="0.001"/>
<testcase classname="t" name="b"><skipped type="pytest.skip" message="Adapter not implemented">skip</skipped></testcase>
<testcase classname="t" name="c"><failure message="adapter not wired">boom</failure></testcase>
</testsuite></testsuites>"#;
        assert_eq!(
            junit_counts(report),
            Ok(JunitCounts {
                tests: 3,
                skipped: 1,
                failures: 1,
                errors: 0
            })
        );
    }

    #[test]
    fn captured_output_cannot_fake_a_marker() {
        let report = r#"<testsuite tests="1"><testcase name="a"><system-out><![CDATA[<skipped/> <failure>]]></system-out><!-- <error/> --></testcase></testsuite>"#;
        let counts = junit_counts(report).expect("count");
        assert_eq!(
            (counts.tests, counts.skipped, counts.failures, counts.errors),
            (1, 0, 0, 0)
        );
        assert_eq!(counts.executed(), 1);
    }

    #[test]
    fn all_skipped_and_empty_reports_execute_nothing() {
        let skipped = r#"<testsuites><testsuite name="vitest" tests="2"><testcase name="a"><skipped/></testcase><testcase name="b"><skipped/></testcase></testsuite></testsuites>"#;
        assert_eq!(junit_counts(skipped).expect("count").executed(), 0);
        let empty = r#"<testsuites name="vitest tests" tests="0"></testsuites>"#;
        assert_eq!(junit_counts(empty), Ok(JunitCounts::default()));
    }

    #[test]
    fn non_junit_and_truncated_documents_are_rejected() {
        assert!(junit_counts("{\"tests\": 3}").is_err());
        assert!(junit_counts("<testsuites><testcase name=\"a\"><skipped/>").is_err());
        assert!(junit_counts("<testsuite><testcase name=\"a\"").is_err());
    }

    #[test]
    fn similarly_named_elements_are_not_test_cases() {
        let report = r#"<testsuite><testcases-extra/><testcase name="a"><errors-note/></testcase></testsuite>"#;
        assert_eq!(
            junit_counts(report),
            Ok(JunitCounts {
                tests: 1,
                ..JunitCounts::default()
            })
        );
    }
}
