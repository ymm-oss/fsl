// SPDX-License-Identifier: Apache-2.0

//! Authored scope positions, not a traversal of the production scope helpers.

#[derive(Clone, Copy, Debug)]
pub enum Role {
    Refuse,
    Violated,
    Proved,
    Guard,
    Collision,
}

pub struct Case {
    pub id: &'static str,
    pub paths: &'static str,
    pub role: Role,
    pub source: String,
    pub component: String,
    pub compose: String,
    pub marker: &'static str,
}

pub fn spec(fields: &str, init: &str, items: &str, predicate: &str) -> String {
    format!(
        "spec A {{\n  type K = 0..1\n  state {{ {fields} }}\n  init {{ {init} }}\n{items}  invariant P {{ {predicate} }}\n}}\n"
    )
}

pub fn scalar(items: &str, predicate: &str) -> String {
    spec(
        "o: Option<Int>, x: Int, y: Int",
        "o = none x = 0 y = 0",
        items,
        predicate,
    )
}

fn case(
    id: &'static str,
    paths: &'static str,
    role: Role,
    source: String,
    marker: &'static str,
) -> Case {
    Case {
        id,
        paths,
        role,
        component: source.clone(),
        source,
        marker,
        compose: "compose C {\n  use A as part from \"a.fsl\"\n}\n".into(),
    }
}

#[allow(clippy::too_many_lines)] // Finite position/path table and paired controls.
pub fn cases() -> Vec<Case> {
    use Role::{Collision, Guard, Proved, Refuse, Violated};
    let mut rows = Vec::new();
    for (id, expression) in [
        ("type-or-right", "(o is some(v)) or v == 1"),
        (
            "type-nested-and-or-right",
            "((o is some(v)) and true) or v == 1",
        ),
        (
            "type-equality-escape",
            "((o is some(v)) == false) and v == 1",
        ),
        (
            "type-inequality-escape",
            "((o is some(v)) != false) and v == 1",
        ),
        ("type-equality-right-operand", "(o is some(v)) == (v == 1)"),
        ("type-not-inside", "not ((o is some(v)) or v == 1)"),
        ("type-not-outside", "(not (o is some(v))) and v == 1"),
        (
            "type-implies-left-or",
            "((o is some(v)) or x == 0) => v == 1",
        ),
        (
            "type-implies-left-implies",
            "((o is some(v)) => true) => v == 1",
        ),
        (
            "type-implies-escape-to-and",
            "((o is some(v)) => true) and v == 1",
        ),
        ("type-if-else", "if (o is some(v)) then true else v == 1"),
        ("type-if-then", "if (o is some(v)) then v == 1 else true"),
        (
            "type-forall-body",
            "(o is some(v)) or (forall k in 0..1 { v == 1 })",
        ),
        (
            "type-exists-body",
            "(o is some(v)) or (exists k in 0..1 { v == 1 })",
        ),
        (
            "type-forall-where",
            "(o is some(v)) or (forall k in 0..1 where v == 1 { true })",
        ),
        (
            "type-sum-body",
            "(o is some(v)) or sum(k in 0..1 of v) == 0",
        ),
        (
            "type-sum-where",
            "(o is some(v)) or sum(k in 0..1 of k where v == 1) == 0",
        ),
        (
            "type-count-where",
            "(o is some(v)) or count(k in 0..1 where v == 1) == 0",
        ),
        (
            "type-quantifier-escape",
            "(forall k in 0..1 { o is some(v) }) and v == 1",
        ),
        (
            "type-aggregate-escape",
            "(count(k in 0..1 where o is some(v)) == 0) and v == 1",
        ),
        (
            "type-method-argument",
            "(o is some(v)) or Set { 1 }.contains(v)",
        ),
    ] {
        rows.push(case(
            id,
            "typecheck,public-kernel,component-rewrite",
            Refuse,
            scalar("  action stay() {}\n", expression),
            "invariant P",
        ));
    }
    let mut issue = case("issue-exact", "typecheck,public-kernel,component-rewrite", Refuse,
        "spec D {\n  type K = 0..1\n  state { o: Option<K> }\n  init { o = none }\n  action g() { o = none }\n  invariant P { (o is some(v)) or v == 1 }\n}\n".into(), "invariant P");
    issue.compose = issue.compose.replace("use A", "use D");
    rows.push(issue);
    // Keep the x = 1 action: unlike the static-state variant, this source
    // exposes a symbolic verdict with an unbound v on the named base.
    let mut witness = case("false-verdict-implies-left-or", "typecheck,public-kernel,component-rewrite", Refuse,
        "spec S {\n  state { o: Option<Int>, x: Int }\n  init { o = none  x = 0 }\n  action a() {  x = 1 }\n  invariant P { ((o is some(v)) or x == 0) => v == 1 }\n}\n".into(), "invariant P");
    witness.compose = witness.compose.replace("use A", "use S");
    rows.push(witness);
    for (id, fields, init, expression) in [
        (
            "partial-seq-index",
            "o: Option<Int>, q: Seq<Int, 2>",
            "o = none q = Seq { 1 }",
            "(o is some(v)) or q.at(v) == 1",
        ),
        (
            "partial-map-key",
            "o: Option<Int>, m: Map<K, Int>",
            "o = none forall k: K { m[k] = 1 }",
            "(o is some(v)) or m[v] == 1",
        ),
    ] {
        rows.push(case(
            id,
            "typecheck,public-kernel,partial-operation",
            Refuse,
            spec(fields, init, "  action stay() {}\n", expression),
            "invariant P",
        ));
    }
    for (id, items, marker) in [
        (
            "requires-or-value",
            "  action a() {\n    requires (o is some(v)) or x == 0\n    y = v\n  }\n",
            "y = v",
        ),
        (
            "requires-or-ensures",
            "  action a() {\n    requires (o is some(v)) or x == 0\n    ensures y == v\n  }\n",
            "ensures y",
        ),
        (
            "requires-or-later-requires",
            "  action a() {\n    requires (o is some(v)) or x == 0\n    requires v == 1\n  }\n",
            "requires v",
        ),
        (
            "requires-implies-value",
            "  action a() {\n    requires (o is some(v)) => true\n    y = v\n  }\n",
            "y = v",
        ),
    ] {
        rows.push(case(
            id,
            "typecheck,public-kernel,requires-export,component-rewrite,compose-names",
            Refuse,
            scalar(items, "y == 0"),
            marker,
        ));
    }
    for (id, items, expression) in [
        (
            "def-body-or",
            "  def p() = (o is some(v)) or v == 1\n  action stay() {}\n",
            "p()",
        ),
        (
            "def-argument-or",
            "  def p(z: Bool) = z\n  action stay() {}\n",
            "p((o is some(v)) or v == 1)",
        ),
        (
            "def-caller-or-right",
            "  def p() = o is some(v)\n  action stay() {}\n",
            "p() or v == 1",
        ),
    ] {
        rows.push(case(
            id,
            "typecheck,public-kernel,def-expansion,free-name",
            Refuse,
            scalar(items, expression),
            "invariant P",
        ));
    }
    let mut sync = case(
        "compose-sync-alias-or",
        "typecheck,public-kernel,alias-resolution,compose-names,requires-export",
        Refuse,
        scalar(
            "  action a() { requires (o is some(v)) or v == 1 }\n",
            "y == 0",
        ),
        "requires",
    );
    sync.component = scalar("  action a() {}\n", "y == 0");
    sync.compose = "compose C {\n  use A as part from \"a.fsl\"\n  internal part.a\n  action call() = part.a() {\n    requires (part.o is some(v)) or v == 1\n  }\n}\n".into();
    rows.push(sync);
    for (id, expression, role) in [
        ("control-and", "(o is some(v)) and v == 1", Violated),
        ("control-implies", "(o is some(v)) => v == 1", Proved),
        (
            "control-nested-and",
            "((o is some(v)) and true) and v == 1",
            Violated,
        ),
    ] {
        rows.push(case(
            id,
            "typecheck,public-kernel,component-rewrite",
            role,
            scalar("  action stay() {}\n", expression),
            "invariant P",
        ));
    }
    rows.push(case(
        "control-requires-and",
        "typecheck,public-kernel,requires-export,component-rewrite,compose-names",
        Guard,
        scalar(
            "  action a() {\n    requires (o is some(v)) and x == 0\n    y = v\n  }\n",
            "y == 0",
        ),
        "y = v",
    ));
    rows.push(case(
        "control-def-and",
        "typecheck,public-kernel,def-expansion,free-name",
        Violated,
        scalar(
            "  def p() = (o is some(v)) and v == 1\n  action stay() {}\n",
            "p()",
        ),
        "invariant P",
    ));
    for (id, fields, init, items, expression) in [
        (
            "collision-state",
            "o: Option<Int>, v: Int",
            "o = none v = 0",
            "  action stay() {}\n",
            "o is some(v) and true",
        ),
        (
            "collision-constant",
            "o: Option<Int>",
            "o = none",
            "  const v = 0\n  action stay() {}\n",
            "o is some(v) and true",
        ),
        (
            "collision-enum",
            "o: Option<Tone>",
            "o = none",
            "  enum Tone { v, Other }\n  action stay() {}\n",
            "o is some(v) and true",
        ),
        (
            "collision-parameter",
            "o: Option<Int>, y: Int",
            "o = none y = 0",
            "  action a(v: K) { requires o is some(v) y = 0 }\n",
            "y == 0",
        ),
        (
            "collision-let",
            "o: Option<Int>, y: Int",
            "o = none y = 0",
            "  action a() { let v = 0 requires o is some(v) y = 0 }\n",
            "y == 0",
        ),
        (
            "collision-quantifier",
            "o: Option<Int>",
            "o = none",
            "  action stay() {}\n",
            "forall v in 0..1 { o is some(v) => true }",
        ),
        (
            "collision-earlier-pattern",
            "o: Option<Int>, p: Option<Int>",
            "o = none p = none",
            "  action stay() {}\n",
            "(o is some(v)) and (p is some(v)) and v == 1",
        ),
    ] {
        rows.push(case(
            id,
            "typecheck,component-rewrite,existing-collision",
            Collision,
            spec(fields, init, items, expression),
            "is some(v)",
        ));
    }
    rows
}
