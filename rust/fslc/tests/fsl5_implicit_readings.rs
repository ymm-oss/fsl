// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! What `explain` and `lint` say about the readings a requirements document
//! headed `fsl 5` leaves implicit (#1817): where an event's inputs are
//! stored, a phrase that leaves the verb out, fairness, that the guards of an
//! event all must hold, and which lifecycle line gives an entity its initial
//! stage and stage order.
//!
//! - `explain` adds a `readings` object (JSON) and the matching lines
//!   (`--readable`); nothing else of its output changes.
//! - `lint` reports `input_stored_by_name`, `verb_from_event_name` and
//!   `initial_stage_from_line_order` as info `notes`, which change neither
//!   `findings`, `finding_count` nor the exit code.
//!
//! Each document is written in English and in Japanese (keywords, particles
//! and a glossary); the two spellings read the same.

#[path = "support/fslc_cli.rs"]
mod fslc_cli;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

const EN: &str = r#"fsl 5
requirements Shop {
  number Qty
  actor User, Shop
  entity User
  entity Order {
    qty: Qty = 0
    note: Qty = 0
    lifecycle Empty -> Placed -> Packed
    lifecycle Placed -> Cancelled
  }
  Order belongs to User as buyer via place
  event place = User places Order with qty, extra: Qty Empty -> Placed
    set note = input extra
  event pack = Shop Order Placed -> Packed
  fault event cancel = Shop cancels Order Placed -> Cancelled
  rule REQ-SHOP-001 "An order is placed for a positive quantity" {
    allow User places Order only when input qty > 0
  }
  scenario AC-SHOP-001 "An order is placed and packed" {
    User places o1 with buyer 0, qty 1, extra 2
    Shop pack o1
    then o1 is Packed
  }
  forbidden FB-SHOP-001 "No empty order" {
    User places o1 with buyer 0, qty 0, extra 0
    then rejected
  }
}
verify {
  instances Order = 1
  instances User = 1
  values Qty = 0..2
}
"#;

const JA: &str = r#"fsl 5
要件 Shop {
  用語集 {
    Qty = 数量型
    User = 利用者
    Shop = 店舗
    Order = 注文
    Order.qty = 数量
    Order.note = 備考
    Empty = 空
    Placed = 受付済み
    Packed = 梱包済み
    Cancelled = 取消済み
    places = 注文する
    cancels = 取り消す
    packs = 梱包する
  }
  数値 数量型
  アクター 利用者, 店舗
  エンティティ 利用者
  エンティティ 注文 {
    数量: 数量型 = 0
    備考: 数量型 = 0
    ライフサイクル 空 -> 受付済み -> 梱包済み
    ライフサイクル 受付済み -> 取消済み
  }
  注文 所属先 利用者 役名 buyer 契機 place
  出来事 place = 利用者 が 注文 を 注文する 数量, extra: 数量型 で 空 -> 受付済み
    更新 備考 = 入力 extra
  出来事 pack = 店舗 が 注文 を 受付済み -> 梱包済み
  異常 出来事 cancel = 店舗 が 注文 を 取り消す 受付済み -> 取消済み
  規則 REQ-SHOP-001 "注文は正の数量で受け付ける" {
    許可 利用者 が 注文 を 注文する 条件 入力 数量 > 0
  }
  シナリオ AC-SHOP-001 "注文を受け付けて梱包する" {
    利用者 が o1 を buyer 0, 数量 1, extra 2 で 注文する
    店舗 が o1 を pack
    結果 o1 は 梱包済み
  }
  拒否シナリオ FB-SHOP-001 "空の注文は受け付けない" {
    利用者 が o1 を buyer 0, 数量 0, extra 0 で 注文する
    結果 拒否
  }
}
検証 {
  個数 注文 = 1
  個数 利用者 = 1
  値域 数量型 = 0..2
}
"#;

/// The edits that make each implicit reading explicit, as `(English,
/// Japanese)` pairs of `(from, to)`: one lifecycle line, a written verb, and
/// an input named apart from the field it is stored in.
const EXPLICIT: [[(&str, &str); 2]; 6] = [
    [
        (
            "    lifecycle Empty -> Placed -> Packed\n    lifecycle Placed -> Cancelled\n",
            "    lifecycle Empty -> Placed -> Packed\n",
        ),
        (
            "    ライフサイクル 空 -> 受付済み -> 梱包済み\n    ライフサイクル 受付済み -> 取消済み\n",
            "    ライフサイクル 空 -> 受付済み -> 梱包済み\n",
        ),
    ],
    [
        (
            "  fault event cancel = Shop cancels Order Placed -> Cancelled\n",
            "",
        ),
        (
            "  異常 出来事 cancel = 店舗 が 注文 を 取り消す 受付済み -> 取消済み\n",
            "",
        ),
    ],
    [
        (
            "  event pack = Shop Order Placed -> Packed",
            "  event pack = Shop packs Order Placed -> Packed",
        ),
        (
            "  出来事 pack = 店舗 が 注文 を 受付済み -> 梱包済み",
            "  出来事 pack = 店舗 が 注文 を 梱包する 受付済み -> 梱包済み",
        ),
    ],
    [
        ("    Shop pack o1\n", "    Shop packs o1\n"),
        ("    店舗 が o1 を pack\n", "    店舗 が o1 を 梱包する\n"),
    ],
    [
        (
            "with qty, extra: Qty Empty -> Placed\n    set note = input extra",
            "with amount: Qty, extra: Qty Empty -> Placed\n    set note = input extra, qty = input amount",
        ),
        (
            "数量, extra: 数量型 で 空 -> 受付済み\n    更新 備考 = 入力 extra",
            "amount: 数量型, extra: 数量型 で 空 -> 受付済み\n    更新 備考 = 入力 extra, 数量 = 入力 amount",
        ),
    ],
    [
        ("only when input qty > 0", "only when input amount > 0"),
        ("条件 入力 数量 > 0", "条件 入力 amount > 0"),
    ],
];

/// The step inputs that follow the renamed input, English then Japanese.
const EXPLICIT_STEPS: [[(&str, &str); 2]; 2] = [
    [
        ("qty 1, extra 2", "amount 1, extra 2"),
        ("qty 0, extra 0", "amount 0, extra 0"),
    ],
    [
        ("数量 1, extra 2", "amount 1, extra 2"),
        ("数量 0, extra 0", "amount 0, extra 0"),
    ],
];

/// The edits of `EXPLICIT` at `positions` in spelling `index`, with the
/// step inputs when the input is renamed.
fn explicit_edits(index: usize, positions: &[usize]) -> Vec<(&'static str, &'static str)> {
    let mut edits = positions
        .iter()
        .map(|position| EXPLICIT[*position][index])
        .collect::<Vec<_>>();
    if positions.contains(&4) {
        edits.extend(EXPLICIT_STEPS[index]);
    }
    edits
}

static SCRATCH_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

/// `text` written to `name` in a directory of its own: the tests run in
/// parallel and reuse names, and `--readable` output shows the name.
fn scratch(name: &str, text: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("fsl5_implicit_readings")
        .join(format!(
            "{}-{}",
            std::process::id(),
            SCRATCH_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
    std::fs::create_dir_all(&directory).expect("scratch directory");
    let path = directory.join(name);
    std::fs::write(&path, text).expect("write scratch source");
    path
}

/// `(stdout, exit)` of `fslc <args...> <path>`, run in `path`'s directory.
fn fslc(args: &[&str], path: &Path) -> (String, i32) {
    let output = fslc_cli::command()
        .args(args)
        .arg(path.file_name().expect("file name"))
        .current_dir(path.parent().expect("directory"))
        .output()
        .expect("run fslc");
    (
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        output.status.code().expect("exit code"),
    )
}

fn json_of(args: &[&str], path: &Path) -> (Value, i32) {
    let (stdout, exit) = fslc(args, path);
    let value =
        serde_json::from_str(&stdout).unwrap_or_else(|error| panic!("{args:?}: {error}: {stdout}"));
    (value, exit)
}

/// `text` with each `(from, to)` replaced exactly once.
fn edited(text: &str, edits: &[(&str, &str)]) -> String {
    let mut text = text.to_owned();
    for (from, to) in edits {
        assert_eq!(text.matches(from).count(), 1, "{from}");
        text = text.replacen(from, to, 1);
    }
    text
}

/// `value` without its `line` keys: the two spellings place their lines
/// apart (the glossary), and nothing else of a reading may differ.
fn without_lines(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| key.as_str() != "line")
                .map(|(key, item)| (key.clone(), without_lines(item)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(without_lines).collect()),
        other => other.clone(),
    }
}

fn expected_readings() -> Value {
    json!({
        "entities": [{
            "entity": "Order",
            "initial": "Empty",
            "stages": ["Empty", "Placed", "Packed", "Cancelled"],
            "lifecycle_lines": 2,
        }],
        "events": [
            {
                "event": "place",
                "entity": "Order",
                "verb": "places",
                "verb_omitted": false,
                "fair": true,
                "inputs": [
                    {"name": "u", "written": false, "stored": [{"field": "buyer", "by": "relation"}]},
                    {"name": "qty", "written": true, "stored": [{"field": "qty", "by": "same_name"}]},
                    {"name": "extra", "written": true, "stored": [{"field": "note", "by": "set"}]},
                ],
                "guards": [
                    {
                        "index": 0,
                        "expr": "order_stage[o] == Empty",
                        "conjunctive": true,
                        "origins": [{"kind": "move", "detail": "Empty -> Placed"}],
                    },
                    {
                        "index": 1,
                        "expr": "qty > 0",
                        "conjunctive": true,
                        "origins": [{
                            "kind": "allow",
                            "detail": "REQ-SHOP-001 allow User places Order only when input qty > 0",
                        }],
                    },
                ],
            },
            {
                "event": "pack",
                "entity": "Order",
                "verb": "pack",
                "verb_omitted": true,
                "fair": true,
                "inputs": [],
                "guards": [{
                    "index": 0,
                    "expr": "order_stage[o] == Placed",
                    "conjunctive": true,
                    "origins": [{"kind": "move", "detail": "Placed -> Packed"}],
                }],
            },
            {
                "event": "cancel",
                "entity": "Order",
                "verb": "cancels",
                "verb_omitted": false,
                "fair": false,
                "inputs": [],
                "guards": [{
                    "index": 0,
                    "expr": "order_stage[o] == Placed",
                    "conjunctive": true,
                    "origins": [{"kind": "move", "detail": "Placed -> Cancelled"}],
                }],
            },
        ],
    })
}

/// `explain` reads each event and entity, in both spellings, as
/// `expected_readings`: in canonical names the two are one reading.
#[test]
fn explain_lists_each_event_and_lifecycle_reading() {
    for (name, source) in [("readings_en.fsl", EN), ("readings_ja.fsl", JA)] {
        let path = scratch(name, source);
        let (output, exit) = json_of(&["explain", "--depth", "2", "--names", "canonical"], &path);
        assert_eq!(exit, 0, "{name}: {output}");
        assert_eq!(
            without_lines(&output["readings"]),
            expected_readings(),
            "{name}"
        );
        // Every origin keeps the line it was read from.
        for guard in output["readings"]["events"][0]["guards"]
            .as_array()
            .unwrap()
        {
            assert!(guard["origins"][0]["line"].is_u64(), "{name}: {guard}");
        }
    }
    // Without `--names canonical` a reader of the glossary sees its words.
    let (output, _) = json_of(
        &["explain", "--depth", "2"],
        &scratch("readings_ja.fsl", JA),
    );
    assert_eq!(
        output["readings"]["entities"][0],
        json!({
            "entity": "注文",
            "initial": "空",
            "stages": ["空", "受付済み", "梱包済み", "取消済み"],
            "lifecycle_lines": 2,
        })
    );
    assert_eq!(
        output["readings"]["events"][0]["inputs"][1],
        json!({"name": "数量", "written": true, "stored": [{"field": "数量", "by": "same_name"}]})
    );
}

/// The readings follow the source: with each one made explicit, `explain`
/// says so, in both spellings.
#[test]
fn explain_readings_follow_the_source() {
    for (index, (name, source)) in [("explicit_en.fsl", EN), ("explicit_ja.fsl", JA)]
        .into_iter()
        .enumerate()
    {
        let edits = explicit_edits(index, &[0, 1, 2, 3, 4, 5]);
        let path = scratch(name, &edited(source, &edits));
        let (output, exit) = json_of(&["explain", "--depth", "2", "--names", "canonical"], &path);
        assert_eq!(exit, 0, "{name}: {output}");
        let readings = &output["readings"];
        assert_eq!(
            readings["entities"],
            json!([{
                "entity": "Order",
                "initial": "Empty",
                "stages": ["Empty", "Placed", "Packed"],
                "lifecycle_lines": 1,
            }]),
            "{name}"
        );
        let events = readings["events"].as_array().unwrap();
        assert_eq!(events.len(), 2, "{name}");
        assert_eq!(
            events[0]["inputs"],
            json!([
                {"name": "u", "written": false, "stored": [{"field": "buyer", "by": "relation"}]},
                {"name": "amount", "written": true, "stored": [{"field": "qty", "by": "set"}]},
                {"name": "extra", "written": true, "stored": [{"field": "note", "by": "set"}]},
            ]),
            "{name}"
        );
        assert_eq!(events[1]["verb"], json!("packs"), "{name}");
        assert_eq!(events[1]["verb_omitted"], json!(false), "{name}");
    }
}

/// An input read only inside a larger expression is stored nowhere, and an
/// input named like no field is not stored by its name.
#[test]
fn explain_reports_an_input_stored_nowhere() {
    let edits = [
        ("with qty, extra: Qty", "with qty, extra: Qty, spare: Qty"),
        (
            "set note = input extra",
            "set note = input extra + input spare",
        ),
        ("qty 1, extra 2", "qty 1, extra 0, spare 1"),
        ("qty 0, extra 0", "qty 0, extra 0, spare 0"),
    ];
    let path = scratch("stored_nowhere_en.fsl", &edited(EN, &edits));
    let (output, exit) = json_of(&["explain", "--depth", "2"], &path);
    assert_eq!(exit, 0, "{output}");
    let inputs = &output["readings"]["events"][0]["inputs"];
    assert_eq!(
        inputs[2],
        json!({"name": "extra", "written": true, "stored": []})
    );
    assert_eq!(
        inputs[3],
        json!({"name": "spare", "written": true, "stored": []})
    );
}

/// `--readable` shows the same readings as lines, in both spellings.
#[test]
fn explain_readable_shows_each_reading() {
    let expected_en = [
        "Lifecycles:\n  - Order: initial Empty; stages Empty, Placed, Packed, Cancelled (2 lifecycle \
         lines: the first stage of the first line is the initial one; the stages are ordered as \
         they first appear)\n",
        "  - place(o: Order, u: User, qty: Qty, extra: Qty) [fair]\n    event: User places Order \
         Empty -> Placed (readings_en.fsl:13)\n    fair: yes (every event but a fault event)\n    \
         input u: stored in buyer (the relation's actor instance)\n    input qty: stored in qty \
         (the field of the same name)\n    input extra: stored in note (set note = input extra)\n",
        "    guards: all 2 requires below must hold (an allow … only when adds one; it allows \
         nothing by itself)\n    requires:\n",
        "  - pack(o: Order) [fair]\n    event: Shop pack Order Placed -> Packed \
         (readings_en.fsl:15)\n    verb: none written; the event name pack is the verb\n    fair: \
         yes (every event but a fault event)\n",
        "  - cancel(o: Order)\n    event: fault Shop cancels Order Placed -> Cancelled \
         (readings_en.fsl:16)\n    fair: no (fault event)\n",
    ];
    let (text, exit) = fslc(
        &["explain", "--readable", "--depth", "2"],
        &scratch("readings_en.fsl", EN),
    );
    assert_eq!(exit, 0, "{text}");
    for expected in expected_en {
        assert!(text.contains(expected), "missing {expected:?} in\n{text}");
    }
    let expected_ja = [
        "Lifecycles:\n  - 注文: initial 空; stages 空, 受付済み, 梱包済み, 取消済み (2 lifecycle \
         lines:",
        "    input 数量: stored in 数量 (the field of the same name)\n",
        "    input extra: stored in 備考 (set 備考 = input extra)\n",
        "    verb: none written; the event name pack is the verb\n",
        "    fair: no (fault event)\n",
        "    guards: all 2 requires below must hold",
    ];
    let (text, exit) = fslc(
        &["explain", "--readable", "--depth", "2"],
        &scratch("readings_ja.fsl", JA),
    );
    assert_eq!(exit, 0, "{text}");
    for expected in expected_ja {
        assert!(text.contains(expected), "missing {expected:?} in\n{text}");
    }
}

/// A document of any other form explains as before: no `readings`.
#[test]
fn explain_of_another_form_has_no_readings() {
    let source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../specs/cart_v1.fsl"),
    )
    .expect("cart spec");
    let path = scratch("cart_v1.fsl", &source);
    let (output, exit) = json_of(&["explain", "--depth", "2"], &path);
    assert_eq!(exit, 0, "{output}");
    assert!(output.get("readings").is_none(), "{output}");
    let (text, _) = fslc(&["explain", "--readable", "--depth", "2"], &path);
    assert!(!text.contains("Lifecycles:"), "{text}");
}

/// `(code, line, column)` of each note of the one file `lint` read.
fn notes(output: &Value) -> Vec<(String, u64, u64)> {
    output["files"][0]["notes"]
        .as_array()
        .unwrap_or_else(|| panic!("no notes: {output}"))
        .iter()
        .map(|note| {
            assert_eq!(note["severity"], json!("info"), "{note}");
            assert_eq!(note["taxonomy"], json!("ambiguous_intent"), "{note}");
            assert_eq!(note["machine_applicable"], json!(false), "{note}");
            (
                note["code"].as_str().unwrap().to_owned(),
                note["loc"]["line"].as_u64().unwrap(),
                note["loc"]["column"].as_u64().unwrap(),
            )
        })
        .collect()
}

/// `lint` reports each implicit reading as an info note at the word it
/// reads; the notes change neither the findings nor the exit code.
#[test]
fn lint_notes_each_implicit_reading() {
    for (name, source, expected) in [
        (
            "readings_en.fsl",
            EN,
            vec![
                ("initial_stage_from_line_order", 9, 15),
                ("input_stored_by_name", 13, 40),
                ("verb_from_event_name", 15, 16),
            ],
        ),
        (
            "readings_ja.fsl",
            JA,
            vec![
                ("initial_stage_from_line_order", 24, 13),
                ("input_stored_by_name", 28, 31),
                ("verb_from_event_name", 30, 14),
            ],
        ),
    ] {
        let (output, exit) = json_of(&["lint"], &scratch(name, source));
        assert_eq!(exit, 0, "{name}: {output}");
        assert_eq!(output["finding_count"], json!(0), "{name}");
        assert_eq!(output["files"][0]["findings"], json!([]), "{name}");
        assert_eq!(output["note_count"], json!(3), "{name}");
        let expected = expected
            .into_iter()
            .map(|(code, line, column)| (code.to_owned(), line, column))
            .collect::<Vec<_>>();
        assert_eq!(notes(&output), expected, "{name}");
    }
    let (output, _) = json_of(&["lint"], &scratch("readings_en.fsl", EN));
    let messages = output["files"][0]["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|note| note["message"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        messages,
        [
            "entity 'Order' has 2 lifecycle lines: the first stage of the first line, 'Empty', \
             is the initial one, and the stages are ordered as they first appear (Empty, Placed, \
             Packed, Cancelled); reordering the lines changes both",
            "input 'qty' of event 'place' is stored in the field 'qty' of 'Order' because the \
             two are named alike; renaming either one drops the store",
            "event 'pack' leaves the verb out of its phrase, so its name 'pack' is the verb its \
             scenario steps use",
        ]
    );
    // A reader of the glossary sees its words beside the canonical ones.
    let (output, _) = json_of(&["lint"], &scratch("readings_ja.fsl", JA));
    let message = output["files"][0]["notes"][1]["message"].as_str().unwrap();
    assert!(
        message.starts_with("input '数量（qty）' of event 'place' is stored in the field '数量（qty）' of '注文（Order）'"),
        "{message}"
    );
}

/// With each reading written out, `lint` notes nothing, in both spellings.
#[test]
fn lint_notes_nothing_for_explicit_readings() {
    for (index, (name, source)) in [("explicit_en.fsl", EN), ("explicit_ja.fsl", JA)]
        .into_iter()
        .enumerate()
    {
        // Each edit alone removes exactly its own note.
        for (positions, gone) in [
            (&[0, 1][..], "initial_stage_from_line_order"),
            (&[2, 3][..], "verb_from_event_name"),
            (&[4, 5][..], "input_stored_by_name"),
        ] {
            let edits = explicit_edits(index, positions);
            let (output, exit) = json_of(&["lint"], &scratch(name, &edited(source, &edits)));
            assert_eq!(exit, 0, "{name} {gone}: {output}");
            let codes = notes(&output)
                .into_iter()
                .map(|(code, _, _)| code)
                .collect::<Vec<_>>();
            assert_eq!(codes.len(), 2, "{name} {gone}: {codes:?}");
            assert!(!codes.iter().any(|code| code == gone), "{name}: {codes:?}");
        }
        let edits = explicit_edits(index, &[0, 1, 2, 3, 4, 5]);
        let (output, exit) = json_of(&["lint"], &scratch(name, &edited(source, &edits)));
        assert_eq!(exit, 0, "{name}: {output}");
        assert_lint_without_notes(&output);
    }
}

/// `lint` output with no note carries neither note key.
fn assert_lint_without_notes(output: &Value) {
    assert!(output["finding_count"].is_u64(), "{output}");
    assert!(output.get("note_count").is_none(), "{output}");
    let file = output["files"][0].as_object().expect("one file");
    assert!(file.contains_key("findings"), "{output}");
    assert!(!file.contains_key("notes"), "{output}");
}

/// A finding still decides the exit code and `finding_count` alone: notes
/// beside it are not counted.
#[test]
fn lint_notes_are_not_findings() {
    let path = scratch(
        "finding_en.fsl",
        &edited(EN, &[("rule REQ-SHOP-001", "rule REQ-1")]),
    );
    let (output, exit) = json_of(&["lint"], &path);
    assert_eq!(exit, 1, "{output}");
    assert_eq!(output["finding_count"], json!(1));
    assert_eq!(
        output["files"][0]["findings"][0]["code"],
        json!("non_canonical_id")
    );
    assert_eq!(output["note_count"], json!(3));
    // A document of another form has no note, so no note keys.
    let source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../specs/cart_v1.fsl"),
    )
    .expect("cart spec");
    let (output, _) = json_of(&["lint"], &scratch("cart_v1.fsl", &source));
    assert_lint_without_notes(&output);
}
