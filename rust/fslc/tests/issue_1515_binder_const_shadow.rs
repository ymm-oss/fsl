// SPDX-License-Identifier: Apache-2.0

//! #1515: reject authored local names that shadow any constant before lowering
//! can substitute or prefix away the collision. Source errors must agree in
//! full across check and every engine. Supported static-domain controls preserve
//! lexical meaning without requiring dependent range bounds.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const ROUTES: [&str; 4] = ["check", "bmc", "induction", "explicit"];
const TEMPORAL_UNSUPPORTED: &str = "the explicit engine does not support leadsTo properties; use --engine bmc or exclude the leadsTo property";

#[derive(Clone, Copy)]
enum Outcome {
    Safe,
    Violated,
    Temporal,
}

struct Case {
    id: &'static str,
    source: String,
    outcome: Outcome,
}

fn case(id: &'static str, source: String, outcome: Outcome) -> Case {
    Case {
        id,
        source,
        outcome,
    }
}

fn spec(extra: &str, state: &str, init: &str, action: &str, property: &str) -> String {
    format!(
        "spec Shadow {{\n  const N = CONSTANT\n  {extra}\n  state {{ {state} }}\n  {init}\n  {action}\n  {property}\n}}\n"
    )
}

fn invariant(property: &str) -> String {
    spec("", "y: Int = 0", "", "action tick() { y = y }", property)
}

#[allow(clippy::too_many_lines)]
fn cases() -> Vec<Case> {
    use Outcome::{Safe, Temporal, Violated};
    let mut cases = vec![
        case(
            "forall-false-violation",
            invariant("invariant Shadow { forall @LOCAL in 0..1 { forall z in 0..LOCAL { z <= 1 } } }"),
            Safe,
        ),
        case(
            "count-false-pass-state-name",
            invariant("invariant C { forall @LOCAL in 0..1 { count(y in 0..LOCAL) == 3 } }"),
            Violated,
        ),
        case(
            "count-false-pass-fresh-name",
            invariant("invariant C { forall @LOCAL in 0..1 { count(w in 0..LOCAL) == 3 } }"),
            Violated,
        ),
        case(
            "exists",
            invariant("invariant E { exists @LOCAL in 0..1 { forall z in 0..LOCAL { z <= 1 } } }"),
            Safe,
        ),
        case(
            "typed-quantifier",
            spec("type K = 0..1", "y: Int = 0", "", "action tick() { y = y }", "invariant Q { forall @LOCAL: K { forall z in 0..LOCAL { z <= 1 } } }"),
            Safe,
        ),
        case(
            "collection-quantifier",
            invariant("invariant Q { forall @LOCAL in Set {0, 1} { forall z in 0..LOCAL { z <= 1 } } }"),
            Safe,
        ),
        case(
            "count-binder",
            invariant("invariant Q { count(@LOCAL in 0..1 where forall z in 0..LOCAL { z <= 1 }) == 2 }"),
            Safe,
        ),
        case(
            "nested-sum",
            invariant("invariant A { sum(@LOCAL in 0..1 of count(w in 0..LOCAL)) == 3 }"),
            Safe,
        ),
        case(
            "unique-binder",
            invariant("invariant Q { unique(@LOCAL in 0..1 where forall z in 0..LOCAL { z <= 1 }) }"),
            Violated,
        ),
        case(
            "exactly-one-binder",
            invariant("invariant Q { exactlyOne(@LOCAL in 0..1 where forall z in 0..LOCAL { z <= 1 }) }"),
            Violated,
        ),
        case(
            "range-action-parameter",
            spec("", "y: Int = 0", "", "action tick(@LOCAL in 0..1) { requires forall z in 0..LOCAL { z <= 1 } y = 1 }", "invariant Z { y == 0 }"),
            Violated,
        ),
        case(
            "typed-action-parameter",
            spec("type K = 0..1", "y: Int = 0", "", "action tick(@LOCAL: K) { requires forall z in 0..LOCAL { z <= 1 } y = 1 }", "invariant Z { y == 0 }"),
            Violated,
        ),
        case(
            "let",
            spec("", "y: Int = 0", "", "action tick() { let @LOCAL = 1 requires forall z in 0..LOCAL { z <= 1 } y = 1 }", "invariant Z { y == 0 }"),
            Violated,
        ),
        case(
            "def-parameter",
            spec("type K = 0..1\n  def bounded(@LOCAL: K) = forall z in 0..LOCAL { z <= 1 }", "y: Int = 0", "", "action tick() { y = y }", "invariant D { bounded(1) }"),
            Safe,
        ),
        case(
            "action-statement-forall",
            spec("", "m: Map<0..2, Int>", "init { forall i in 0..2 { m[i] = 0 } }", "action tick() { forall @LOCAL in 0..1 { m[LOCAL] = 1 } }", "invariant M { m[2] == 0 }"),
            Safe,
        ),
        case(
            "init-statement-forall",
            spec("", "m: Map<0..2, Int>, y: Int = 0", "init { forall @LOCAL in 0..2 { m[LOCAL] = 0 } }", "action tick() { y = y }", "invariant M { m[2] == 0 }"),
            Safe,
        ),
        case(
            "leads-to-binder",
            invariant("leadsTo L { forall @LOCAL in 0..1 { forall z in 0..LOCAL { true ~> z <= 1 } } }"),
            Temporal,
        ),
        case(
            "requirements-transition-input",
            "requirements Shadow {\n  const N = CONSTANT\n  number Amount\n  process Case with value: Amount = 0 {\n    stages Idle, Done\n    initial Idle\n    transition act Idle -> Done by System with @LOCAL: Amount set value = LOCAL\n  }\n  invariant P { true }\n}\nverify { instances Case = 1 values Amount = 0..1 }\n".to_owned(),
            Safe,
        ),
        case(
            "time-age-binder",
            "requirements Shadow {\n  const N = CONSTANT\n  type K = 0..1\n  state { y: Int = 0 }\n  time { age waitAge[@LOCAL: K] while LOCAL == 0 }\n  requirement SLA \"bounded waiting\" { deadline waitAge <= 1 }\n  action stay() { y = y }\n}\n".to_owned(),
            Violated,
        ),
    ];
    for (id, original) in [
        ("requirements-parameter", "range-action-parameter"),
        ("requirements-let", "let"),
    ] {
        let c = cases
            .iter()
            .find(|c| c.id == original)
            .expect("ordinary action case");
        let source = c.source.replacen("spec Shadow", "requirements Shadow", 1);
        let outcome = c.outcome;
        cases.push(case(id, source, outcome));
    }
    cases
}

fn static_domain(source: &str) -> String {
    // Temporal wrapper filters are unsupported: membership restricts P instead.
    // Elsewhere a static superset plus where has the dependent range's meaning.
    source
        .replace(
            "forall z in 0..LOCAL { true ~> z <= 1 }",
            "forall z in 0..1 { z <= LOCAL ~> z <= 1 }",
        )
        .replace("z in 0..LOCAL {", "z in 0..1 where z <= LOCAL {")
        .replace("w in 0..LOCAL)", "w in 0..1 where w <= LOCAL)")
        .replace("y in 0..LOCAL)", "y in 0..1 where y <= LOCAL)")
}

fn authored(template: &str, constant: &str, name: &str) -> (String, Value) {
    let marked = template
        .replace("CONSTANT", constant)
        .replace("LOCAL", name);
    assert_eq!(
        marked.matches('@').count(),
        1,
        "one authored location marker"
    );
    let offset = marked.find('@').expect("authored binding marker");
    let prefix = &marked[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit('\n')
        .next()
        .expect("last source line")
        .chars()
        .count()
        + 1;
    (
        marked.replacen('@', "", 1),
        json!({"line": line, "column": column}),
    )
}

fn run(label: &str, source: &str, composed: bool) -> [(i32, Value); 4] {
    if composed {
        run_files(
            label,
            &[
                (
                    "case.fsl",
                    "compose System {\n  use Shadow as part from \"component.fsl\"\n}\n",
                ),
                ("component.fsl", source),
            ],
        )
    } else {
        run_files(label, &[("case.fsl", source)])
    }
}

fn run_files(label: &str, files: &[(&str, &str)]) -> [(i32, Value); 4] {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root");
    let directory = root
        .join(".codex/tmp/issue_1515_binder_const_shadow")
        .join(label);
    std::fs::create_dir_all(&directory).expect("create owned regression directory");
    for (name, source) in files {
        std::fs::write(directory.join(name), source).expect("write regression source");
    }
    let outputs = ROUTES.map(|route| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
        command.args([
            if route == "check" { "check" } else { "verify" },
            "case.fsl",
        ]);
        if route != "check" {
            command.args(["--depth", "3", "--no-cache", "--engine", route]);
        }
        let output = command
            .current_dir(&directory)
            .env("FSLC_CACHE", "off")
            .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
            .output()
            .expect("run native regression");
        assert!(output.stderr.is_empty(), "{label} {route}: {output:?}");
        let report = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{label} {route}: invalid JSON {error}: {output:?}"));
        (output.status.code().expect("native exit code"), report)
    });
    std::fs::write(
        directory.join(format!("observations-{}.json", std::process::id())),
        serde_json::to_vec_pretty(&outputs).expect("serialize complete observations"),
    )
    .expect("retain each run's observations");
    outputs
}

fn composed_variants(c: &Case) -> &'static [bool] {
    if c.source.starts_with("spec Shadow") {
        &[false, true]
    } else {
        &[false]
    }
}

fn label(prefix: &str, c: &Case, composed: bool, constant: &str) -> String {
    format!(
        "{prefix}-{constant}-{}{id}",
        if composed { "component-" } else { "" },
        id = c.id
    )
}

fn is_shadow_error(output: &(i32, Value), loc: &Value, component_loc: Option<&str>) -> bool {
    let (exit, report) = output;
    *exit == 2
        && report["result"] == "error"
        && report["kind"] == "semantics"
        && &report["loc"] == loc
        && report["message"].as_str().is_some_and(|message| {
            message.contains("shadows a constant")
                && message.contains("'N'")
                && component_loc.is_none_or(|origin| message.contains(origin))
        })
}

fn same_envelopes(outputs: &[(i32, Value); 4]) -> bool {
    outputs.iter().all(|output| output == &outputs[0])
}

fn detectors(constant: &str) {
    let mut failures = Vec::new();
    for c in cases() {
        for &composed in composed_variants(&c) {
            let id = label("reject", &c, composed, constant);
            // Component prefixing and aggregate normalization can reject a
            // dependent bound already. Use the admitted static spelling there,
            // and for Bool consts, so every detector isolates the name collision.
            let template = if composed || constant == "true" {
                static_domain(&c.source)
            } else {
                c.source.clone()
            };
            let (source, binding_loc) = authored(&template, constant, "N");
            let outputs = run(&id, &source, composed);
            let expected_loc = if composed {
                json!({"line": 2, "column": 3})
            } else {
                binding_loc.clone()
            };
            for (route, output) in ROUTES.iter().zip(&outputs) {
                let (exit, report) = output;
                let component_loc = format!(
                    "component.fsl:{}:{}",
                    binding_loc["line"], binding_loc["column"]
                );
                if !is_shadow_error(
                    output,
                    &expected_loc,
                    composed.then_some(component_loc.as_str()),
                ) {
                    failures.push(format!("{id} {route}: expected exit 2 semantics const-shadow error at {expected_loc}; produced exit {exit}, result {}, kind {}, loc {}, message {}", report["result"], report["kind"], report["loc"], report["message"]));
                }
            }
            // No projection, no exclusions: source refusal precedes routing.
            if outputs.iter().all(|(exit, _)| *exit == 2) && !same_envelopes(&outputs) {
                failures.push(format!("{id}: complete error envelopes differ"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failing cells:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn assert_control(id: &str, outcome: Outcome, outputs: &[(i32, Value); 4]) {
    for (route, (exit, report)) in ROUTES.iter().zip(outputs) {
        let expected = if *route == "check" {
            (0, "ok")
        } else if matches!(outcome, Outcome::Temporal) && *route == "explicit" {
            assert_eq!(report["kind"], "semantics", "{id}: {report}");
            assert_eq!(report["message"], TEMPORAL_UNSUPPORTED, "{id}: {report}");
            (2, "error")
        } else if matches!(outcome, Outcome::Violated) {
            (1, "violated")
        } else {
            (
                0,
                if *route == "bmc" {
                    "verified"
                } else {
                    "proved"
                },
            )
        };
        assert_eq!(
            (*exit, report["result"].as_str()),
            (expected.0, Some(expected.1)),
            "{id} {route}: {outputs:#?}"
        );
    }
}

#[test]
fn rejects_local_names_shadowing_integer_constants() {
    detectors("2");
}

#[test]
fn rejects_local_names_shadowing_non_integer_constants() {
    detectors("true");
}

#[test]
fn static_domain_controls_preserve_correct_verdicts() {
    for constant in ["2", "true"] {
        for c in cases() {
            for &composed in composed_variants(&c) {
                let id = label("control", &c, composed, constant);
                let (source, _) = authored(&static_domain(&c.source), constant, "k");
                assert_control(&id, c.outcome, &run(&id, &source, composed));
            }
        }
    }
}

#[test]
fn existing_pattern_constant_collision_stays_rejected() {
    // Existing pattern errors locate the owning declaration, not a new token.
    let template = spec(
        "",
        "opt: Option<Int> = some(1), y: Int = 0",
        "",
        "@action tick() { requires opt is some(N) y = N }",
        "invariant P { true }",
    );
    let (source, loc) = authored(&template, "2", "N");
    let outputs = run("existing-pattern-collision", &source, false);
    for output in &outputs {
        assert!(is_shadow_error(output, &loc, None), "{outputs:#?}");
    }
    assert!(
        same_envelopes(&outputs),
        "complete pattern refusal envelope: {outputs:#?}"
    );
    let renamed = source
        .replace("some(N)", "some(k)")
        .replace("y = N", "y = k");
    assert_control(
        "fresh-pattern",
        Outcome::Safe,
        &run("fresh-pattern", &renamed, false),
    );
}

#[test]
fn qualified_component_constants_do_not_collide_with_glue_or_sync_locals() {
    for sync in [false, true] {
        for (form, header, binding, guard) in [
            (
                "parameter",
                "N in 0..1",
                "",
                "forall z in 0..part.N { z <= 1 }",
            ),
            ("let", "", "let N = 1", "forall z in 0..part.N { z <= 1 }"),
            (
                "quantifier",
                "",
                "",
                "forall N in 0..1 { forall z in 0..part.N { z <= 1 } }",
            ),
        ] {
            let id = format!("qualified-{}-{form}", if sync { "sync" } else { "glue" });
            let target = if sync { " = part.idle()" } else { "" };
            let source = format!(
                "compose System {{\n  use Part as part from \"component.fsl\"\n  state {{ y: Int = 0 }}\n  action tick({header}){target} {{ {binding} requires {guard} y = 1 }}\n  invariant Z {{ y == 0 }}\n}}\n"
            );
            let outputs = run_files(
                &id,
                &[
                    ("case.fsl", &source),
                    (
                        "component.fsl",
                        "spec Part { const N = 2 state { x: Int = 0 } action idle() { x = x } }\n",
                    ),
                ],
            );
            assert_control(&id, Outcome::Safe, &outputs);
        }
    }
}
