// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::process::Command;
use std::task::{Context, Poll, Waker};

use fsl_core::{
    FsResolver, FslValue, KernelModel, ParamDef, TypeDef, TypeRef, build_model, parse_kernel_source,
};
use fsl_syntax::{DomainSpec, SurfaceDocument, SyntaxExpr, parse_surface_document};
use serde_json::{Map, Value, json};

const UPDATE_ENV: &str = "UPDATE_DOMAIN_CHARACTERIZATION";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/domain_characterization")
}

fn fixture(name: &str) -> PathBuf {
    corpus().join(name)
}

fn relative_fixture(name: &str) -> String {
    format!("rust/fslc/tests/fixtures/domain_characterization/{name}")
}

fn source(name: &str) -> String {
    std::fs::read_to_string(fixture(name)).expect("read characterization fixture")
}

fn domain(name: &str) -> DomainSpec {
    let SurfaceDocument::Domain(domain) =
        parse_surface_document(&source(name)).expect("parse domain fixture")
    else {
        panic!("expected domain fixture");
    };
    domain
}

fn rendered(expression: Option<&SyntaxExpr>) -> Option<String> {
    expression.map(SyntaxExpr::render_source)
}

fn rendered_all(expressions: &[SyntaxExpr]) -> Vec<String> {
    expressions.iter().map(SyntaxExpr::render_source).collect()
}

fn surface_projection(domain: &DomainSpec) -> Value {
    json!({
        "name":domain.name,
        "loc":domain.loc,
        "types":domain.types.iter().map(|ty|json!({
            "name":ty.name,"kind":ty.kind,"members":ty.members,"lo":rendered(ty.lo.as_ref()),"hi":rendered(ty.hi.as_ref()),
            "fields":ty.fields.iter().map(|field|json!({"name":field.name.text,"type":field.type_name.render_source(),"default":rendered(field.default.as_ref()),"loc":field.loc})).collect::<Vec<_>>(),
            "invariants":ty.invariants.iter().map(|item|json!({"name":item.name.text,"expr":item.expr.render_source(),"loc":item.loc})).collect::<Vec<_>>(),
            "loc":ty.loc
        })).collect::<Vec<_>>(),
        "aggregates":domain.aggregates.iter().map(|aggregate|json!({
            "name":aggregate.name,
            "state":aggregate.state.iter().map(|field|json!({"name":field.name.text,"type":field.type_name.render_source(),"default":rendered(field.default.as_ref()),"loc":field.loc})).collect::<Vec<_>>(),
            "decides":aggregate.decides.iter().map(|item|json!({
                "command":item.command,"requires":rendered_all(&item.requires),
                "rejects":item.rejects.iter().map(|reject|json!({"error":reject.error,"condition":reject.condition.render_source(),"loc":reject.loc})).collect::<Vec<_>>(),
                "emits":item.emits,"loc":item.loc
            })).collect::<Vec<_>>(),
            "evolves":aggregate.evolves.iter().map(|item|json!({
                "event":item.event,"requires":rendered_all(&item.requires),
                "assignments":item.assignments.iter().map(|assignment|json!({"target":assignment.target.render_source(),"expr":assignment.value.render_source(),"loc":assignment.loc})).collect::<Vec<_>>(),
                "loc":item.loc
            })).collect::<Vec<_>>(),
            "invariants":aggregate.invariants.iter().map(|item|json!({"name":item.name.text,"expr":item.expr.render_source(),"loc":item.loc})).collect::<Vec<_>>(),
            "stale_policies":aggregate.stale_policies.iter().map(|item|json!({"event":item.event,"condition":item.condition.render_source(),"emits":item.emits,"loc":item.loc})).collect::<Vec<_>>(),
            "loc":aggregate.loc
        })).collect::<Vec<_>>(),
        "effects":domain.effects.iter().map(|effect|json!({
            "name":effect.name,"idempotency_key":rendered(effect.idempotency_key.as_ref()),"correlation_id":rendered(effect.correlation_id.as_ref()),
            "handles":effect.handles,"outcomes":effect.outcomes,"timeout_event":effect.timeout_event,
            "retry_max_attempts":effect.retry.max_attempts,"loc":effect.loc
        })).collect::<Vec<_>>(),
        "sagas":domain.sagas.iter().map(|saga|json!({
            "name":saga.name,"starts_on":saga.starts_on,
            "steps":saga.steps.iter().map(|step|json!({"name":step.name,"requires":rendered_all(&step.requires),"emits":step.emits,"awaits":step.awaits,"timeout_event":step.timeout_event,"loc":step.loc})).collect::<Vec<_>>(),
            "invariants":saga.invariants.iter().map(|item|json!({"name":item.name.text,"expr":item.expr.render_source(),"loc":item.loc})).collect::<Vec<_>>(),
            "loc":saga.loc
        })).collect::<Vec<_>>()
    })
}

fn model(name: &str) -> KernelModel {
    let path = fixture(name);
    let kernel = parse_kernel_source(
        &source(name),
        &FsResolver::new(path.parent().expect("fixture directory")),
    )
    .expect("lower domain fixture");
    build_model(kernel).expect("build domain fixture")
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}

fn type_ref_json(ty: &TypeRef) -> Value {
    match ty {
        TypeRef::Int => json!({"kind":"int"}),
        TypeRef::Bool => json!({"kind":"bool"}),
        TypeRef::Named(name) => json!({"kind":"named","name":name}),
        TypeRef::Range(lo, hi) => json!({"kind":"range","lo":lo,"hi":hi}),
        TypeRef::Map(key, value) => {
            json!({"kind":"map","key":type_ref_json(key),"value":type_ref_json(value)})
        }
        TypeRef::Relation(source, target) => json!({
            "kind":"relation",
            "source":type_ref_json(source),
            "target":type_ref_json(target)
        }),
        TypeRef::Set(item) => json!({"kind":"set","item":type_ref_json(item)}),
        TypeRef::Seq(item, capacity) => {
            json!({"kind":"seq","item":type_ref_json(item),"capacity":capacity})
        }
        TypeRef::Option(item) => json!({"kind":"option","item":type_ref_json(item)}),
    }
}

fn type_def_json(definition: &TypeDef) -> Value {
    match definition {
        TypeDef::Domain { lo, hi, symmetric } => {
            json!({"kind":"domain","lo":lo,"hi":hi,"symmetric":symmetric})
        }
        TypeDef::Enum { members, symmetric } => {
            json!({"kind":"enum","members":members,"symmetric":symmetric})
        }
        TypeDef::Struct { fields } => json!({
            "kind":"struct",
            "fields":fields.iter().map(|(name,ty)|json!({"name":name,"type":type_ref_json(ty)})).collect::<Vec<_>>()
        }),
    }
}

fn param_json(param: &ParamDef) -> Value {
    match param {
        ParamDef::Typed { name, ty } => {
            json!({"kind":"typed","name":name,"type":type_ref_json(ty)})
        }
        ParamDef::Range { name, lo, hi } => {
            json!({"kind":"range","name":name,"lo":lo,"hi":hi})
        }
    }
}

fn erase_locations(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(erase_locations).collect()),
        Value::Object(values)
            if values.len() == 2
                && values.contains_key("line")
                && values.contains_key("column") =>
        {
            Value::String("<location>".to_owned())
        }
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, erase_locations(value)))
                .collect(),
        ),
        other => other,
    }
}

fn value_map(state: &BTreeMap<String, FslValue>) -> Value {
    Value::Object(
        state
            .iter()
            .map(|(name, value)| (name.clone(), fslc_rust::fsl_value_json(value)))
            .collect(),
    )
}

fn semantic_model(model: &KernelModel) -> Value {
    let types = model
        .types
        .iter()
        .map(|(name, definition)| (name.clone(), type_def_json(definition)))
        .collect::<Map<_, _>>();
    let mut state = model
        .state
        .iter()
        .map(|(name, ty)| json!({"name":name,"type":type_ref_json(ty)}))
        .collect::<Vec<_>>();
    state.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    let initial = fsl_runtime::Monitor::new(model.clone()).expect("construct initial Monitor");
    let mut actions = model
        .actions
        .iter()
        .map(|action| {
            json!({
                "name":action.name,
                "parameters":action.params.iter().map(param_json).collect::<Vec<_>>(),
                "requires":action.requires.iter().map(|expr|erase_locations(expr.python_ast())).collect::<Vec<_>>(),
                "lets":action.lets.iter().map(|(name,expr)|json!([name,erase_locations(expr.python_ast())])).collect::<Vec<_>>(),
                "updates":action.statements.iter().map(|statement|erase_locations(statement.python_ast())).collect::<Vec<_>>(),
                "ensures":action.ensures.iter().map(|expr|erase_locations(expr.python_ast())).collect::<Vec<_>>(),
                "fair":action.fair
            })
        })
        .collect::<Vec<_>>();
    actions.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    let mut invariants = model
        .invariants
        .iter()
        .map(|property| {
            json!({"name":property.name,"expression":erase_locations(property.expr.python_ast())})
        })
        .collect::<Vec<_>>();
    invariants.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    json!({
        "name":model.name,
        "types":types,
        "state":state,
        "initial_state":value_map(&initial.state),
        "actions":actions,
        "invariants":invariants,
        "terminal":model.terminal.as_ref().map(|expr|erase_locations(expr.python_ast()))
    })
}

fn normalize_files(value: &mut Value, file_name: &str) {
    match value {
        Value::Array(values) => {
            for value in values {
                normalize_files(value, file_name);
            }
        }
        Value::Object(values) => {
            if values.contains_key("file") {
                values.insert("file".to_owned(), Value::String(file_name.to_owned()));
            }
            for value in values.values_mut() {
                normalize_files(value, file_name);
            }
        }
        _ => {}
    }
}

fn property_projection(items: &Value) -> Vec<Value> {
    items
        .as_array()
        .expect("properties")
        .iter()
        .filter(|item| {
            !item["name"]
                .as_str()
                .unwrap_or_default()
                .starts_with("_bounds_")
        })
        .map(|item| {
            json!({
                "name":item["name"],
                "source_kind":item["source_kind"],
                "expression":item["expression"],
                "origin":item["origin"],
                "span":item["span"]
            })
        })
        .collect()
}

fn public_projection(name: &str) -> Value {
    let input = source(name);
    let path = fixture(name);
    let kernel = parse_kernel_source(
        &input,
        &FsResolver::new(path.parent().expect("fixture directory")),
    )
    .expect("lower public Kernel fixture");
    let checked = build_model(kernel.clone()).expect("build public Kernel fixture");
    let mut contract = fsl_core::public_kernel_contract(&kernel, &checked, name, "domain")
        .expect("export public Kernel");
    normalize_files(&mut contract, name);
    let representative_actions = match name {
        "expressions_valid.fsl" => ["order_approve", "order_cancel"].as_slice(),
        "effect_saga_valid.fsl" => [
            "order_approve",
            "order_request_payment",
            "capture_payment_complete_payment_captured",
            "saga_payment_flow_capture",
        ]
        .as_slice(),
        _ => &[],
    };
    let properties = &contract["properties"];
    let init = &contract["init"];
    json!({
        "schema_version":contract["schema_version"],
        "spec":contract["spec"],
        "state":contract["state"].as_array().expect("state").iter().map(|item|json!({"name":item["name"],"type":item["type"],"origin":item["origin"]})).collect::<Vec<_>>(),
        "init":{
            "statements":init["statements"],
            "origin":init["origin"],
            "span":init["span"]
        },
        "actions":contract["actions"].as_array().expect("actions").iter().filter(|action|representative_actions.contains(&action["name"].as_str().unwrap_or_default())).map(|action|json!({
            "name":action["name"],
            "fair":action["fair"],
            "guards":action["guards"],
            "updates":action["updates"],
            "origin":action["origin"],
            "span":action["span"]
        })).collect::<Vec<_>>(),
        "properties":{
            "invariants":property_projection(&properties["invariants"]),
            "transitions":property_projection(&properties["transitions"]),
            "terminal":properties["terminal"]
        }
    })
}

fn generated_fragments(domain: &DomainSpec, needles: &[&str]) -> Value {
    let source = fsl_core::domain_kernel_source(domain);
    Value::Array(
        needles
            .iter()
            .map(|needle| {
                source
                    .lines()
                    .find(|line| line.contains(needle))
                    .unwrap_or_else(|| panic!("generated Kernel is missing '{needle}'"))
                    .trim()
                    .to_owned()
                    .into()
            })
            .collect(),
    )
}

fn run_cli(args: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .current_dir(workspace())
        .args(args)
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse CLI JSON for {args:?}: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().expect("fslc exit code"), value)
}

fn scrub_cli(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                scrub_cli(value);
            }
        }
        Value::Object(values) => {
            for key in [
                "cache",
                "cost",
                "hint",
                "note",
                "recommended_action",
                "warnings",
            ] {
                values.remove(key);
            }
            for value in values.values_mut() {
                scrub_cli(value);
            }
        }
        _ => {}
    }
}

fn cli_snapshot(args: &[&str]) -> Value {
    let (status, mut output) = run_cli(args);
    scrub_cli(&mut output);
    json!({"exit":status,"output":output})
}

fn direct_span_note(name: &str, action_name: &str, source_declaration: &str) -> Value {
    let contract = public_projection(name);
    let action = contract["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .find(|action| action["name"] == action_name)
        .expect("generated action");
    let source_line = source(name)
        .lines()
        .position(|line| line.trim() == source_declaration)
        .unwrap_or_else(|| panic!("missing source declaration '{source_declaration}'"))
        + 1;
    json!({
        "classification":"original_domain_coordinate_preserved",
        "file":name,
        "source_declaration_line":source_line,
        "reported_public_kernel_line":action["span"]["line"],
        "origin":action["origin"]
    })
}

#[allow(clippy::too_many_lines)]
fn baseline() -> Value {
    let expressions = domain("expressions_valid.fsl");
    let lvalues = domain("lvalues_surface.fsl");
    let effects = domain("effect_saga_valid.fsl");
    let expression_model = model("expressions_valid.fsl");
    let effect_model = model("effect_saga_valid.fsl");
    let mut cli = Map::new();
    for name in [
        "expressions_valid.fsl",
        "effect_saga_valid.fsl",
        "lvalues_surface.fsl",
        "legacy_logical_parse_error.fsl",
        "invalid_unknown_name.fsl",
        "invalid_unknown_member.fsl",
        "invalid_type_mismatch.fsl",
        "invalid_operator.fsl",
        "invalid_broken_expression.fsl",
        "ai_internal_name_misuse.fsl",
    ] {
        let relative = relative_fixture(name);
        cli.insert(
            name.to_owned(),
            json!({
                "check":cli_snapshot(&["check", &relative]),
                "verify":cli_snapshot(&["verify", &relative, "--depth", "2"])
            }),
        );
    }
    json!({
        "spdx":"Apache-2.0",
        "schema_version":"domain-expression-characterization.v1",
        "surface_ast":{
            "expressions_valid.fsl":surface_projection(&expressions),
            "lvalues_surface.fsl":surface_projection(&lvalues),
            "effect_saga_valid.fsl":surface_projection(&effects)
        },
        "semantic_kernel_model":{
            "expressions_valid.fsl":semantic_model(&expression_model),
            "effect_saga_valid.fsl":semantic_model(&effect_model)
        },
        "public_kernel":{
            "expressions_valid.fsl":public_projection("expressions_valid.fsl"),
            "effect_saga_valid.fsl":public_projection("effect_saga_valid.fsl")
        },
        "direct_domain_spans":[
            direct_span_note("expressions_valid.fsl","order_approve","decide Approve {"),
            direct_span_note("effect_saga_valid.fsl","order_approve","decide Approve {")
        ],
        "generated_kernel_source_fragments":{
            "expressions_valid.fsl":generated_fragments(&expressions,&[
                "requires order_status == OrderStatus_Draft and not",
                "requires order_status != OrderStatus_Cancelled and order_quantity >= 0",
                "order_audit.status = order_status",
                "Order_legacyImplication",
                "Order_legacyDisjunction",
                "Order_finiteMembership"
            ]),
            "effect_saga_valid.fsl":generated_fragments(&effects,&[
                "action capture_payment_complete_payment_captured",
                "requires event_Approved and not event_PaymentFailed",
                "PaymentFlow_terminalOutcome",
                "CapturePayment_SuccessSticky"
            ]),
            "lvalues_surface.fsl":generated_fragments(&lvalues,&[
                "inventory_total = inventory_total + 1",
                "inventory_counts[item] = inventory_counts[item] + 1",
                "inventory_counter.value = inventory_total"
            ])
        },
        "cli":Value::Object(cli)
    })
}

#[test]
fn domain_surface_lowering_public_kernel_and_diagnostics_match_baseline() {
    let mut actual = baseline();
    let path = fixture("baseline.v1.json");
    if std::env::var_os(UPDATE_ENV).is_some() {
        std::fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&actual).expect("serialize baseline")
            ),
        )
        .expect("write characterization baseline");
        return;
    }
    let mut expected: Value = serde_json::from_str(
        &std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing {}; run with {UPDATE_ENV}=1", path.display())),
    )
    .expect("parse characterization baseline");
    // JSON object member order is not part of this semantic characterization.
    // The separately built native CLI may serialize object members in a
    // different order across targets without changing any JSON value.
    actual.sort_all_objects();
    expected.sort_all_objects();
    assert_eq!(actual, expected);
}

#[test]
fn domain_monitor_and_symbolic_semantics_agree() {
    let mut total_checked = 0_usize;
    let mut rejected_changed_successor = false;
    for name in [
        "expressions_valid.fsl",
        "effect_saga_valid.fsl",
        "lvalues_surface.fsl",
    ] {
        let model = model(name);
        let initial = fsl_runtime::Monitor::new(model.clone()).expect("create Monitor");
        let mut queue = VecDeque::from([(initial, 0_usize)]);
        let mut seen = BTreeSet::new();
        while let Some((monitor, depth)) = queue.pop_front() {
            if !seen.insert(monitor.state.clone()) {
                continue;
            }
            for property in &model.invariants {
                let expected = fsl_runtime::eval(
                    &property.expr,
                    &monitor.state,
                    &mut BTreeMap::new(),
                    &model,
                    None,
                )
                .expect("evaluate invariant concretely");
                let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
                assert!(
                    block_on(fsl_verifier::expression_matches_value(
                        &model,
                        &mut solver,
                        &property.expr,
                        &monitor.state,
                        &expected,
                    ))
                    .expect("check expression agreement"),
                    "{} disagreed in {name}",
                    property.name
                );
            }
            for enabled in monitor.enabled().expect("enumerate actions") {
                let current = monitor.state.clone();
                let mut successor = monitor.clone();
                let result = successor.step(&enabled).expect("step Monitor");
                if result.violation.is_some() {
                    continue;
                }
                let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
                assert!(
                    block_on(fsl_verifier::transition_matches_step(
                        &model,
                        &mut solver,
                        &current,
                        &enabled.action,
                        &enabled.params,
                        &result.state,
                    ))
                    .expect("check transition agreement"),
                    "{} disagreed in {name}",
                    enabled.action
                );
                total_checked += 1;
                if !rejected_changed_successor {
                    let mut wrong = result.state.clone();
                    if let Some((_, FslValue::Bool(value))) = wrong
                        .iter_mut()
                        .find(|(_, value)| matches!(value, FslValue::Bool(_)))
                    {
                        *value = !*value;
                        let mut solver =
                            fsl_solver_z3::Z3Solver::new().expect("create rejection solver");
                        assert!(
                            !block_on(fsl_verifier::transition_matches_step(
                                &model,
                                &mut solver,
                                &current,
                                &enabled.action,
                                &enabled.params,
                                &wrong,
                            ))
                            .expect("reject altered successor")
                        );
                        rejected_changed_successor = true;
                    }
                }
                if depth < 2 {
                    queue.push_back((successor, depth + 1));
                }
            }
        }
    }
    assert!(
        total_checked >= 12,
        "corpus exercised only {total_checked} transitions"
    );
    assert!(rejected_changed_successor);
}

#[test]
fn ai_native_prompt_attempt_metrics_match_baseline() {
    let manifest: Value = serde_json::from_str(&source("ai_native_cases.v1.json"))
        .expect("parse AI-native corpus manifest");
    let cases = manifest["cases"].as_array().expect("cases");
    let mut initial_successes = 0_usize;
    let mut repairs = Vec::new();
    let mut operator_misuse = 0_i64;
    let mut enum_misuse = 0_i64;
    let mut internal_name_misuse = 0_i64;
    let mut diagnostic_hits = 0_usize;
    let mut diagnostic_total = 0_usize;
    for case in cases {
        let attempts = case["attempts"].as_array().expect("attempts");
        let mut first_success = None;
        let mut first_failure = None;
        for (index, attempt) in attempts.iter().enumerate() {
            let relative = relative_fixture(attempt.as_str().expect("attempt path"));
            let (status, output) = run_cli(&["check", &relative]);
            if index == 0 && status != 0 {
                first_failure = Some(output);
            }
            if status == 0 {
                first_success.get_or_insert(index);
                break;
            }
        }
        let repair_count = first_success.unwrap_or(attempts.len());
        if repair_count == 0 {
            initial_successes += 1;
        }
        repairs.push(repair_count);
        operator_misuse += case["misuse"]["operator"].as_i64().expect("operator count");
        enum_misuse += case["misuse"]["enum"].as_i64().expect("enum count");
        internal_name_misuse += case["misuse"]["internal_generated_name"]
            .as_i64()
            .expect("generated-name count");
        if let Some(line) = case
            .get("diagnostic_expression_line")
            .and_then(Value::as_u64)
        {
            diagnostic_total += 1;
            let message = first_failure
                .as_ref()
                .and_then(|output| output["message"].as_str())
                .unwrap_or_default();
            if message.contains(&format!("at {line}:")) {
                diagnostic_hits += 1;
            }
        }
    }
    let actual = json!({
        "case_count":cases.len(),
        "initial_check_successes":initial_successes,
        "initial_check_success_rate":format!("{initial_successes}/{}",cases.len()),
        "repairs_until_check_success":repairs,
        "operator_misuse_count":operator_misuse,
        "enum_misuse_count":enum_misuse,
        "internal_generated_name_misuse_count":internal_name_misuse,
        "diagnostic_expression_hits":diagnostic_hits,
        "diagnostic_expression_total":diagnostic_total,
        "diagnostic_expression_hit_rate":format!("{diagnostic_hits}/{diagnostic_total}")
    });
    assert_eq!(actual, manifest["baseline"]);
}
