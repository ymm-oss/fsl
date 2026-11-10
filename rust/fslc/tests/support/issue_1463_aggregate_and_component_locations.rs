// SPDX-License-Identifier: Apache-2.0

// Shared aggregate source-typing and component-location fixtures.

use std::path::{Path, PathBuf};

pub struct Case {
    pub id: String,
    pub source: String,
    pub files: Vec<(String, String)>,
    pub rejection: bool,
    pub negative: bool,
    pub public_name: Option<&'static str>,
    pub marker: &'static str,
}

pub fn unchecked_init_collection_model() -> (fsl_core::KernelModel, String) {
    let source = "spec UncheckedInit { type K = 0..1 state { s: Set<K>, done: Bool } init { s = Set { 0, 1 } forall i in s {} done = false } action go() { done = true } invariant Capture { not done } }";
    let resolver = fsl_core::FsResolver::new(".");
    let expected =
        fsl_core::build_model(fsl_core::parse_kernel_source(source, &resolver).expect("parse"))
            .expect_err("source must reject state collection")
            .message;
    let valid = source.replace("forall i in s", "forall i in Set { 0, 1 }");
    let mut model = fsl_core::build_model(
        fsl_core::parse_kernel_source(&valid, &resolver).expect("parse literal"),
    )
    .expect("literal model");
    let fsl_core::KernelStatement::ForAll {
        binder: fsl_core::KernelBinder::Collection { collection, .. },
        ..
    } = &mut model.init[1]
    else {
        panic!("forall source")
    };
    **collection = fsl_core::KernelExpr::Var("s".into());
    (model, expected)
}

impl Case {
    pub fn location(&self) -> (u32, u32) {
        let offset = self.source.find(self.marker).expect("authored marker");
        let before = &self.source[..offset];
        (
            u32::try_from(before.bytes().filter(|b| *b == b'\n').count() + 1).unwrap(),
            u32::try_from(before.rsplit('\n').next().unwrap().chars().count() + 1).unwrap(),
        )
    }

    pub fn materialize(&self, root: &Path, index: usize) -> PathBuf {
        let directory = root.join(index.to_string());
        std::fs::create_dir_all(&directory).expect("task-local fixtures");
        for (name, source) in &self.files {
            std::fs::write(directory.join(name), source).expect("component");
        }
        let path = directory.join("root.fsl");
        std::fs::write(&path, &self.source).expect("source");
        path
    }
}

#[allow(clippy::too_many_lines)] // Explicit cross-consumer source/position inventory.
pub fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (domain, source, capacity, values, expected) in [
        ("range-empty", "1..0", 2, "1", 0),
        ("range-full", "0..1", 2, "1", 2),
        ("seq-zero", "ys", 0, "", 0),
        ("seq-full", "ys", 2, "1", 1),
        ("set-empty", "xs", 2, "", 0),
        ("set-full", "xs", 2, "1", 1),
    ] {
        for mode in ["count-filter", "sum-filter", "sum-value"] {
            for receiver in ["h", "done", "m"] {
                for negative in [false, true] {
                    if receiver != "m" && negative {
                        continue;
                    }
                    let expression = match mode {
                        "count-filter" => format!("count(z in {source} where {receiver}[z] == 1)"),
                        "sum-filter" => format!("sum(z in {source} of 1 where {receiver}[z] == 1)"),
                        _ => format!("sum(z in {source} of {receiver}[z])"),
                    };
                    let literal = if domain.starts_with("set") {
                        values
                    } else {
                        "1"
                    };
                    let seq_values = if domain.starts_with("seq") {
                        values
                    } else {
                        "1"
                    };
                    cases.push(Case {
                        id: format!("aggregate/{domain}/{mode}/{receiver}/{negative}"),
                        source: format!("spec Aggregate {{\n  type K = 0..1\n  state {{ h: relation K -> K, m: Map<K, K>, xs: Set<K>, ys: Seq<K, {capacity}>, done: Bool }}\n  init {{ h = Set {{}} forall k: K {{ m[k] = 1 }} xs = Set {{ {literal} }} ys = Seq {{ {seq_values} }} done = false }}\n  action go() {{ done = done }}\n  invariant Capture {{ {expression} == {} }}\n}}\n", expected + i32::from(negative)),
                        files: vec![], rejection: receiver != "m", negative,
                        public_name: None, marker: "invariant Capture",
                    });
                }
            }
        }
    }
    for (kind, init, property, public_name) in [
        (
            "leadsTo",
            "xs = Set { 1 } done = false h = Set {}",
            "leadsTo Capture { forall z in xs { z == 1 ~> within 1 true } }",
            "part.Capture",
        ),
        (
            "init",
            "xs = Set { 1 } done = false h = Set {}\n    forall z in xs { }",
            "invariant Capture { not done }",
            "part.xs",
        ),
        (
            "relation",
            "xs = Set { 1 } done = false h = Set {}",
            "invariant Capture { h[0] == 0 }",
            "part.Capture",
        ),
    ] {
        cases.push(Case {
            id: format!("component/{kind}"),
            source: "compose Root {\n  use Good as first from \"good.fsl\"\n  use Piece as part from \"piece.fsl\"\n}\n".into(),
            files: vec![
                ("good.fsl".into(), "spec Good { state { ok: Bool } init { ok = false } action idle() {} }".into()),
                ("piece.fsl".into(), format!("spec Piece {{\n  type K = 0..1\n  state {{ xs: Set<K>, done: Bool, h: relation K -> K }}\n  init {{\n    {init}\n  }}\n  action go() {{ done = true }}\n  {property}\n}}\n")),
            ], rejection: true, negative: false, public_name: Some(public_name), marker: "use Piece",
        });
    }
    cases
}
