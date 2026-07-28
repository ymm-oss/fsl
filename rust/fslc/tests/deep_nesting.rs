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
fn exit_code(label: &str, output: &Output) -> i32 {
    output.status.code().unwrap_or_else(|| {
        panic!(
            "`fslc {label}` on a {WITNESS_STAGES}-stage witness died on a signal \
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
    let status = exit_code("refine", &output);
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
    let status = exit_code("check", &output);
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
    let status = exit_code("fmt", &output);

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
}
