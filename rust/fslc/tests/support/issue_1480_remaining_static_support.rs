// SPDX-License-Identifier: Apache-2.0
use fsl_core::{FsResolver, build_model, parse_kernel_source};
use fsl_runtime::Monitor;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    process::Command,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

static NEXT: AtomicU64 = AtomicU64::new(0);
static BINARY_HASH: OnceLock<String> = OnceLock::new();
pub const BODY_CASES: &[(&str, bool)] = &[
    ("forall i in 1..0 { 1/i > 0 }", false),
    ("forall i in 0..(1+0) where false { 1/0 > 0 }", false),
    ("forall i in 0..(1-1) { 1/i > 0 }", true),
];
// Hand-calculated inclusive candidates and Euclidean sums; never production folding.
pub const ROWS: &[(&str, &[i64], i64)] = &[
    ("0..1", &[0, 1], 1),
    ("0..N", &[0, 1], 1),
    ("-1..(2-1)", &[-1, 0, 1], 0),
    ("0..(1+0)", &[0, 1], 1),
    ("0..(2*2)", &[0, 1, 2, 3, 4], 10),
    ("0..(5/2)", &[0, 1, 2], 3),
    ("(-5/2)..-1", &[-3, -2, -1], -6),
    ("(-5/-2)..3", &[3], 3),
    ("(5/-2)..-1", &[-2, -1], -3),
    ("0..(5%2)", &[0, 1], 1),
    ("0..(-5%2)", &[0, 1], 1),
    ("0..(5%-2)", &[0, 1], 1),
    ("0..(-5%-2)", &[0, 1], 1),
    ("0..min(2,1)", &[0, 1], 1),
    ("0..max(-1,1)", &[0, 1], 1),
    ("0..abs(-1)", &[0, 1], 1),
    ("0..(if true then 1 else 0)", &[0, 1], 1),
    ("0..(if false then (1/0) else 1)", &[0, 1], 1),
    ("0..(if not false and 2>=1 then 1 else 0)", &[0, 1], 1),
    ("1..0", &[], 0),
    ("MAX..MAX", &[i64::MAX], i64::MAX),
    ("MIN..MIN", &[i64::MIN], i64::MIN),
    ("0..(if true then 1 else (1/0))", &[0, 1], 1),
    ("0..(if false then 0 else 1)", &[0, 1], 1),
];

pub fn note(errors: &mut Vec<String>, label: &str, ok: bool, actual: impl std::fmt::Debug) {
    eprintln!("ORACLE {label}: matches={ok} produced={actual:?}");
    if !ok {
        errors.push(format!("{label}: {actual:?}"));
    }
}

pub fn source(init: &str, action: &str, property: &str, state: &str) -> String {
    format!("spec A {{ const N = 1 const MAX = 9223372036854775807 const MIN = (-9223372036854775807-1) {state} init {{ {init} }} action go() {{ {action} }} {property} }}").replace("-9223372036854775808", "MIN")
}

pub fn simple(property: &str) -> String {
    source("d=false", "d=not d", property, "state { d: Bool }")
}

fn temporal_packet(route: &str, exit: i32, p: &Value, errors: &mut Vec<String>) {
    if route == "explicit" {
        let want = json!({"fsl":"1.0","result":"error","kind":"semantics","message":"the explicit engine does not support leadsTo properties; use --engine bmc or exclude the leadsTo property","versions":{"verifier":{"name":"fslc-rust","version":env!("CARGO_PKG_VERSION")},"core":{"name":"fsl-core","version":env!("CARGO_PKG_VERSION")},"solver":{"name":"z3","backend":"native-z3","version":"Z3 4.16.0.0"}}});
        note(
            errors,
            "explicit unsupported full envelope",
            exit == 2 && p == &want,
            p,
        );
    } else if exit == 0 && matches!(route, "bmc" | "induction") {
        if route == "induction" {
            note(
                errors,
                "bounded induction note",
                p.get("note")
                    == Some(&json!(
                        "invariants proved for all depths; leadsTo checked to depth 3 only"
                    )),
                p,
            );
        }
        note(
            errors,
            "property keyed bounded metadata",
            p.get("leads_to") == Some(&json!({"P":{"checked_to_depth":3,"within":1}})),
            p,
        );
    }
}

pub fn cli(src: &str, bad: bool, temporal: bool, errors: &mut Vec<String>) -> Vec<(String, Value)> {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("remaining-static-r2-{}-{id}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("model.fsl"), src).unwrap();
    let trace = if temporal {
        json!({"$schema":"https://fsl.dev/schemas/fslc/kernel/replay-trace.v1.schema.json","schema_version":"1.2.0","kernel_schema_version":"1.0.0","spec":"A","initial":{"d":false},"events":[{"tick":1,"action":"go","params":{},"state":{"d":true}}]})
    } else {
        json!([{"action":"go","params":{}}])
    };
    std::fs::write(dir.join("trace.json"), trace.to_string()).unwrap();
    let binary = env!("CARGO_BIN_EXE_fslc");
    let binary_hash =
        BINARY_HASH.get_or_init(|| format!("{:x}", Sha256::digest(std::fs::read(binary).unwrap())));
    let mut observations = Vec::new();
    let mut requests = Vec::new();
    for route in ["check", "bmc", "induction", "explicit", "replay"] {
        let args = match route {
            "check" => vec!["check", "model.fsl"],
            "replay" => vec!["replay", "model.fsl", "--trace", "trace.json"],
            _ => vec![
                "verify",
                "model.fsl",
                "--engine",
                route,
                "--depth",
                "3",
                "--no-cache",
            ],
        };
        requests.push(json!({"route":route,"argv":args}));
    }
    let request = json!({"case":id,"source":src,"source_sha256":format!("{:x}",Sha256::digest(src.as_bytes())),"trace":trace,"binary":binary,"binary_sha256":binary_hash,"cwd":dir,"environment_overrides":{"FSLC_CACHE":"off"},"requests":requests,"bad":bad,"temporal":temporal});
    // Independent request records precede CLI observations, outside observed dirs.
    let plans = std::env::temp_dir().join(format!("plans-{}", std::process::id()));
    std::fs::create_dir_all(&plans).unwrap();
    std::fs::write(plans.join(format!("{id}.json")), request.to_string()).unwrap();
    for request_route in requests {
        let route = request_route["route"].as_str().unwrap();
        let args = request_route["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>();
        let out = Command::new(binary)
            .args(&args)
            .current_dir(&dir)
            .env("FSLC_CACHE", "off")
            .output()
            .unwrap();
        let packet: Value = serde_json::from_slice(&out.stdout).expect("public JSON");
        let expected = match route {
            "check" => (0, "ok"),
            "explicit" if temporal => (2, "error"),
            "replay" if bad => (1, "nonconformant"),
            "replay" => (0, "conformant"),
            _ if bad => (1, "violated"),
            "bmc" => (0, "verified"),
            _ => (0, "proved"),
        };
        let record = json!({"request":request,"route":route,"argv":args,"exit":out.status.code(),"stdout":String::from_utf8_lossy(&out.stdout),"stderr":String::from_utf8_lossy(&out.stderr),"packet":packet,"expected":expected});
        std::fs::write(dir.join(format!("{route}.json")), record.to_string()).unwrap();
        let ok = out.status.code() == Some(expected.0)
            && packet.get("result").and_then(Value::as_str) == Some(expected.1)
            && out.stderr.is_empty();
        note(errors, &format!("{}/{route}", dir.display()), ok, &packet);
        if temporal {
            temporal_packet(route, out.status.code().unwrap(), &packet, errors);
        }
        observations.push((route.into(), packet));
    }
    observations
}

pub fn concrete(
    src: &str,
    errors: &mut Vec<String>,
    inspect: impl FnOnce(&mut Monitor, &mut Vec<String>),
) {
    let result = parse_kernel_source(src, &FsResolver::new("."))
        .map_err(|e| format!("{e:?}"))
        .and_then(|p| build_model(p).map_err(|e| format!("{e:?}")))
        .and_then(|m| Monitor::new(m).map_err(|e| format!("{e:?}")));
    match result {
        Ok(mut m) => inspect(&mut m, errors),
        Err(e) => note(
            errors,
            "admission/normalization or initialization gap (not numeric proof)",
            false,
            (src, e),
        ),
    }
}
