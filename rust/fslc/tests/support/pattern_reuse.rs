// SPDX-License-Identifier: Apache-2.0

//! Reuse inputs preserve the evaluator's existing first-binding-wins boundary.

use super::forms::{Case, Role};

#[allow(clippy::too_many_lines)] // Literal source corpus preserves baseline locations byte-for-byte.
pub fn cases() -> Vec<Case> {
    use Role::{Collision, Proved, Violated};
    [
        (
            "a14",
            Collision,
            "action a",
            r"spec A {
  state { o: Option<Int>, p: Option<Int>, y: Int }
  init { o = some(1) p = some(2) y = 0 }
  action a() {
    requires (o is some(v)) => v == 1
    requires (p is some(v)) => v == 2
    y = 2
  }
  invariant P { y == 0 }
}
",
        ),
        (
            "a01",
            Collision,
            "action a",
            r"spec A {
  state { o: Option<Int>, p: Option<Int>, x: Int, y: Int }
  init { o = some(1) p = some(2) x = 0 y = 0 }
  action a() {
    requires (o is some(v)) => true
    requires p is some(v)
    y = v
  }
  invariant P { y == 0 or y == 2 }
}
",
        ),
        (
            "a02",
            Collision,
            "action a",
            r"spec A {
  state { o: Option<Int>, p: Option<Int>, x: Int, y: Int }
  init { o = some(1) p = some(2) x = 0 y = 0 }
  action a() {
    requires (o is some(v)) or x == 0
    requires p is some(v)
    y = v
  }
  invariant P { y == 0 or y == 2 }
}
",
        ),
        (
            "a13",
            Collision,
            "action a",
            r"spec A {
  state { o: Option<Int>, p: Option<Int>, x: Int, y: Int }
  init { o = some(1) p = some(2) x = 0 y = 0 }
  action a() {
    requires (o is some(v)) => true
    requires (p is some(v)) => v == 2
    y = 2
  }
  invariant P { y == 0 or y == 2 }
}
",
        ),
        (
            "r08",
            Collision,
            "invariant P",
            r"spec A {
  state { o: Option<Int>, p: Option<Int>, x: Int }
  init { o = some(1) p = some(2) x = 0 }
  action stay() {}
  invariant P { ((p is some(v)) => v == 2) and ((o is some(v)) => v == 1) }
}
",
        ),
        (
            "r01",
            Collision,
            "invariant P",
            r"spec A {
  state { o: Option<Int>, p: Option<Int>, x: Int }
  init { o = some(1) p = some(2) x = 0 }
  action stay() {}
  invariant P { ((o is some(v)) => true) and ((p is some(v)) => v == 2) }
}
",
        ),
        (
            "r02",
            Collision,
            "invariant P",
            r"spec A {
  state { o: Option<Int>, p: Option<Int>, x: Int }
  init { o = some(1) p = some(2) x = 0 }
  action stay() {}
  invariant P { ((o is some(v)) or true) and ((p is some(v)) => v == 2) }
}
",
        ),
        (
            "r03",
            Collision,
            "invariant P",
            r"spec A {
  state { o: Option<Int>, p: Option<Int>, x: Int }
  init { o = none p = some(2) x = 0 }
  action stay() {}
  invariant P { (o is some(v)) or ((p is some(v)) and v == 2) }
}
",
        ),
        (
            "r04",
            Collision,
            "invariant P",
            r"spec A {
  state { o: Option<Int>, p: Option<Int>, x: Int }
  init { o = some(1) p = some(2) x = 0 }
  action stay() {}
  invariant P { ((o is some(v)) == true) and ((p is some(v)) => v == 2) }
}
",
        ),
        (
            "d01",
            Collision,
            "invariant P",
            r"spec A {
  state { o: Option<Int>, p: Option<Int>, x: Int }
  init { o = some(1) p = some(2) x = 0 }
  def first() = (o is some(v)) => true
  action stay() {}
  invariant P { first() and ((p is some(v)) => v == 2) }
}
",
        ),
        (
            "d02",
            Collision,
            "invariant P",
            r"spec A {
  state { o: Option<Int>, p: Option<Int>, x: Int }
  init { o = some(1) p = some(2) x = 0 }
  def second() = (p is some(v)) => v == 2
  action stay() {}
  invariant P { ((o is some(v)) => true) and second() }
}
",
        ),
        (
            "i02",
            Collision,
            "invariant P",
            r"spec A {
  type K = 0..1
  state { o: Option<Int>, p: Option<Int>, x: Int, y: Int }
  init { o = none p = none x = 0 y = 0 }
  def q(z: Int) = z == 1
  action stay() {}
  invariant P { ((o is some(v)) or (p is some(v))) => v == 1 }
}
",
        ),
        (
            "i23",
            Collision,
            "invariant P",
            r"spec A {
  type K = 0..1
  state { o: Option<Int>, p: Option<Int>, x: Int, y: Int }
  init { o = none p = none x = 0 y = 0 }
  def q(z: Int) = z == 1
  action stay() {}
  invariant P { ((o is some(v)) and x == 0 or (p is some(v))) => v == 1 }
}
",
        ),
        (
            "a15",
            Violated,
            "invariant P",
            r"spec A {
  state { o: Option<Int>, p: Option<Int>, y: Int }
  init { o = some(1) p = some(2) y = 0 }
  action a() {
    requires (o is some(v)) => v == 1
    requires (p is some(w)) => w == 2
    y = 2
  }
  invariant P { y == 0 }
}
",
        ),
        (
            "r08-renamed",
            Proved,
            "invariant P",
            r"spec A {
  state { o: Option<Int>, p: Option<Int>, x: Int }
  init { o = some(1) p = some(2) x = 0 }
  action stay() {}
  invariant P { ((p is some(w)) => w == 2) and ((o is some(v)) => v == 1) }
}
",
        ),
    ]
    .into_iter()
    .map(|(id, role, marker, source)| {
        super::forms::case(
            id,
            "sibling-collision/evaluator-bound-names",
            role,
            source.into(),
            marker,
        )
    })
    .collect()
}

pub fn priority_cases() -> Vec<Case> {
    [
        (
            "priority-guards", "priority-guards-renamed", "action a",
            super::forms::spec(
                "o: Option<Int>, p: Option<Int>, x: Int", "o = some(1) p = some(2) x = 1",
                "  action a() {\n    requires (o is some(v)) => true\n    requires v == 1\n    requires p is some(v)\n    x = v\n  }\n", "x >= 0",
            ),
            "requires v == 1", "requires x == 1",
        ),
        (
            "priority-siblings", "priority-siblings-renamed", "invariant P",
            super::forms::spec(
                "o: Option<Int>, p: Option<Int>, x: Int", "o = some(1) p = some(2) x = 1",
                "  action stay() {}\n", "((o is some(v)) => true) and (v == 1 and (p is some(v)))",
            ),
            "(v == 1 and", "(x == 1 and",
        ),
    ]
    .into_iter()
    .flat_map(|(id, renamed_id, marker, source, from, to)| {
        assert_eq!(source.matches(from).count(), 1, "one earlier invalid read");
        let renamed = source.replacen(from, to, 1);
        [
            super::forms::case(id, "diagnostic-priority", Role::Refuse, source, marker),
            super::forms::case(renamed_id, "diagnostic-priority", Role::Collision, renamed, marker),
        ]
    })
    .collect()
}

pub const RENAMER_FILES: &[(&str, &str)] = &[
    (
        "h1.fsl",
        r#"compose C {
  use A as l from "a.fsl"
  use B as r from "b.fsl"
  internal l.b
  internal r.c
  action s() = l.b() || r.c() {
    requires (l.o is some(v)) or v == 1
  }
}
"#,
    ),
    (
        "a.fsl",
        r"spec A {
  type K = 0..3
  state { o: Option<K>, x: Int }
  init { o = none x = 0 }
  action b() {
    requires not (o is some(v))
    x = 1
  }
  invariant PA { x >= 0 }
}
",
    ),
    (
        "b.fsl",
        r"spec B {
  type K = 0..3
  state { p: Option<K>, y: Int }
  init { p = some(2) y = 0 }
  action c() {
    requires p is some(v)
    y = v
  }
  invariant PB { y == 0 or y == 2 }
}
",
    ),
];

pub fn renamer_export_files() -> Vec<(&'static str, String)> {
    RENAMER_FILES
        .iter()
        .map(|&(name, source)| {
            (
                name,
                source
                    .replace("o = none", "o = some(1)")
                    .replace(
                        "requires not (o is some(v))",
                        "requires (o is some(v)) => true",
                    )
                    .replace(
                        "requires (l.o is some(v)) or v == 1",
                        "requires (l.o is some(v)) and v == 1\n    ensures v == 1",
                    ),
            )
        })
        .collect()
}
