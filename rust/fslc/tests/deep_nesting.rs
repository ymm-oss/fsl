// SPDX-License-Identifier: Apache-2.0

//! Regression for #620: a spec whose *structure* is deep must not abort the
//! process.
//!
//! Recursion whose depth tracks the spec -- not a `--depth` bound the user
//! chose -- reaches the one failure mode the delivery layer cannot report. A
//! stack overflow returns neither the JSON envelope nor an exit code: the
//! process dies on signal, so it leaves the outcome-projection contract
//! entirely (#537 C2). That makes "did not abort" the property under test here,
//! not "produced the right verdict" -- though both are asserted, because a
//! guard that silently changed an answer would be worse than the crash.
//!
//! The witness is *generated* rather than checked in. The defect class was
//! found by machine-generated specs, and a fixture of fixed size is a fixture
//! whose relationship to the failure threshold rots the moment a frame grows.
//! `refinement_trio` is the fixture, and `N` is the knob: see the note on
//! [`WITNESS_STAGES`] for why it is set where it is.
//!
//! The `unguarded-recursion` fault operator
//! (`rust/fslc/tests/fault_operators/`) patches `recursion::guard` back into a
//! direct call and requires these tests to fail, so "the guard disappeared" is
//! machine-checked rather than assumed.

#[path = "support/fslc_cli.rs"]
mod fslc_cli;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

/// Abstract stages in the generated witness; the `map` if-chain is twice this.
///
/// Measured on a debug arm64 build with the 8 MiB thread #621 gives every
/// platform: the unguarded binary aborts (`exit 134`, "has overflowed its
/// stack") between N=140 and N=160, so 200 is above the threshold with margin
/// rather than merely at it. Below ~160 this test would pass on an unguarded
/// binary and assert nothing.
///
/// It is also cheap: the whole file runs in a couple of seconds. If a future
/// change makes it slow, raise the *cost* budget or narrow the commands --
/// lowering N below 160 turns this file into decoration, and the fault operator
/// is what will catch that having happened.
const WITNESS_STAGES: usize = 200;

/// The unguarded binary survives to N=140-160 on the 8 MiB stack #621 gives
/// every platform, so a witness at or below that threshold cannot detect the
/// guard's absence. Enforced at compile time: lowering `WITNESS_STAGES` to make
/// a slow test fast would otherwise leave three tests that pass while asserting
/// nothing about recursion.
const _: () = assert!(WITNESS_STAGES > 160);

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

static NEXT_SCRATCH: AtomicUsize = AtomicUsize::new(0);

/// A fresh scratch directory per test under the gitignored `rust/target/`,
/// following the `scratch_dir` idiom `analysis_conservation.rs` establishes.
fn scratch_dir(name: &str) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = root()
        .join("rust/target")
        .join(format!("deep-nesting-{name}-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Paths of one generated refinement trio.
struct Trio {
    implementation: PathBuf,
    abstraction: PathBuf,
    mapping: PathBuf,
}

/// Writes a refinement trio whose structural size is `n`.
///
/// The abstraction walks an `n`-stage enum; the implementation walks `2n`
/// stages, two per abstract stage. The mapping folds the implementation chain
/// onto the abstract one with a `2n`-long right-nested `if` chain, mapping odd
/// steps to abstract steps and even steps to stutter. That scales the three
/// quantities the crashing corpus mappings were large in at once: enum members,
/// actions, and `map`-expression nesting.
///
/// This is the Rust port of the Python generator that produced the #620
/// witness. It lives here, in the test that needs it, so the regression carries
/// its own fixture and does not depend on an interpreter being present.
fn refinement_trio(directory: &Path, n: usize) -> Trio {
    assert!(n >= 2, "a trio needs at least two stages");

    let mut abstraction = format!("spec DeepAbs{n} {{\n  enum ASt {{ ");
    for k in 0..n {
        if k > 0 {
            abstraction.push_str(", ");
        }
        let _ = write!(abstraction, "A{k}");
    }
    abstraction.push_str(" }\n  state { st: ASt }\n  init { st = A0 }\n");
    for k in 0..n - 1 {
        let _ = write!(
            abstraction,
            "  action step_{k}() {{\n    requires st == A{k}\n    st = A{}\n  }}\n",
            k + 1
        );
    }
    let _ = write!(abstraction, "  reachable Done {{ st == A{} }}\n}}\n", n - 1);

    let m = 2 * n;
    let mut implementation = format!("spec DeepImpl{n} {{\n  enum ISt {{ ");
    for k in 0..m {
        if k > 0 {
            implementation.push_str(", ");
        }
        let _ = write!(implementation, "I{k}");
    }
    implementation.push_str(" }\n  state { st: ISt }\n  init { st = I0 }\n");
    for k in 0..m - 1 {
        let _ = write!(
            implementation,
            "  action istep_{k}() {{\n    requires st == I{k}\n    st = I{}\n  }}\n",
            k + 1
        );
    }
    let _ = write!(
        implementation,
        "  reachable Done {{ st == I{} }}\n}}\n",
        m - 1
    );

    // `I(2k)` and `I(2k+1)` both fold to `A(k)`, written as one right-nested
    // `if` chain of length `2n`. This expression is the deep structure.
    let mut chain = format!("A{}", n - 1);
    for k in (0..m - 1).rev() {
        chain = format!("if st == I{k} then A{}\n           else {chain}", k / 2);
    }
    let mut mapping = format!(
        "refinement DeepImpl{n}RefinesDeepAbs{n} {{\n  impl DeepImpl{n}\n  abs  DeepAbs{n}\n\n  map st = {chain}\n\n"
    );
    for k in 0..m - 1 {
        if k % 2 == 0 {
            let _ = writeln!(mapping, "  action istep_{k}() -> stutter");
        } else {
            let _ = writeln!(mapping, "  action istep_{k}() -> step_{}()", k / 2);
        }
    }
    mapping.push_str("}\n");

    let trio = Trio {
        implementation: directory.join(format!("impl_{n}.fsl")),
        abstraction: directory.join(format!("abs_{n}.fsl")),
        mapping: directory.join(format!("map_{n}.fsl")),
    };
    std::fs::write(&trio.implementation, implementation).expect("write impl");
    std::fs::write(&trio.abstraction, abstraction).expect("write abs");
    std::fs::write(&trio.mapping, mapping).expect("write map");
    trio
}

fn run(args: &[&str]) -> Output {
    fslc_cli::command()
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native fslc")
}

/// Requires `fslc` to have exited on its own rather than dying on a signal, and
/// returns its exit code.
///
/// This is the assertion the whole file exists for. `Output::status.code()` is
/// `None` exactly when the process was killed by a signal, which is how a stack
/// overflow ends: `SIGABRT` after the runtime prints "has overflowed its
/// stack". A test that only compared exit codes would panic on `unwrap` with a
/// message that says nothing about why.
fn exit_code(label: &str, stages: usize, output: &Output) -> i32 {
    output.status.code().unwrap_or_else(|| {
        panic!(
            "`fslc {label}` on a {stages}-stage witness died on a signal \
             instead of exiting -- a stack overflow returns neither an exit code \
             nor a JSON envelope, so it escapes the outcome contract entirely \
             (#620). stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn json(label: &str, output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "`fslc {label}` did not print a JSON envelope: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// `refine` is the command the #620 witness was built against: it reaches the
/// parser, the surface-to-kernel conversion, enum-conversion elaboration, the
/// typechecker, and the symbolic evaluator, which are five of the guarded
/// cycles.
#[test]
fn refine_proves_a_deeply_nested_mapping_instead_of_overflowing_the_stack() {
    let directory = scratch_dir("refine");
    let trio = refinement_trio(&directory, WITNESS_STAGES);

    let output = run(&[
        "refine",
        trio.implementation.to_str().expect("impl path"),
        trio.abstraction.to_str().expect("abs path"),
        trio.mapping.to_str().expect("map path"),
    ]);
    let status = exit_code("refine", WITNESS_STAGES, &output);
    let verdict = json("refine", &output);

    assert_eq!(
        verdict["result"], "refines",
        "the generated trio is a real refinement, so a verdict other than \
         `refines` means the stack guard changed an answer rather than \
         preserving it; envelope={verdict}"
    );
    assert_eq!(status, 0, "a `refines` verdict exits 0; envelope={verdict}");
}

/// The same file under `check`, which reaches the parser and the
/// surface-to-kernel conversion but not the evaluator.
///
/// A mapping file has no `state` block, so `check` rejects it. That rejection
/// is the point: it must arrive as a `semantics` envelope on stdout with exit
/// 2, not as a dead process. #620 confirmed `check` and `fmt` abort on the same
/// file `refine` aborts on, which is why this is not a `refine`-only defect.
#[test]
fn check_reports_a_deeply_nested_mapping_as_a_diagnostic_not_a_crash() {
    let directory = scratch_dir("check");
    let trio = refinement_trio(&directory, WITNESS_STAGES);

    let output = run(&["check", trio.mapping.to_str().expect("map path")]);
    let status = exit_code("check", WITNESS_STAGES, &output);
    let envelope = json("check", &output);

    assert_eq!(status, 2, "envelope={envelope}");
    assert_eq!(envelope["result"], "error", "envelope={envelope}");
    assert_eq!(envelope["kind"], "semantics", "envelope={envelope}");
}

/// `fmt` reaches the parser and `render_source`, a third cycle that neither of
/// the other two commands exercises.
#[test]
fn fmt_formats_a_deeply_nested_mapping_instead_of_overflowing_the_stack() {
    let directory = scratch_dir("fmt");
    let trio = refinement_trio(&directory, WITNESS_STAGES);

    let output = run(&["fmt", trio.mapping.to_str().expect("map path")]);
    let status = exit_code("fmt", WITNESS_STAGES, &output);

    assert_eq!(
        status,
        0,
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.stdout.is_empty(),
        "`fmt` produced no output for a {WITNESS_STAGES}-stage witness"
    );
}

/// `analyze` reaches `Expr::kernel_ast_v1`, which is where #622 lived: the
/// projection built each node with `json!`, and `json!` re-serialized every
/// already-built child subtree, spending O(depth) stack *inside*
/// `recursion::guard` rather than around it. This aborted (exit 138) until the
/// projection switched to `Value::Array`.
#[test]
fn analyze_projects_a_deeply_nested_mapping_instead_of_overflowing_the_stack() {
    let directory = scratch_dir("analyze");
    let trio = refinement_trio(&directory, WITNESS_STAGES);

    let output = run(&["analyze", trio.mapping.to_str().expect("map path")]);
    let status = exit_code("analyze", WITNESS_STAGES, &output);

    assert_eq!(
        status,
        0,
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Deliberately not parsed. `serde_json`'s *deserializer* has its own
    // 128-deep nesting limit, so `from_slice` on a 200-deep tagged array fails
    // with "recursion limit exceeded" even though `fslc` produced the envelope
    // correctly. That is a property of this test's reader, not of the output,
    // and asserting on the exit code plus non-empty stdout tests what this file
    // is about -- that the process finished at all.
    assert!(
        output.stdout.starts_with(b"{"),
        "`analyze` produced no JSON envelope for a {WITNESS_STAGES}-stage witness"
    );
}

/// The same projection, reached the way the *digests* reach it.
///
/// This is the lane the #620 witness could not open. Its depth lives in a
/// refinement mapping, so it reached the projection only through `analyze` --
/// a property of the witness, not of the defect. Both spec digests
/// (`fslc::approval::spec_digest`,
/// `fsl_tools::document_digest::spec_digest_from_kernel`) project a *spec*, so
/// proving anything about them needs the depth inside one. Written as a
/// separate generator rather than by deepening the trio, because the two
/// shapes reach different cycles: this one found
/// `PredicateExpander::expand_expr` and `public_kernel::expr_json` still
/// unguarded after #620 shipped.
#[test]
fn a_deeply_nested_invariant_reaches_the_digest_projection_without_aborting() {
    let directory = scratch_dir("invariant");
    let spec = deep_invariant_spec(&directory, WITNESS_STAGES);
    // Repository-relative on purpose: Public Kernel v2 refuses an absolute
    // `spec.source.file` ("must be repository-relative or a portable URI"), and
    // `document claims` carries that identity. The scratch directory lives
    // under `rust/target/`, so a relative path always exists.
    let relative = spec
        .strip_prefix(root())
        .expect("scratch path is inside the repository");
    let path = relative.to_str().expect("spec path");

    for command in [
        vec!["check", path],
        vec!["document", "claims", path],
        vec!["kernel", path],
    ] {
        let label = command.join(" ");
        let output = run(&command);
        let status = exit_code(&label, WITNESS_STAGES, &output);
        assert_eq!(
            status,
            0,
            "`fslc {label}` on a {WITNESS_STAGES}-deep invariant; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Nesting depth of the invariant witness for the symbolic engines (#1164).
///
/// `WITNESS_STAGES` does not reach this cycle: `evaluation_status_with_policy`
/// (the definedness walk in `fsl-verifier/src/eval.rs`) survives 200 levels
/// unguarded. Measured on a debug arm64 build with the 8 MiB `fslc` thread, the
/// unguarded binary aborts `verify` (bmc and induction alike) between N=300
/// and N=320, at ~25 KiB per level, so 600 is above the threshold with margin. The ceiling is the other side: at N=3000 a
/// debug build overflows in the derived `Clone` of `fsl_syntax::ast::Expr`
/// before verification starts, which is a separate cycle every command shares
/// and not what these two tests are about.
const VERIFIER_WITNESS_STAGES: usize = 600;

/// The largest depth the unguarded definedness walk was measured to survive.
const VERIFIER_UNGUARDED_SURVIVES: usize = 300;

/// Same reasoning as the assertion on `WITNESS_STAGES`: a witness at or below
/// the survival depth passes on an unguarded binary and asserts nothing.
const _: () = assert!(VERIFIER_WITNESS_STAGES > VERIFIER_UNGUARDED_SURVIVES);

/// Runs `verify` under one symbolic engine on the deep invariant and requires
/// an ordinary envelope carrying `expected` and exit 0.
fn verify_deep_invariant(engine: &str, expected: &str) {
    let directory = scratch_dir(&format!("verify-{engine}"));
    let spec = deep_invariant_spec(&directory, VERIFIER_WITNESS_STAGES);
    let path = spec.to_str().expect("spec path");
    let label = format!("verify --engine {engine}");

    let output = run(&["verify", path, "--engine", engine, "--depth", "2"]);
    let status = exit_code(&label, VERIFIER_WITNESS_STAGES, &output);
    let envelope = json(&label, &output);

    assert_eq!(
        envelope["result"], expected,
        "every `if` arm is non-negative, so the invariant holds; a different \
         verdict means the stack guard changed an answer rather than \
         preserving it; envelope={envelope}"
    );
    assert_eq!(status, 0, "envelope={envelope}");
}

/// `verify --engine bmc` evaluates every invariant twice per step: once
/// through `eval` (guarded since #620) and once through the definedness walk
/// `evaluation_status_with_policy`, which recursed unguarded and aborted with
/// exit 134 and no envelope (#1164).
#[test]
fn bmc_verifies_a_deeply_nested_invariant_instead_of_overflowing_the_stack() {
    verify_deep_invariant("bmc", "verified");
}

/// `verify --engine induction` reaches the same definedness walk through its
/// base case and step, and aborted the same way (#1164).
#[test]
fn induction_proves_a_deeply_nested_invariant_instead_of_overflowing_the_stack() {
    verify_deep_invariant("induction", "proved");
}

/// Chain length of the full-evaluation witnesses below (#1164 review).
///
/// The `if` chain above is deep but *cheap to evaluate concretely*: `x` is
/// 0..2 there, so the concrete evaluator stops at the first true arm and never
/// descends. It therefore says nothing about `fsl_runtime::eval`, which every
/// engine runs -- bmc and induction through `find_boundary_violation`'s
/// monitor, explicit through `current_violation_selected`. These shapes force
/// every level to be evaluated: a left-nested `+` chain, a left-nested `and`
/// chain whose every conjunct is true, and an even-length `not` chain.
///
/// Measured on a debug arm64 build with the 8 MiB `fslc` thread and every
/// #1164 guard removed: `+` and `and` abort all four engines between N=200 and
/// N=300, `not` between N=300 and N=500. 1200 clears both with margin and
/// stays under N=2500, where a debug build starts overflowing in the derived
/// `Clone` of `fsl_syntax::ast::Expr` (#1186) before verification starts.
const FULL_EVALUATION_STAGES: usize = 1200;

/// The largest depth any full-evaluation shape survived with the guards removed.
const FULL_EVALUATION_UNGUARDED_SURVIVES: usize = 300;

const _: () = assert!(FULL_EVALUATION_STAGES > FULL_EVALUATION_UNGUARDED_SURVIVES);

/// Chain length for `check` on the same shapes.
///
/// `check` never evaluates, so its unguarded walks are smaller per level and
/// need a deeper witness: with the guards removed, `+` and `and` abort `check`
/// between N=500 and N=1000 (`typecheck::extend_pattern_binding`) and `not`
/// between N=1000 and N=2000 (`reserved::check_expr`). 2400 is above both and
/// still below the N=2500..5000 band where the derived `Clone` takes over.
const CHECK_STAGES: usize = 2400;

/// The largest depth `check` survived on any shape with the guards removed.
const CHECK_UNGUARDED_SURVIVES: usize = 1000;

const _: () = assert!(CHECK_STAGES > CHECK_UNGUARDED_SURVIVES);

/// An invariant shape whose concrete and symbolic evaluation both visit every
/// level.
#[derive(Clone, Copy)]
enum FullEvaluation {
    /// `(x + x + ... + x) >= 0`, left-nested.
    Sum,
    /// `x >= 0 and x >= 0 and ...`, left-nested, every conjunct true.
    Conjunction,
    /// `not not ... not (x >= 0)`, an even number of `not`s.
    Negation,
}

impl FullEvaluation {
    const ALL: [Self; 3] = [Self::Sum, Self::Conjunction, Self::Negation];

    fn name(self) -> &'static str {
        match self {
            Self::Sum => "sum",
            Self::Conjunction => "conjunction",
            Self::Negation => "negation",
        }
    }

    fn invariant(self, n: usize) -> String {
        match self {
            Self::Sum => format!("({}) >= 0", vec!["x"; n].join(" + ")),
            Self::Conjunction => vec!["x >= 0"; n].join(" and "),
            Self::Negation => format!("{}(x >= 0)", "not ".repeat(n - n % 2)),
        }
    }
}

/// Writes a spec whose invariant has shape `shape` at chain length `n`. `x`
/// stays in 0..2, so the invariant holds and every engine has a verdict to
/// preserve.
fn full_evaluation_spec(directory: &Path, shape: FullEvaluation, n: usize) -> PathBuf {
    let spec = format!(
        "spec Deep{}{n} {{\n  state {{ x: Int }}\n  init {{ x = 0 }}\n  \
         action bump() {{\n    requires x < 2\n    x = x + 1\n  }}\n  \
         invariant DeepChain {{\n    {}\n  }}\n}}\n",
        shape.name(),
        shape.invariant(n)
    );
    let path = directory.join(format!("full_evaluation_{}_{n}.fsl", shape.name()));
    std::fs::write(&path, spec).expect("write full-evaluation spec");
    path
}

/// Runs `verify` under `engine` on every full-evaluation shape and requires the
/// ordinary verdict and exit 0.
fn verify_full_evaluation(engine: &str, expected: &str) {
    let directory = scratch_dir(&format!("full-{engine}"));
    for shape in FullEvaluation::ALL {
        let spec = full_evaluation_spec(&directory, shape, FULL_EVALUATION_STAGES);
        let path = spec.to_str().expect("spec path");
        let label = format!("verify --engine {engine} ({})", shape.name());

        let output = run(&["verify", path, "--engine", engine, "--depth", "2"]);
        let status = exit_code(&label, FULL_EVALUATION_STAGES, &output);
        let envelope = json(&label, &output);

        assert_eq!(
            envelope["result"], expected,
            "`fslc {label}`: the invariant holds, so a different verdict means \
             a stack guard changed an answer; envelope={envelope}"
        );
        assert_eq!(status, 0, "`fslc {label}`; envelope={envelope}");
    }
}

/// bmc runs the concrete monitor (`prepare_bmc` -> `find_boundary_violation`)
/// before the solver, and then the symbolic `eval` and definedness walk.
#[test]
fn bmc_verifies_fully_evaluated_deep_invariants_instead_of_overflowing_the_stack() {
    verify_full_evaluation("bmc", "verified");
}

/// induction shares bmc's concrete monitor and symbolic evaluation.
#[test]
fn induction_proves_fully_evaluated_deep_invariants_instead_of_overflowing_the_stack() {
    verify_full_evaluation("induction", "proved");
}

/// explicit evaluates each invariant concretely in every reached state
/// (`verify_explicit_selected` -> `current_violation_selected`).
#[test]
fn explicit_verifies_fully_evaluated_deep_invariants_instead_of_overflowing_the_stack() {
    verify_full_evaluation("explicit", "verified");
}

/// auto routes this finite spec to one of the above; tested separately so a
/// routing change cannot move it onto an unguarded path unseen.
#[test]
fn auto_verifies_fully_evaluated_deep_invariants_instead_of_overflowing_the_stack() {
    verify_full_evaluation("auto", "verified");
}

/// `check` on the same shapes, deeper: it reaches `extend_pattern_binding` once
/// per `Binary` level and `reserved::check_expr` over the whole tree, neither
/// of which passes back through a guarded cycle.
#[test]
fn check_accepts_fully_evaluated_deep_invariants_instead_of_overflowing_the_stack() {
    let directory = scratch_dir("full-check");
    for shape in FullEvaluation::ALL {
        let spec = full_evaluation_spec(&directory, shape, CHECK_STAGES);
        let label = format!("check ({})", shape.name());

        let output = run(&["check", spec.to_str().expect("spec path")]);
        let status = exit_code(&label, CHECK_STAGES, &output);
        assert_eq!(
            status,
            0,
            "`fslc {label}`; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Number of `+ 0` terms in the `helpful` witness below.
///
/// With the guards removed, a debug arm64 build aborts `verify --engine
/// induction` between N=1000 and N=2000, in `expr_text_with_origins` while
/// rendering the `helpful` argument into the success envelope. 2400 is above
/// that and below the derived-`Clone` band.
const HELPFUL_STAGES: usize = 2400;

/// A `helpful` argument is rendered as text into the induction envelope and
/// folded by `induction::eval_state_independent`; both walk its whole tree.
#[test]
fn induction_renders_a_deeply_nested_helpful_argument_instead_of_overflowing_the_stack() {
    let directory = scratch_dir("helpful");
    let argument = format!("c{}", " + 0".repeat(HELPFUL_STAGES));
    let spec = format!(
        "spec DeepHelpful{HELPFUL_STAGES} {{\n  type Case = 0..1\n  type Level = 0..2\n  \
         state {{ level: Map<Case, Level> }}\n  \
         init {{ forall c: Case {{ level[c] = 2 }} }}\n  \
         fair action step(c: Case) {{\n    requires level[c] > 0\n    \
         level[c] = level[c] - 1\n  }}\n  \
         invariant NonNeg {{ forall c: Case {{ level[c] >= 0 }} }}\n  \
         leadsTo Responds {{\n    forall c: Case {{ level[c] > 0 ~> level[c] == 0 }}\n    \
         helpful step({argument})\n    decreases level[c]\n  }}\n}}\n"
    );
    let path = directory.join(format!("helpful_{HELPFUL_STAGES}.fsl"));
    std::fs::write(&path, spec).expect("write helpful spec");

    let label = "verify --engine induction (helpful)";
    let output = run(&[
        "verify",
        path.to_str().expect("spec path"),
        "--engine",
        "induction",
        "--depth",
        "2",
    ]);
    let status = exit_code(label, HELPFUL_STAGES, &output);
    let envelope = json(label, &output);
    assert_eq!(envelope["result"], "proved", "envelope={envelope}");
    assert_eq!(status, 0, "envelope={envelope}");
}

/// Writes a spec whose *invariant* carries a right-nested `if` chain of length
/// `n`, so the depth is inside the spec rather than in a mapping.
fn deep_invariant_spec(directory: &Path, n: usize) -> PathBuf {
    assert!(n >= 2, "a chain needs at least two arms");
    let mut chain = String::from("0");
    for k in (0..n).rev() {
        chain = format!("if x == {k} then {k}\n      else {chain}");
    }
    let spec = format!(
        "spec DeepInvariant{n} {{\n  state {{ x: Int, seen: Int }}\n  init {{ x = 0  seen = 0 }}\n  \
         action bump() {{\n    requires x < {n}\n    x = x + 1\n    seen = seen + 1\n  }}\n  \
         invariant DeepChain {{\n    ({chain}) >= 0\n  }}\n}}\n"
    );
    let path = directory.join(format!("deep_invariant_{n}.fsl"));
    std::fs::write(&path, spec).expect("write deep invariant spec");
    path
}

/// The generator has to keep producing a *deep* file for any of the above to
/// mean anything.
///
/// `WITNESS_STAGES` being above the crash threshold is checked at compile time;
/// this checks the other half, that the generator still emits nesting rather
/// than a flat file, which no compile-time assertion can see.
#[test]
fn the_generated_witness_is_still_deeper_than_the_unguarded_crash_threshold() {
    let directory = scratch_dir("shape");
    let trio = refinement_trio(&directory, WITNESS_STAGES);
    let mapping = std::fs::read_to_string(&trio.mapping).expect("read map");

    let nesting = mapping.matches("if st == I").count();
    assert_eq!(
        nesting,
        2 * WITNESS_STAGES - 1,
        "the mapping's if-chain is the deep structure under test"
    );

    let spec = std::fs::read_to_string(deep_invariant_spec(&directory, WITNESS_STAGES))
        .expect("read deep invariant spec");
    assert_eq!(
        spec.matches("if x ==").count(),
        WITNESS_STAGES,
        "the invariant's if-chain is the deep structure under test"
    );
}

/// How deep the evaluator tests below nest one expression. A release build of
/// an evaluator walk that recurses without the guard overflows a 1 MiB stack
/// well below this.
const EVALUATED_DEPTH: usize = 16_384;

/// The stack each evaluator test runs on: small, so the guard has to grow it.
const EVALUATOR_STACK: usize = 1024 * 1024;

/// A model whose state `m` maps every key to `0`, so `m[0]`, `m[m[0]]`, and
/// every deeper nesting read `0`.
fn constant_map_model() -> (fsl_core::KernelModel, fsl_runtime::State) {
    let source = "spec ConstantMap {\n  type K = 0..1\n  state { m: Map<K, K> }\n  \
                  init { forall k: K { m[k] = 0 } }\n  action stay() { m = m }\n}\n";
    let document = fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new("."))
        .expect("parse constant map model");
    let model = fsl_core::build_model(document).expect("build constant map model");
    let state = fsl_runtime::Monitor::new(model.clone())
        .expect("initialize constant map model")
        .state;
    (model, state)
}

/// `fsl_runtime::eval` on `expr` with no bindings, on an [`EVALUATOR_STACK`]
/// thread.
fn eval_on_small_stack(
    model: &fsl_core::KernelModel,
    state: &fsl_runtime::State,
    expr: &fsl_core::KernelExpr,
) -> Result<fsl_core::FslValue, fsl_runtime::RuntimeError> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(EVALUATOR_STACK)
            .spawn_scoped(scope, || {
                fsl_runtime::eval(
                    expr,
                    state,
                    &mut std::collections::BTreeMap::new(),
                    model,
                    None,
                )
            })
            .expect("spawn evaluator thread")
            .join()
            .expect("evaluator thread")
    })
}

/// `m[m[...m[0]...]]`, nested `depth` deep in the key.
fn nested_key(depth: usize) -> fsl_core::KernelExpr {
    let mut expr = fsl_core::KernelExpr::Num(0);
    for _ in 0..depth {
        expr = fsl_core::KernelExpr::Index(
            Box::new(fsl_core::KernelExpr::Var("m".to_owned())),
            Box::new(expr),
        );
    }
    expr
}

/// Takes a [`nested_key`] chain apart one level at a time: the derived `Drop`
/// recurses once per level.
fn drop_nested_key(mut expr: fsl_core::KernelExpr) {
    while let fsl_core::KernelExpr::Index(_, key) = expr {
        expr = *key;
    }
}

/// The in-place read of an `Index` chain recurses on its key without passing
/// through `eval`'s own guard, both when the chain is evaluated and when it is
/// an operand of `==`.
#[test]
fn eval_reads_a_deeply_nested_map_key_instead_of_overflowing_the_stack() {
    use fsl_core::{FslValue, KernelExpr};

    let (model, state) = constant_map_model();
    let chain = nested_key(EVALUATED_DEPTH);
    assert_eq!(
        eval_on_small_stack(&model, &state, &chain).expect("evaluate the nested key"),
        FslValue::Int(0)
    );
    let comparison = KernelExpr::Binary {
        op: "==".to_owned(),
        left: Box::new(chain),
        right: Box::new(KernelExpr::Num(0)),
    };
    assert_eq!(
        eval_on_small_stack(&model, &state, &comparison).expect("compare the nested key"),
        FslValue::Bool(true)
    );
    let KernelExpr::Binary { left, .. } = comparison else {
        unreachable!("built as a comparison");
    };
    drop_nested_key(*left);
}

/// The in-place read failing deep inside the key leaves `eval` to read the
/// chain step by step, and each step evaluates its key the same way. Here the
/// innermost key is outside the map's domain, so every level is read that way.
#[test]
fn eval_reports_a_deeply_nested_missing_map_key_instead_of_overflowing_the_stack() {
    let (model, state) = constant_map_model();
    let mut chain = nested_key(EVALUATED_DEPTH);
    let mut innermost = &mut chain;
    while let fsl_core::KernelExpr::Index(_, key) = innermost {
        innermost = key;
    }
    *innermost = fsl_core::KernelExpr::Num(5);
    let error = eval_on_small_stack(&model, &state, &chain).expect_err("m[5] is outside");
    assert!(
        error.map_key_outside_domain,
        "the innermost lookup fails: {error:?}"
    );
    drop_nested_key(chain);
}

/// `m[0] == 0`, which holds in [`constant_map_model`].
fn true_term() -> fsl_core::KernelExpr {
    fsl_core::KernelExpr::Binary {
        op: "==".to_owned(),
        left: Box::new(fsl_core::KernelExpr::Index(
            Box::new(fsl_core::KernelExpr::Var("m".to_owned())),
            Box::new(fsl_core::KernelExpr::Num(0)),
        )),
        right: Box::new(fsl_core::KernelExpr::Num(0)),
    }
}

/// `terms` copies of [`true_term`] joined by `and`, nested to the left or to
/// the right.
fn conjunction(terms: usize, left_nested: bool) -> fsl_core::KernelExpr {
    let mut expr = true_term();
    for _ in 1..terms {
        let (left, right) = if left_nested {
            (expr, true_term())
        } else {
            (true_term(), expr)
        };
        expr = fsl_core::KernelExpr::Binary {
            op: "and".to_owned(),
            left: Box::new(left),
            right: Box::new(right),
        };
    }
    expr
}

/// Takes a [`conjunction`] apart one level at a time.
fn drop_conjunction(mut expr: fsl_core::KernelExpr) {
    while let fsl_core::KernelExpr::Binary { op, left, right } = expr {
        if op != "and" {
            break;
        }
        expr = if matches!(*left, fsl_core::KernelExpr::Binary { ref op, .. } if op == "and") {
            *left
        } else {
            *right
        };
    }
}

/// An `and` chain is evaluated connective by connective without passing
/// through `eval` between them, so every conjunct is evaluated.
#[test]
fn eval_decides_a_deeply_nested_conjunction_instead_of_overflowing_the_stack() {
    let (model, state) = constant_map_model();
    for left_nested in [true, false] {
        let chain = conjunction(EVALUATED_DEPTH, left_nested);
        assert_eq!(
            eval_on_small_stack(&model, &state, &chain).expect("evaluate the conjunction"),
            fsl_core::FslValue::Bool(true),
            "left-nested: {left_nested}"
        );
        drop_conjunction(chain);
    }
}

/// A model whose state `o` is `some(1)`, and the operand of its invariant
/// that binds `v` from it: `o is some(v)`.
fn bound_option_model() -> (
    fsl_core::KernelModel,
    fsl_runtime::State,
    fsl_core::KernelExpr,
) {
    let source = "spec BoundOption {\n  type K = 0..1\n  state { m: Map<K, K>, o: Option<K> }\n  \
                  init {\n    forall k: K { m[k] = 0 }\n    o = some(1)\n  }\n  \
                  action stay() { m = m }\n  invariant Bound { o is some(v) and v == 1 }\n}\n";
    let document = fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new("."))
        .expect("parse bound option model");
    let model = fsl_core::build_model(document).expect("build bound option model");
    let state = fsl_runtime::Monitor::new(model.clone())
        .expect("initialize bound option model")
        .state;
    let fsl_core::KernelExpr::Binary { left, .. } = model.invariants[0].expr.clone() else {
        unreachable!("the invariant is a conjunction");
    };
    (model, state, *left)
}

/// `v == value`.
fn v_equals(value: i64) -> fsl_core::KernelExpr {
    fsl_core::KernelExpr::Binary {
        op: "==".to_owned(),
        left: Box::new(fsl_core::KernelExpr::Var("v".to_owned())),
        right: Box::new(fsl_core::KernelExpr::Num(value)),
    }
}

/// `(c and c and ... and o is some(v) ... and v == 0) or v == 1`, with
/// `terms` conjuncts before `v == 0` and the `is` deepest, the conjunction
/// nested to the left or to the right. The `is` binds `v` to 1 inside the
/// left operand of `or` only, so `v == 1` reads the caller's `v`.
fn spine_binding_at_the_far_end(
    is_term: &fsl_core::KernelExpr,
    terms: usize,
    left_nested: bool,
) -> fsl_core::KernelExpr {
    let binary = |op: &str, left, right| fsl_core::KernelExpr::Binary {
        op: op.to_owned(),
        left: Box::new(left),
        right: Box::new(right),
    };
    let mut conjunction = is_term.clone();
    for _ in 1..terms {
        conjunction = if left_nested {
            binary("and", conjunction, true_term())
        } else {
            binary("and", true_term(), conjunction)
        };
    }
    binary("or", binary("and", conjunction, v_equals(0)), v_equals(1))
}

/// Takes a [`spine_binding_at_the_far_end`] apart one level at a time.
fn drop_spine_binding(expr: fsl_core::KernelExpr) {
    let fsl_core::KernelExpr::Binary { left, .. } = expr else {
        return;
    };
    let fsl_core::KernelExpr::Binary { left, .. } = *left else {
        return;
    };
    drop_conjunction(*left);
}

/// Whether an `and`/`or`/`=>` spine can bind is decided by a walk that
/// continues into a connective operand and sets nothing aside when the other
/// operand ends the spine, so the `is` at the far end of a spine nested to one
/// side is reached at every length. The binding it makes stays out of the
/// right operand of `or`, which reads the caller's `v == 0` and is false.
#[test]
fn eval_keeps_a_binding_from_the_far_end_of_a_long_spine_in_its_scope() {
    let (model, state, is_term) = bound_option_model();
    for terms in [1, 7, 8, 9, 10, 17, EVALUATED_DEPTH] {
        for left_nested in [true, false] {
            let spine = spine_binding_at_the_far_end(&is_term, terms, left_nested);
            let value = std::thread::scope(|scope| {
                std::thread::Builder::new()
                    .stack_size(EVALUATOR_STACK)
                    .spawn_scoped(scope, || {
                        let mut bindings = std::collections::BTreeMap::from([(
                            "v".to_owned(),
                            fsl_core::FslValue::Int(0),
                        )]);
                        fsl_runtime::eval(&spine, &state, &mut bindings, &model, None)
                    })
                    .expect("spawn evaluator thread")
                    .join()
                    .expect("evaluator thread")
            });
            assert_eq!(
                value.expect("evaluate the spine"),
                fsl_core::FslValue::Bool(false),
                "terms: {terms}, left-nested: {left_nested}"
            );
            drop_spine_binding(spine);
        }
    }
}

/// `(c and c and ... and v == 0) or v == 1` with `terms` conjuncts `c`, each
/// itself `true_term() and true_term()` and nested to the left, so the walk
/// sets one conjunct aside per level below the first. The `is` replaces the
/// left operand of the outermost conjunct, which is set aside first and taken
/// back last, or of the innermost one set aside, which is set aside last and
/// taken back first.
fn spine_binding_set_aside(
    is_term: &fsl_core::KernelExpr,
    terms: usize,
    in_outermost: bool,
) -> fsl_core::KernelExpr {
    let binary = |op: &str, left, right| fsl_core::KernelExpr::Binary {
        op: op.to_owned(),
        left: Box::new(left),
        right: Box::new(right),
    };
    let binding_level = if in_outermost { terms - 1 } else { 1 };
    let mut conjunction = binary("and", true_term(), true_term());
    for level in 1..terms {
        let first = if level == binding_level {
            is_term.clone()
        } else {
            true_term()
        };
        conjunction = binary("and", conjunction, binary("and", first, true_term()));
    }
    binary("or", binary("and", conjunction, v_equals(0)), v_equals(1))
}

/// The walk holds eight operands set aside on the stack and the rest on the
/// heap. With more than eight set aside, the `is` set aside last is taken
/// back from the heap and the one set aside first from the stack. Either way
/// the spine is found to bind, so its binding stays out of the right operand
/// of `or` as above.
#[test]
fn eval_keeps_a_binding_set_aside_by_the_spine_walk_in_its_scope() {
    let (model, state, is_term) = bound_option_model();
    for terms in [2, 8, 9, 10, 17] {
        for in_outermost in [true, false] {
            let spine = spine_binding_set_aside(&is_term, terms, in_outermost);
            let mut bindings =
                std::collections::BTreeMap::from([("v".to_owned(), fsl_core::FslValue::Int(0))]);
            let value = fsl_runtime::eval(&spine, &state, &mut bindings, &model, None);
            assert_eq!(
                value.expect("evaluate the spine"),
                fsl_core::FslValue::Bool(false),
                "terms: {terms}, in the outermost operand: {in_outermost}"
            );
        }
    }
}

/// A `requires` that is `o is some(v) or m[0] == 0` is an `or` whose left
/// operand is the `is` itself. Its binding stays in that operand, so a later
/// `requires v == 1` cannot read it.
#[test]
fn a_requires_disjunct_that_binds_does_not_bind_for_a_later_guard() {
    let (mut model, state, is_term) = bound_option_model();
    let requires = fsl_core::KernelExpr::Binary {
        op: "or".to_owned(),
        left: Box::new(is_term),
        right: Box::new(true_term()),
    };
    model.actions[0].guards = vec![
        fsl_core::ActionGuard::Requires(requires),
        fsl_core::ActionGuard::Requires(v_equals(1)),
    ];
    let error = fsl_runtime::Monitor::from_state(model, state)
        .enabled()
        .expect_err("the later guard reads an unbound name");
    assert!(error.message.contains("'v'"), "{error:?}");
}
