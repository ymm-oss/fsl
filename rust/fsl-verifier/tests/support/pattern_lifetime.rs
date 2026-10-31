// SPDX-License-Identifier: Apache-2.0

use fsl_core::{
    FsResolver, KernelBinder as Binder, KernelExpr as Expr, KernelModel, Pattern, build_model,
    parse_kernel_source,
};

pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
    {
        std::task::Poll::Ready(value) => value,
        std::task::Poll::Pending => panic!("native solver yielded Pending"),
    }
}

pub fn build(source: &str) -> KernelModel {
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse"))
        .expect("fresh checked model")
}

// Bypass only collision reservation, not checked expression typing. No model
// field other than the test-owned expression is changed.
pub fn rename(expr: &mut Expr, target: &str) -> usize {
    match expr {
        Expr::Var(name) if name == "a" || name == "b" => {
            target.clone_into(name);
            1
        }
        Expr::Is { expr, pattern } => {
            let count = rename(expr, target);
            if let Pattern::Some(name) = pattern
                && (name == "a" || name == "b")
            {
                target.clone_into(name);
                count + 1
            } else {
                count
            }
        }
        Expr::Binary { left, right, .. } | Expr::Index(left, right) => {
            rename(left, target) + rename(right, target)
        }
        Expr::Not(expr) | Expr::Some(expr) => rename(expr, target),
        Expr::Seq(items) => items.iter_mut().map(|e| rename(e, target)).sum(),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => rename(condition, target) + rename(then_expr, target) + rename(else_expr, target),
        Expr::Quantified { binder, body, .. } => {
            let (Binder::Typed { where_expr, .. }
            | Binder::Range { where_expr, .. }
            | Binder::Collection { where_expr, .. }) = binder;
            where_expr.as_mut().map_or(0, |e| rename(e, target)) + rename(body, target)
        }
        Expr::Var(_) | Expr::Num(_) | Expr::Bool(_) => 0,
        other => panic!("unhandled fixture: {other:?}"),
    }
}

pub struct Cell {
    pub name: &'static str,
    pub expr: &'static str,
    pub target: &'static str,
    pub absent: bool,
    pub expected: bool,
}

// Each truth value is independent of the engines. Fresh and rewritten forms
// must both produce it; no historical concrete false-proof pins.
#[rustfmt::skip]
pub const CELLS: &[Cell] = &[
    Cell { name: "r3-map-outer", expr: "forall z in 0..3 { ((o is some(a)) and a == 3) or z != 3 }", target: "z", absent: false, expected: false },
    Cell { name: "r4-or", expr: "((r[0] is some(a)) and a == 3) or not ((r[1] is some(b)) and b == 3)", target: "v", absent: false, expected: false },
    Cell { name: "r4-reverse", expr: "not ((r[1] is some(a)) and a == 3) or ((r[0] is some(b)) and b == 3)", target: "v", absent: false, expected: false },
    Cell { name: "r4-equality", expr: "((r[0] is some(a)) and a == 3) == ((r[1] is some(b)) and b == 3)", target: "v", absent: false, expected: false },
    Cell { name: "r4-not-and", expr: "not ((r[0] is some(a)) and a == 3) and not ((r[1] is some(b)) and b == 3)", target: "v", absent: false, expected: false },
    Cell { name: "b18-or", expr: "((h is some(a)) or false) and x == 3", target: "x", absent: false, expected: false },
    Cell { name: "b15-not", expr: "not (h is some(a)) or x == 3", target: "x", absent: false, expected: false },
    Cell { name: "b17-implies", expr: "((h is some(a)) => false) or x == 3", target: "x", absent: false, expected: false },
    Cell { name: "and-false-or", expr: "((h is some(a)) and false) or x == 3", target: "x", absent: false, expected: false },
    Cell { name: "b04-reverse", expr: "not (h is some(a)) or x == 1", target: "x", absent: false, expected: true },
    Cell { name: "if-unselected", expr: "(if false then (o is some(a)) else true) and (p is some(b) and b == 2)", target: "v", absent: false, expected: true },
    Cell { name: "not-reuse", expr: "not (o is some(a)) or (p is some(b) and b == 2)", target: "v", absent: false, expected: true },
    Cell { name: "if-reuse", expr: "(if true then (o is some(a)) else false) and (p is some(b) and b == 2)", target: "v", absent: false, expected: true },
    Cell { name: "not-none-reuse", expr: "not (o is some(a)) and (p is some(b) and b == 2)", target: "v", absent: true, expected: true },
    Cell { name: "expected-if", expr: "(true == (if false then (o is some(a)) else true)) and (p is some(b) and b == 2)", target: "v", absent: false, expected: true },
    Cell { name: "negative-reuse", expr: "(if false then (o is some(a)) else true) and (p is some(b) and b == 1)", target: "v", absent: false, expected: false },
    Cell { name: "legal-and", expr: "o is some(a) and a == 1", target: "v", absent: false, expected: true },
    Cell { name: "legal-implies", expr: "(o is some(a)) => a == 1", target: "v", absent: false, expected: true },
    Cell { name: "negative-and", expr: "o is some(a) and a == 2", target: "v", absent: false, expected: false },
    Cell { name: "negative-implies", expr: "(o is some(a)) => a == 2", target: "v", absent: false, expected: false },
    Cell { name: "none-implies", expr: "(o is some(a)) => a == 1", target: "v", absent: true, expected: true },
    Cell { name: "none-and", expr: "o is some(a) and a == 1", target: "v", absent: true, expected: false },
    Cell { name: "where-selected", expr: "forall k in 0..0 where ((o is some(a)) and a == 1) { false }", target: "v", absent: false, expected: false },
    Cell { name: "where-unselected", expr: "forall k in 0..0 where ((o is some(a)) and a == 2) { false }", target: "v", absent: false, expected: true },
    Cell { name: "if-else-2", expr: "if false then (o is some(a) and a == 1) else (p is some(b) and b == 2)", target: "v", absent: false, expected: true },
    Cell { name: "if-else-not-1", expr: "if false then (o is some(a) and a == 1) else (p is some(b) and b == 1)", target: "v", absent: false, expected: false },
    Cell { name: "contextual-if-else", expr: "true == (if false then (o is some(a) and a == 1) else (p is some(b) and b == 2))", target: "v", absent: false, expected: true },
    Cell { name: "collection-if", expr: "(if false then Seq {o is some(a) and a == 1} else Seq {p is some(b) and b == 2}) == s", target: "v", absent: false, expected: true },
];

pub fn model(cell: &Cell, fresh: bool) -> KernelModel {
    let option = if cell.absent { "none" } else { "some(1)" };
    let mut model = build(&format!(
        "spec Lifetime {{type V=0..5 type K=0..1
         state {{o:Option<V> = {option},p:Option<V> = some(2),h:Option<V> = some(3),x:V=1,r:Map<K,Option<V>>,s:Seq<Bool,1> = Seq {{true}}}}
         init {{r[0]=some(1) r[1]=some(3)}} action tick() {{}}
         invariant Q {{ {} }} }}", cell.expr
    ));
    if !fresh {
        let property = model.invariants.iter_mut().find(|p| p.name == "Q").unwrap();
        assert!(rename(&mut property.expr, cell.target) > 0);
    }
    let property = model.invariants.iter().find(|p| p.name == "Q").unwrap();
    assert_eq!(
        model
            .expression_type(&property.expr, &[], None)
            .expect("rewritten checked context"),
        Some(fsl_core::TypeRef::Bool),
        "collision seam must still have a valid checked Bool context: {}",
        cell.name
    );
    model
}
