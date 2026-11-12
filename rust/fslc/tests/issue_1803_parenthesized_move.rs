// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1803: an `fsl 5` event may write its move in parentheses after the
//! phrase and its inputs, `event reject = Manager rejects Claim (Submitted ->
//! Rejected)` / `出来事 reject = 上長 が 申請 を 却下する (提出済み -> 却下済み)`,
//! beside the bare `Submitted -> Rejected`. Both lower to the same kernel;
//! `fmt` keeps the form each event was written in. The stay `in S` / `段階 S`
//! takes no parentheses.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

/// The `fsl 5` fixtures with events, as `(example, file)`: English, the
/// Japanese keywords with particles, all-Japanese, relations, stays (`rules`)
/// and updates on the line after a move (`multi_entity`).
const FIXTURES: [(&str, &str); 7] = [
    ("e2e", "fsl5_en.fsl"),
    ("e2e", "fsl5_ja_particles.fsl"),
    ("e2e", "fsl5_ja_full.fsl"),
    ("relations", "fsl5_en.fsl"),
    ("rules", "fsl5_en.fsl"),
    ("multi_entity", "fsl5_en.fsl"),
    ("multi_entity", "fsl5_ja_full.fsl"),
];

fn fixture_dir(example: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fsl5")
        .join(example)
}

fn fixture(example: &str, file: &str) -> String {
    std::fs::read_to_string(fixture_dir(example).join(file)).expect("read fixture")
}

/// `source` with `old`, which occurs exactly once, replaced by `new`.
fn edit(source: &str, old: &str, new: &str) -> String {
    assert_eq!(source.matches(old).count(), 1, "{old:?}");
    source.replacen(old, new, 1)
}

/// Whether `line` declares an event (`event`, `fault event`, `出来事`, `異常 出来事`).
fn declares_event(line: &str) -> bool {
    let words = line.split_whitespace().take(2).collect::<Vec<_>>();
    matches!(
        words.as_slice(),
        ["event" | "出来事", ..] | ["fault" | "異常", "event" | "出来事"]
    )
}

/// `source` with the move `S -> T` of each event line rewritten to what
/// `rewrite` returns for it, given its index among the event lines with a
/// move, the event's name and the move as written; `None` keeps the line.
fn rewrite_moves(source: &str, rewrite: impl Fn(usize, &str, &str) -> Option<String>) -> String {
    let mut moves = 0;
    let mut lines = Vec::new();
    for line in source.split_inclusive('\n') {
        let Some(arrow) = line.find(" -> ").filter(|_| declares_event(line)) else {
            lines.push(line.to_owned());
            continue;
        };
        let index = moves;
        moves += 1;
        let name = line
            .split_whitespace()
            .skip_while(|word| !matches!(*word, "event" | "出来事"))
            .nth(1)
            .expect("an event name");
        let from = line[..arrow].rfind(' ').expect("a word before the move") + 1;
        let rest = &line[arrow + 4..];
        let to = arrow + 4 + rest.find([' ', '\n']).unwrap_or(rest.len());
        match rewrite(index, name, &line[from..to]) {
            Some(new) => lines.push(format!("{}{new}{}", &line[..from], &line[to..])),
            None => lines.push(line.to_owned()),
        }
    }
    assert!(moves > 0, "no event moves in {source}");
    lines.concat()
}

/// `source` with the move of each event line `keep` selects (by its index
/// among the event lines with a move) written `(S -> T)`; the stays and the
/// other lines are unchanged.
fn parenthesize_where(source: &str, keep: impl Fn(usize) -> bool) -> String {
    rewrite_moves(source, |index, _, written| {
        keep(index).then(|| format!("({written})"))
    })
}

fn parenthesize(source: &str) -> String {
    parenthesize_where(source, |_| true)
}

/// `source` with the move of the event `reject` written as `new(move)`.
fn rewrite_reject(source: &str, new: impl Fn(&str) -> String) -> String {
    let rewritten = rewrite_moves(source, |_, name, written| {
        (name == "reject").then(|| new(written))
    });
    assert_ne!(rewritten, source, "no event reject");
    rewritten
}

/// A scratch directory holding a copy of one fixture example.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str, example: &str) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("issue_1803")
            .join(label);
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create scratch directory");
        for entry in std::fs::read_dir(fixture_dir(example)).expect("read example") {
            let path = entry.expect("example entry").path();
            std::fs::copy(&path, directory.join(path.file_name().expect("file name")))
                .expect("copy example");
        }
        Self(directory)
    }

    fn write(&self, name: &str, source: &str) {
        std::fs::write(self.0.join(name), source).expect("write scratch source");
    }

    /// The exit and stdout of `fslc <args>`.
    fn run(&self, args: &[&str]) -> (i32, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&self.0)
            .env("FSLC_CACHE", "off")
            .output()
            .expect("run fslc");
        (
            output.status.code().expect("exit code"),
            String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        )
    }

    /// The exit and the JSON packet of `fslc <args>`.
    fn fslc(&self, args: &[&str]) -> (i32, Value) {
        let (exit, stdout) = self.run(args);
        (exit, serde_json::from_str(&stdout).unwrap_or(Value::Null))
    }

    /// The text `fslc fmt doc.fsl <args>` prints for `source`, which must
    /// succeed.
    fn fmt(&self, source: &str, args: &[&str]) -> String {
        self.write("doc.fsl", source);
        let (exit, output) = self.run(&[&["fmt", "doc.fsl"][..], args].concat());
        assert_eq!(exit, 0, "fmt {args:?}: {output}");
        output
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `check` and `kernel` of `source`, written as `doc.fsl` beside its
/// example's other files.
fn check_and_kernel(label: &str, example: &str, source: &str) -> ((i32, Value), (i32, Value)) {
    let scratch = Scratch::new(label, example);
    scratch.write("doc.fsl", source);
    (
        scratch.fslc(&["check", "doc.fsl"]),
        scratch.fslc(&["kernel", "doc.fsl"]),
    )
}

/// Every event of each fixture written with the parenthesized move, and
/// every other one, gives the `check` packet and the kernel of the fixture
/// as written, compared in full: stays, inputs, updates on the next line and
/// relations included, in both languages.
#[test]
fn a_parenthesized_move_lowers_to_the_kernel_of_the_bare_move() {
    for (example, file) in FIXTURES {
        let source = fixture(example, file);
        let (check, kernel) = check_and_kernel(&format!("{example}-{file}-bare"), example, &source);
        assert_eq!((check.0, &check.1["result"]), (0, &json!("ok")), "{file}");
        assert_eq!(kernel.0, 0, "{example}/{file}");
        for (variant, rewritten) in [
            ("all", parenthesize(&source)),
            ("mixed", parenthesize_where(&source, |index| index % 2 == 0)),
        ] {
            assert_ne!(rewritten, source, "{example}/{file} {variant}");
            let label = format!("{example}-{file}-{variant}");
            let (paren_check, paren_kernel) = check_and_kernel(&label, example, &rewritten);
            assert_eq!(paren_check, check, "{example}/{file} {variant}: check");
            assert_eq!(paren_kernel, kernel, "{example}/{file} {variant}: kernel");
        }
    }
}

/// An input with a type before the parenthesized move: `with amount: Amount
/// (Draft -> Submitted)` is the input `amount` of type `Amount`, not a call.
#[test]
fn a_typed_input_before_a_parenthesized_move_keeps_its_type() {
    let source = fixture("e2e", "fsl5_en.fsl");
    let typed = edit(
        &source,
        "with amount   Draft -> Submitted",
        "with amount: Amount Draft -> Submitted",
    );
    let paren = edit(
        &typed,
        "amount: Amount Draft -> Submitted",
        "amount: Amount (Draft -> Submitted)",
    );
    let bare = check_and_kernel("typed-bare", "e2e", &typed);
    assert_eq!((bare.0.0, &bare.0.1["result"]), (0, &json!("ok")));
    assert_eq!(check_and_kernel("typed-paren", "e2e", &paren), bare);
}

/// fmt keeps each event in the form it was written in: a formatted document
/// with parenthesized moves, bare moves, or both is printed unchanged, and
/// `--lang` respells the keywords without moving a parenthesis.
#[test]
fn fmt_keeps_the_form_each_event_was_written_in() {
    for (example, file) in FIXTURES {
        let scratch = Scratch::new(&format!("fmt-{example}-{file}"), example);
        let bare = scratch.fmt(&fixture(example, file), &[]);
        let all = parenthesize(&bare);
        let mixed = parenthesize_where(&bare, |index| index % 2 == 1);
        for (form, source) in [("bare", &bare), ("all", &all), ("mixed", &mixed)] {
            assert_eq!(
                &scratch.fmt(source, &[]),
                source,
                "{example}/{file}: fmt {form}"
            );
            scratch.write("doc.fsl", source);
            let (exit, output) = scratch.run(&["fmt", "--check", "doc.fsl"]);
            assert_eq!(exit, 0, "{example}/{file}: fmt --check {form}: {output}");
        }
        for language in ["ja", "en"] {
            let respelled = scratch.fmt(&bare, &["--lang", language]);
            assert_eq!(
                scratch.fmt(&all, &["--lang", language]),
                parenthesize(&respelled),
                "{example}/{file}: fmt --lang {language}"
            );
        }
    }
}

/// fmt lays the parenthesized move out like the docs: one space before
/// `(`, none inside the parentheses, whatever the spacing written.
#[test]
fn fmt_spaces_a_parenthesized_move_after_the_phrase() {
    let scratch = Scratch::new("fmt-spacing", "e2e");
    let formatted = parenthesize(&scratch.fmt(&fixture("e2e", "fsl5_en.fsl"), &[]));
    let target = "Claim (Submitted -> Rejected)";
    for written in [
        "Claim(Submitted -> Rejected)",
        "Claim ( Submitted->Rejected )",
        "Claim   (Submitted -> Rejected)",
    ] {
        let source = edit(&formatted, target, written);
        assert_eq!(scratch.fmt(&source, &[]), formatted, "{written}");
    }
    let japanese = parenthesize(&scratch.fmt(&fixture("e2e", "fsl5_ja_full.fsl"), &[]));
    let source = edit(&japanese, "却下する (提出済み", "却下する(提出済み");
    assert_eq!(scratch.fmt(&source, &[]), japanese);
}

/// The 1-based line and column of the one occurrence of `needle`.
fn locate(source: &str, needle: &str) -> (usize, usize) {
    assert_eq!(source.matches(needle).count(), 1, "{needle:?}");
    let offset = source.find(needle).expect("occurs");
    let line_start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
    (
        source[..offset].matches('\n').count() + 1,
        source[line_start..offset].chars().count() + 1,
    )
}

fn refusal(example: &str, source: &str) -> Value {
    let scratch = Scratch::new("refusal", example);
    scratch.write("doc.fsl", source);
    let (exit, packet) = scratch.fslc(&["check", "doc.fsl"]);
    json!({
        "exit": exit,
        "result": packet["result"],
        "kind": packet["kind"],
        "message": packet["message"],
        "diagnostic_code": packet["diagnostic_code"],
        "loc": packet["loc"],
    })
}

fn parse_error(message: &str, code: &str, (line, column): (usize, usize)) -> Value {
    json!({
        "exit": 2,
        "result": "error",
        "kind": "parse",
        "message": format!("{message} at {line}:{column}"),
        "diagnostic_code": code,
        "loc": {"line": line, "column": column},
    })
}

const STAY_IN_PARENTHESES: &str = "a parenthesized move is `(S -> T)`; a stay is written `in S` \
     (`段階 S`) without parentheses";

/// What the parentheses may hold: one move. A stay, a lone stage, a missing
/// or doubled parenthesis, and full-width parentheses are refused, in each
/// language, at the token that is wrong.
#[test]
fn the_parentheses_hold_one_move_and_nothing_else() {
    let cases = [
        ("e2e", "fsl5_en.fsl", "Submitted -> Rejected", "Submitted"),
        (
            "e2e",
            "fsl5_ja_full.fsl",
            "提出済み -> 却下済み",
            "提出済み",
        ),
    ];
    for (example, file, written, from) in cases {
        let source = fixture(example, file);
        let stay_keyword = if file == "fsl5_en.fsl" {
            "in"
        } else {
            "段階"
        };
        let next = if file == "fsl5_en.fsl" {
            "  event pay"
        } else {
            "  出来事 pay"
        };
        let replace = |new: &str| {
            rewrite_reject(&source, |move_text| {
                assert_eq!(move_text, written);
                new.to_owned()
            })
        };

        let one_stage = replace(&format!("({from})"));
        assert_eq!(
            refusal(example, &one_stage),
            parse_error(
                STAY_IN_PARENTHESES,
                "FSL-PARSE",
                locate(&one_stage, &format!("({from})"))
            ),
            "{file}: one stage"
        );
        let stayed = replace(&format!("({stay_keyword} {from})"));
        assert_eq!(
            refusal(example, &stayed),
            parse_error(
                STAY_IN_PARENTHESES,
                "FSL-PARSE",
                locate(&stayed, &format!("({stay_keyword} {from})"))
            ),
            "{file}: stay"
        );
        let unclosed = replace(&format!("({written}"));
        let (line, _) = locate(&unclosed, &format!("({written}"));
        let (next_line, _) = locate(&unclosed, next);
        assert_eq!(next_line, line + 1);
        assert_eq!(
            refusal(example, &unclosed),
            parse_error("expected ')'", "FSL-PARSE", (next_line, 3)),
            "{file}: unclosed"
        );
        let doubled = replace(&format!("(({written}))"));
        let (line, column) = locate(&doubled, &format!("(({written}))"));
        assert_eq!(
            refusal(example, &doubled),
            parse_error("expected identifier", "FSL-PARSE", (line, column + 1)),
            "{file}: doubled"
        );
        let extra_close = replace(&format!("{written})"));
        let (line, column) = locate(&extra_close, &format!("{written})"));
        let refused = refusal(example, &extra_close);
        assert_eq!(
            (&refused["exit"], &refused["kind"], &refused["loc"]),
            (
                &json!(2),
                &json!("parse"),
                &json!({"line": line, "column": column + written.chars().count()})
            ),
            "{file}: extra ')' {refused}"
        );
        let full_width = replace(&format!("（{written}）"));
        assert_eq!(
            refusal(example, &full_width),
            parse_error(
                "full-width character '（'; write '('",
                "fullwidth_symbol",
                locate(&full_width, "（")
            ),
            "{file}: full-width"
        );
    }
}
