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

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
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
    Command::new(env!("CARGO_BIN_EXE_fslc"))
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
