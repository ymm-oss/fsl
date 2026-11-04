// SPDX-License-Identifier: Apache-2.0
use fsl_core::FslValue;
use fsl_runtime::BoundedLivenessMonitor;
use serde_json::{Value, json};
use std::collections::BTreeMap;

// The frozen producer owns general CLI/raw-packet and Monitor construction.
// Its unrelated static matrix/source helpers are intentionally unused here.
#[allow(dead_code)]
#[path = "issue_1480_remaining_static_support.rs"]
pub mod frozen;
pub use frozen::{cli, concrete, note};

pub fn model(n: i64, property: &str) -> String {
    format!(
        "spec A {{ type K=0..2 state {{ n:K }} init {{ n={n} }} action go() {{}} invariant P {{ {property} }} }}"
    )
}

pub fn exercise(src: &str, bad: bool, errors: &mut Vec<String>) {
    cli(src, bad, false, errors);
    concrete(src, errors, |m, e| {
        let observed = m.current_violation();
        note(
            e,
            &format!("Monitor initial violation full Result success: {src}"),
            observed.as_ref().is_ok_and(|v| match v {
                Some(v) => bad && v.kind == "invariant" && v.name == "P",
                None => !bad,
            }),
            &observed,
        );
    });
}

pub fn full_state(m: &fsl_runtime::Monitor) -> Value {
    fn value(v: &FslValue) -> Value {
        match v {
            FslValue::Int(i) => json!(i),
            FslValue::Bool(b) => json!(b),
            FslValue::Map(m) => Value::Object(
                m.iter()
                    .map(|(k, v)| {
                        let FslValue::Int(k) = k else {
                            panic!("noninteger test key")
                        };
                        (k.to_string(), value(v))
                    })
                    .collect(),
            ),
            _ => panic!("unexpected test state value {v:?}"),
        }
    }
    Value::Object(m.state.iter().map(|(k, v)| (k.clone(), value(v))).collect())
}

pub fn fill(src: &str, errors: &mut Vec<String>) {
    cli(src, false, false, errors);
    for (lim, expected, fixed_accepts) in [
        (0, json!({"m":{"0":true,"1":false,"2":false}}), false),
        (1, json!({"m":{"0":true,"1":true,"2":false}}), true),
        (2, json!({"m":{"0":true,"1":true,"2":true}}), false),
    ] {
        for fixed in [false, true] {
            let bound = if fixed { "0..1" } else { "0..lim" };
            let fixture = src.replace("0..lim", bound);
            concrete(&fixture, errors, |m, e| {
                let fresh = full_state(m);
                let params = BTreeMap::from([("lim".into(), FslValue::Int(lim))]);
                let result = m.attempt("fill", &params);
                let success = result.as_ref().is_ok_and(|r| r.violation.is_none());
                let actual = full_state(m);
                let accepted = success && actual == expected;
                let expected_accept = !fixed || fixed_accepts;
                let record = json!({"route":"monitor-fill","source":fixture,"params":{"lim":lim},"fixed":fixed,"fresh":fresh,"expected":expected,"actual":actual,"success":success,"result":format!("{result:?}"),"accepted":accepted,"expected_accept":expected_accept});
                eprintln!("FILL_RECORD {record}");
                let pristine = fresh == json!({"m":{"0":false,"1":false,"2":false}});
                let ok = pristine && success && accepted == expected_accept;
                note(e, "fresh literal fill / fixed control", ok, record);
            });
        }
    }
}

pub fn rhs(src: &str, expected: &Value, bad: bool, errors: &mut Vec<String>) {
    cli(src, bad, false, errors);
    concrete(src, errors, |m, e| {
        note(
            e,
            &format!("init RHS complete state: {src}"),
            &full_state(m) == expected,
            full_state(m),
        );
    });
}

pub fn temporal(src: &str, bad: bool, errors: &mut Vec<String>) {
    let observations = cli(src, bad, true, errors);
    for (route, p) in observations {
        if bad && matches!(route.as_str(), "bmc" | "induction") {
            note(
                errors,
                "symbolic selected binding and deadline",
                p["violation_kind"] == "leadsTo"
                    && p["bindings"] == json!({"i":1,"j":1})
                    && p["pending_since"] == 0
                    && p["deadline"] == 1
                    && p["within"] == 1,
                &p,
            );
        }
    }
    concrete(src, errors, |m, e| {
        let Ok(mut live) = BoundedLivenessMonitor::new(m.model.clone())
            .inspect_err(|v| note(e, "liveness construction gap", false, v))
        else {
            return;
        };
        let first = live.observe(&m.state, 0);
        note(
            e,
            &format!("initial temporal observation full Result success: {src}"),
            matches!(first, Ok(None)),
            &first,
        );
        let bindings = [(0, 0), (1, 0), (1, 1)].map(|(i, j)| {
            BTreeMap::from([
                ("i".into(), FslValue::Int(i)),
                ("j".into(), FslValue::Int(j)),
            ])
        });
        let action = m.attempt("go", &BTreeMap::new());
        note(
            e,
            "go full Result success",
            action.as_ref().is_ok_and(|r| r.violation.is_none()),
            &action,
        );
        let observed = live.observe(&m.state, 1);
        note(
            e,
            &format!("temporal observation full Result success: {src}"),
            observed.is_ok(),
            &observed,
        );
        let deciding = observed
            .as_ref()
            .ok()
            .and_then(|v| v.as_ref())
            .map(|v| (&v.bindings, v.pending_since, v.deadline, v.within, v.step));
        let expected = bad.then_some((&bindings[2], 0, 1, 1, 1));
        note(
            e,
            "selected (1,1) exact deadline; Err never equals None",
            observed.is_ok() && deciding == expected,
            &observed,
        );
    });
    let (exit, packet) = probe(
        src,
        "replay",
        &json!({"$schema":"https://fsl.dev/schemas/fslc/kernel/replay-trace.v1.schema.json","schema_version":"1.2.0","kernel_schema_version":"1.0.0","spec":"A","initial":{"d":false},"events":[]}),
    );
    note(
        errors,
        "three ordered pending replay bindings",
        exit == 0
            && packet["checks"]["bounded_liveness"]["pending"]
                == json!([{"property":"P","bindings":{"i":0,"j":0},"pending_since":0,"deadline":1,"within":1},{"property":"P","bindings":{"i":1,"j":0},"pending_since":0,"deadline":1,"within":1},{"property":"P","bindings":{"i":1,"j":1},"pending_since":0,"deadline":1,"within":1}]),
        &packet,
    );
}

pub fn partial(src: &str, reached: bool, errors: &mut Vec<String>) {
    // A runtime partial_op is NOT interchangeable with an arbitrary engine error.
    let observations = cli(src, reached, false, errors);
    concrete(src, errors, |m, e| {
        let result = m.attempt("go", &BTreeMap::new());
        note(
            e,
            &format!("partial action full Result success: {src}"),
            result.is_ok(),
            &result,
        );
        let violation = result.as_ref().ok().and_then(|r| r.violation.as_ref());
        note(
            e,
            &format!("exact runtime partial_op reachability: {src}"),
            result.is_ok()
                && if reached {
                    violation.is_some_and(|v| v.kind == "partial_op")
                } else {
                    violation.is_none()
                },
            &result,
        );
    });
    for (route, p) in observations {
        if reached && matches!(route.as_str(), "bmc" | "induction" | "explicit") {
            note(
                errors,
                "reached symbolic partial_op, not generic error",
                p["result"] == "violated" && p["violation_kind"] == "partial_op",
                p,
            );
        }
    }
}

// Focused extra public routes only; five-route framework remains frozen/reused.
pub fn probe(src: &str, route: &str, trace: &Value) -> (i32, Value) {
    use sha2::{Digest, Sha256};
    use std::{
        process::Command,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("dynamic-probe-{}-{id}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("model.fsl"), src).unwrap();
    std::fs::write(dir.join("trace.json"), trace.to_string()).unwrap();
    let binary = env!("CARGO_BIN_EXE_fslc");
    let mut args = vec![route, "model.fsl"];
    if route == "replay" {
        args.extend(["--trace", "trace.json"]);
    }
    let request = json!({"source":src,"source_sha256":format!("{:x}",Sha256::digest(src.as_bytes())),"route":route,"argv":args,"trace":trace,"binary":binary,"binary_sha256":format!("{:x}",Sha256::digest(std::fs::read(binary).unwrap())),"cwd":dir,"environment_overrides":{"FSLC_CACHE":"off","FSLC_UNSTABLE":null}});
    std::fs::write(dir.join("request.json"), request.to_string()).unwrap();
    let out = Command::new(binary)
        .args(args)
        .current_dir(&dir)
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_UNSTABLE")
        .output()
        .unwrap();
    let packet: Value = serde_json::from_slice(&out.stdout).expect("public JSON");
    let exit = out.status.code().unwrap();
    let record = json!({"request":request,"exit":exit,"stdout":String::from_utf8_lossy(&out.stdout),"stderr":String::from_utf8_lossy(&out.stderr),"packet":packet});
    std::fs::write(dir.join("observation.json"), record.to_string()).unwrap();
    (exit, packet)
}
