// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

#[path = "support/identifiers.rs"]
mod identifiers;

static NEXT: AtomicUsize = AtomicUsize::new(0);
const HEADER: &str = "// location sentinel\n\n   refinement R { impl Impl1599Spec abs Abs1599Spec ";
const IMPL_RANGE: &str = "spec Impl1599Spec { type Carrier1599 = 0..2 state { x: Carrier1599 = 0 } action go() { requires x == 0 x = 1 } }";
const ABS_PROGRESS: &str = "spec Abs1599Spec { type Carrier1599 = 0..0 state { y: Int = 0 } fair action step() { requires y < 2 y = y + 1 } leadsTo L { y == 0 ~> y == 2 } }";
const PROGRESS: &str = "action go() -> step() preserve progress { respond L by go } }";
const DOMAIN_NAMES: [&str; 3] = ["Carrier1599", "Impl1599Spec", "Abs1599Spec"];

struct Fixture(PathBuf);
impl Fixture {
    fn new(implementation: &str, abstraction: &str, mapping: &str) -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = root().join(format!("rust/target/fsl-1599-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fixture = Self(dir);
        for (file, source) in [
            ("impl.fsl", implementation),
            ("abs.fsl", abstraction),
            ("map.fsl", mapping),
        ] {
            fixture.write(file, source);
        }
        fixture
    }
    fn write(&self, file: &str, source: &str) {
        std::fs::write(self.0.join(file), source).unwrap();
    }
    fn run(&self, args: &[&str]) -> (Value, i32) {
        run(&self.0, args)
    }
    fn refine(&self) -> (Value, i32) {
        self.run(&["refine", "impl.fsl", "abs.fsl", "map.fsl", "--depth", "4"])
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn run(dir: &Path, args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(dir)
        .env_remove("FSLC_CACHE")
        .env_remove("FSLC_CACHE_VERIFY")
        .env("FSLC_CACHE_DIR", dir.join("cache-1599"))
        .output()
        .unwrap();
    assert!(
        output.stderr.is_empty(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let status = output.status.code().unwrap();
    println!("OBS {args:?} exit={status} {value}");
    (value, status)
}
fn verdict(observed: &(Value, i32), status: i32, result: &str, kind: Option<&str>) {
    assert_eq!(observed.1, status, "{}", observed.0);
    assert_eq!(observed.0["result"], result, "{}", observed.0);
    if let Some(kind) = kind {
        assert_eq!(observed.0["kind"], kind, "{}", observed.0);
    }
}
fn located_collision(observed: &(Value, i32), names: [&str; 3], line: u32, col: u32) {
    verdict(observed, 2, "error", Some("type"));
    assert_eq!(observed.0["loc"]["line"], line, "{}", observed.0);
    assert_eq!(observed.0["loc"]["column"], col, "{}", observed.0);
    let message = observed.0["message"].as_str().unwrap();
    identifiers::assert_names(message, &names);
}

fn issue_case(index: usize, control: bool) {
    let abs_enum = "spec Abs1599Spec { enum P { Member1599B } state { y: Int = 0 } fair action step() { requires y == 0 y = 1 } leadsTo L { y == 0 ~> y == 1 } }";
    let abs_safety = "spec Abs1599Spec { type Carrier1599 = 0..1 state { y: Int = 0 } action step() { requires y == 0 y = 1 } }";
    let rows = [
        (
            IMPL_RANGE,
            ABS_PROGRESS,
            "map y = x ".to_owned() + PROGRESS,
            "Carrier1599",
            "progress_lost",
        ),
        (
            "spec Impl1599Spec { enum S { A, Member1599B, C } state { s: S = A } fair action go() { requires s == Member1599B s = C } }",
            abs_enum,
            "map y = if s == C then 1 else 0 ".to_owned() + PROGRESS,
            "Member1599B",
            "progress_lost",
        ),
        (
            "spec Impl1599Spec { enum S { A, Member1599B } state { s: S = A } fair action go() { requires s == A s = Member1599B } }",
            abs_enum,
            "map y = if s == A then 0 else 1 ".to_owned() + PROGRESS,
            "Member1599B",
            "",
        ),
        (
            "spec Impl1599Spec { type Carrier1599 = 0..2 state { x: Int = 0 } action go() { requires x == 0 x = 1 } }",
            abs_safety,
            "map y = if (exists k: Carrier1599 { k == 2 }) then 1 else 0 action go() -> stutter }".to_owned(),
            "Carrier1599",
            "abs_state_mismatch",
        ),
        (
            "spec Impl1599Spec { enum Carrier1599 { A, Member1599B } state { x: Int = 0 } action go() { requires x == 0 x = 1 } }",
            abs_safety,
            "map y = if (exists k: Carrier1599 { k == Member1599B }) then 1 else 0 action go() -> stutter }".to_owned(),
            "Carrier1599",
            "abs_state_mismatch",
        ),
    ];
    let (implementation, abstraction, body, name, failure) = &rows[index];
    let (from, to) = if *name == "Carrier1599" {
        ("type Carrier1599", "type Renamed1599")
    } else {
        ("{ Member1599B }", "{ Q }")
    };
    let abstraction = if control {
        abstraction.replace(from, to)
    } else {
        (*abstraction).to_owned()
    };
    let fixture = Fixture::new(implementation, &abstraction, &(HEADER.to_owned() + body));
    // The control changes only abs; the implementation and mapping bytes are shared above.
    for file in ["impl.fsl", "abs.fsl"] {
        verdict(
            &fixture.run(&[
                "verify",
                "--no-cache",
                file,
                "--engine",
                "bmc",
                "--depth",
                "4",
            ]),
            0,
            "verified",
            None,
        );
    }
    let observed = fixture.refine();
    if !control {
        located_collision(&observed, [name, "Impl1599Spec", "Abs1599Spec"], 3, 4);
    } else if failure.is_empty() {
        verdict(&observed, 0, "refines", None);
    } else {
        verdict(&observed, 1, "refinement_failed", Some(failure));
    }
}
macro_rules! issue_tests { ($($name:ident: $index:expr, $control:expr;)*) => { $(#[test] fn $name() { issue_case($index, $control); })* }; }
issue_tests! {
    case1_collision: 0, false; case1_control: 0, true;
    case2a_collision: 1, false; case2a_control: 1, true;
    case2b_collision: 2, false; case2b_control: 2, true;
    case3a_collision: 3, false; case3a_control: 3, true;
    case3b_collision: 4, false; case3b_control: 4, true;
}

fn definitions(left: &str, right: &str, reject: bool) {
    let [left, right] = [left, right].map(|declaration| declaration.replace('T', "Carrier1599"));
    let fixture = Fixture::new(
        &format!("spec Impl1599Spec {{ {left} state {{ x: Int = 0 }} }}"),
        &format!("spec Abs1599Spec {{ {right} state {{ x: Int = 0 }} }}"),
        &(HEADER.to_owned() + "maps auto }"),
    );
    let observed = fixture.refine();
    if reject {
        located_collision(&observed, DOMAIN_NAMES, 3, 4);
    } else {
        verdict(&observed, 0, "refines", None);
    }
}
macro_rules! type_tests { ($($name:ident: $left:expr, $right:expr, $reject:expr;)*) => { $(#[test] fn $name() { definitions($left, $right, $reject); })* }; }
// All declarations are deliberately unused: merge validation cannot depend on use sites.
type_tests! {
    domain_same: "type T = 0..2", "type T = 0..2", false;
    domain_normalized: "const Max = 2 type T = 0..Max", "type T = 0..2", false;
    domain_symmetric: "symmetric type T = 0..2", "type T = 0..2", false;
    domain_bounds: "type T = 0..2", "type T = 0..1", true;
    domain_lower: "type T = 0..2", "type T = 1..2", true;
    kind_change: "enum T { A, B }", "type T = 0..1", true;
    enum_same: "enum T { A, B }", "enum T { A, B }", false;
    enum_order: "enum T { A, B }", "enum T { B, A }", true;
    enum_members: "enum T { A, B }", "enum T { A, C }", true;
    struct_same: "struct T { a: Int }", "struct T { a: Int }", false;
    struct_field: "struct T { a: Int }", "struct T { b: Int }", true;
    struct_type: "struct T { a: Int }", "struct T { a: Bool }", true;
    struct_named_type: "type T = 0..2 struct Box { a: T }", "type T = 0..1 struct Box { a: T }", true;
}

#[test]
fn shared_enum_without_progress_is_accepted() {
    definitions("enum S { A, B }", "enum P { B }", false);
}
#[test]
fn progress_member_collision_rejects_even_matching_ordinals() {
    let fixture = Fixture::new(
        "spec Impl1599Spec { enum S { A, Member1599B } state { s: S = A } fair action go() { requires s == A s = Member1599B } }",
        "spec Abs1599Spec { enum P { Q, Member1599B } state { y: Int = 0 } fair action step() { requires y == 0 y = 1 } leadsTo L { y == 0 ~> y == 1 } }",
        &(HEADER.to_owned() + "map y = if s == A then 0 else 1 " + PROGRESS),
    );
    located_collision(
        &fixture.refine(),
        ["Member1599B", "Impl1599Spec", "Abs1599Spec"],
        3,
        4,
    );
}
#[test]
fn shared_enum_bare_reference_remains_ambiguous() {
    let fixture = Fixture::new(
        "spec Impl1599Spec { enum S { A, B } state { s: S = A } }",
        "spec Abs1599Spec { enum P { B } state { y: Int = 0 } }",
        &(HEADER.to_owned() + "map y = if s == B then 1 else 0 }"),
    );
    let observed = fixture.refine();
    verdict(&observed, 2, "error", Some("type"));
    assert!(
        observed.0["message"]
            .as_str()
            .unwrap()
            .contains("ambiguous enum member 'B'")
    );
}

const GOVERNANCE: &str = "governance G {\n  control CTRL-1 \"preserved\"\n\n  preservation P {\n    before Abs1599Spec from \"abs.fsl\"\n    after Impl1599Spec from \"impl.fsl\"\n    preserve CTRL-1\n    checked_by refinement \"map.fsl\"\n  }\n}";
fn governance_collision(cmd: &str) {
    let fixture = Fixture::new(
        IMPL_RANGE,
        ABS_PROGRESS,
        &(HEADER.to_owned() + "map y = x " + PROGRESS),
    );
    fixture.write("governance.fsl", GOVERNANCE);
    located_collision(&fixture.run(&[cmd, "governance.fsl"]), DOMAIN_NAMES, 4, 3);
}
#[test]
fn governance_check_collision() {
    governance_collision("check");
}
#[test]
fn governance_verify_collision() {
    governance_collision("verify");
}
#[test]
fn inline_collision_is_located_at_implements() {
    let fixture = Fixture::new(
        "",
        "spec Abs1599Spec { type Carrier1599 = 0..1 state { x: Int = 0 } }",
        "",
    );
    fixture.write("inline.fsl", "requirements Impl1599Spec {\n\n   implements Abs1599Spec from \"abs.fsl\" { maps auto }\n type Carrier1599 = 0..2\n state { x: Int = 0 }\n}");
    located_collision(&fixture.run(&["check", "inline.fsl"]), DOMAIN_NAMES, 3, 4);
}
#[test]
fn chain_collision_reports_failed_link() {
    let fixture = Fixture::new(
        IMPL_RANGE,
        ABS_PROGRESS,
        &(HEADER.to_owned() + "map y = x " + PROGRESS),
    );
    fixture.write(
        "top.fsl",
        &ABS_PROGRESS.replace("Abs1599Spec", "Top1599Spec"),
    );
    fixture.write(
        "top-map.fsl",
        "refinement S { impl Abs1599Spec abs Top1599Spec maps auto }",
    );
    let observed = fixture.run(&[
        "refine",
        "impl.fsl",
        "abs.fsl",
        "map.fsl",
        "top.fsl",
        "top-map.fsl",
        "--depth",
        "4",
    ]);
    located_collision(&observed, DOMAIN_NAMES, 3, 4);
    assert_eq!(
        observed.0["failed_link"],
        json!({"from":"Impl1599Spec", "to":"Abs1599Spec", "kind":"type"})
    );
}

fn corpus(paths: &[&str], depth: &str, status: i32, kind: Option<&str>) {
    let mut args = vec!["refine"];
    args.extend_from_slice(paths);
    args.extend(["--depth", depth]);
    let observed = run(&root(), &args);
    verdict(
        &observed,
        status,
        if status == 0 {
            "refines"
        } else {
            "refinement_failed"
        },
        kind,
    );
}
macro_rules! corpus_tests { ($($name:ident: $(($paths:expr, $depth:expr, $status:expr, $kind:expr)),+;)*) => { $(#[test] fn $name() { $(corpus($paths, $depth, $status, $kind);)+ })* }; }
corpus_tests! {
    agentic_rag_symmetric_difference_stays_refines: (&["examples/agentic_rag/agentic_rag_requirements.fsl", "examples/agentic_rag/agentic_rag_business.fsl", "examples/agentic_rag/agentic_rag_requirements_refines_business.fsl"], "7", 0, None);
    identical_chain_types_stay_accepted:
        (&["examples/refinement_chain/bot.fsl", "examples/refinement_chain/mid.fsl", "examples/refinement_chain/bot_refines_mid.fsl"], "6", 0, None),
        (&["examples/refinement_chain/mid.fsl", "examples/refinement_chain/top.fsl", "examples/refinement_chain/mid_refines_top.fsl"], "6", 0, None),
        (&["examples/refinement_chain/bot.fsl", "examples/refinement_chain/mid.fsl", "examples/refinement_chain/bot_refines_mid.fsl", "examples/refinement_chain/top.fsl", "examples/refinement_chain/mid_refines_top.fsl"], "6", 0, None);
    order_refund_preserves_both_verdicts:
        (&["examples/validation/order_refund_windowed.fsl", "examples/validation/order_refund.fsl", "examples/validation/order_refund_windowed_refines.fsl"], "8", 0, None),
        (&["examples/validation/order_refund_instant.fsl", "examples/validation/order_refund.fsl", "examples/validation/order_refund_instant_refines.fsl"], "8", 1, Some("abs_requires_failed"));
    typed_enum_conversion_and_abstraction_stay_accepted:
        (&["rust/fslc/tests/fixtures/issue_450_design.fsl", "rust/fslc/tests/fixtures/issue_450_requirements.fsl", "rust/fslc/tests/fixtures/issue_450_mapping.fsl"], "2", 0, None),
        (&["examples/layers/return_impl.fsl", "examples/layers/return_system.fsl", "examples/layers/return_impl_refines.fsl"], "5", 0, None);
}
#[test]
fn boundary_changes_to_type_error() {
    let dir = "examples/gallery/adversarial/";
    let paths =
        ["impl", "abs", "map"].map(|part| format!("{dir}refine_mapping_boundary_{part}.fsl"));
    let observed = run(
        &root(),
        &["refine", &paths[0], &paths[1], &paths[2], "--depth", "2"],
    );
    located_collision(
        &observed,
        [
            "N",
            "GalleryAdversarialRefineImpl",
            "GalleryAdversarialRefineAbs",
        ],
        4,
        1,
    );
}

fn progress_governance_corpus(cmd: &str) {
    let mut args = vec![
        cmd,
        "examples/refinement_liveness/governance_detects_progress_merge_collision.fsl",
    ];
    if cmd == "verify" {
        args.push("--no-cache");
    }
    let observed = run(&root(), &args);
    located_collision(&observed, DOMAIN_NAMES, 7, 3);
}
#[test]
fn progress_governance_corpus_check() {
    progress_governance_corpus("check");
}
#[test]
fn progress_governance_corpus_verify() {
    progress_governance_corpus("verify");
}

#[test]
fn verifier_error_uses_semantics_boundary_without_trace() {
    let fixture = Fixture::new(
        "spec Impl1599Spec { state { x: Int = 0 } fair action go() { requires x == 0 x = 1 } }",
        "spec Abs1599Spec { state { y: Int = 0 } fair action step() { requires y == 0 y = 1 } leadsTo L { forall k: Int { y == 0 ~> y == 1 } } }",
        &(HEADER.to_owned() + "map y = x " + PROGRESS),
    );
    let observed = fixture.refine();
    verdict(&observed, 2, "error", Some("semantics"));
    for key in ["impl_trace", "trace", "abs_trace", "trace_type"] {
        assert!(observed.0.get(key).is_none(), "{key}: {}", observed.0);
    }
    identifiers::assert_names(observed.0["message"].as_str().unwrap(), &["Int"]);
}
#[test]
fn entity_instances_collision() {
    let dir = root().join("rust/fslc/tests/fixtures/inline_implements_bounds");
    for observed in ["check", "verify"].map(|cmd| run(&dir, &[cmd, "impl_spec_extra.fsl"])) {
        located_collision(&observed, ["Item", "SharedOnlyReq", "SharedOnly"], 2, 3);
        let message = observed.0["message"].as_str().unwrap();
        identifiers::assert_names(message, &["1", "3", "verify", "instances"]);
        assert!(message.contains("--instances Item="), "{message}");
    }
}
