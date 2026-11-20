// SPDX-License-Identifier: Apache-2.0
//! The solver query sequence of bounded verification on specs with partial
//! operations in guards, bodies and `ensures` (#1874). The expected values
//! were recorded on `4c664d89`, before the per-instance definedness probes
//! moved into one function; a change that adds, drops, reorders or rebuilds a
//! query changes them. The separate witness regression also pins evidence
//! affected by native SMT term lifetimes even when formulas stay identical.
use fsl_core::{FsResolver, KernelModel, build_model, parse_kernel_source};
use fsl_solver::{
    CheckFuture, ModelFuture, ModelValue, SatResult, SmtSolver, SolverResult, Sort,
    VerificationStatistics,
};
use std::collections::BTreeMap;
use std::fmt::Write as _;

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
    {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}

fn model(source: &str) -> KernelModel {
    build_model(parse_kernel_source(source, &FsResolver::new(".")).unwrap()).unwrap()
}

/// One `set_query_context` call and the answers of the checks issued under it.
struct Segment {
    kind: String,
    name: String,
    answers: String,
}

/// Records the query sequence; every solver operation delegates to Z3.
struct RecordingSolver {
    inner: fsl_solver_z3::Z3Solver,
    segments: Vec<Segment>,
    digest: u64,
}

impl RecordingSolver {
    fn new() -> Self {
        Self {
            inner: fsl_solver_z3::Z3Solver::new().unwrap(),
            segments: vec![Segment {
                kind: String::new(),
                name: String::new(),
                answers: String::new(),
            }],
            digest: 0xcbf2_9ce4_8422_2325,
        }
    }
    /// FNV-1a over every assertion, assumption, scope operation and answer.
    fn absorb(&mut self, event: &str) {
        for byte in event.bytes().chain(*b"\n") {
            self.digest = (self.digest ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn answer(&mut self, result: &SolverResult<SatResult>) {
        let letter = match result {
            Ok(SatResult::Sat) => 's',
            Ok(SatResult::Unsat) => 'u',
            Ok(SatResult::Unknown) => '?',
            Err(_) => 'e',
        };
        self.segments.last_mut().unwrap().answers.push(letter);
        self.absorb(&letter.to_string());
    }
}

macro_rules! delegate {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $result:ty;)*) => {
        $(fn $name(&self,$($arg:$ty),*) -> $result {self.inner.$name($($arg),*)})*
    };
}

impl SmtSolver for RecordingSolver {
    type Term = <fsl_solver_z3::Z3Solver as SmtSolver>::Term;
    delegate! {
        version() -> &str;
        statistics() -> VerificationStatistics;
        sort(term: &Self::Term) -> Sort;
        literal_value(term: &Self::Term) -> Option<ModelValue>;
        bool_value(value: bool) -> Self::Term;
        int_value(value: i64) -> Self::Term;
        constant(name: &str, sort: &Sort) -> SolverResult<Self::Term>;
        not(term: &Self::Term) -> SolverResult<Self::Term>;
        and(terms: &[Self::Term]) -> SolverResult<Self::Term>;
        or(terms: &[Self::Term]) -> SolverResult<Self::Term>;
        implies(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        equal(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        ite(condition: &Self::Term, then_term: &Self::Term, else_term: &Self::Term) -> SolverResult<Self::Term>;
        neg(term: &Self::Term) -> SolverResult<Self::Term>;
        add(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        sub(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        mul(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        div(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        modulo(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        lt(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        le(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        gt(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        ge(left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term>;
        const_array(domain: &Sort, value: &Self::Term) -> SolverResult<Self::Term>;
        select(array: &Self::Term, index: &Self::Term) -> SolverResult<Self::Term>;
        store(array: &Self::Term, index: &Self::Term, value: &Self::Term) -> SolverResult<Self::Term>;
        substitute(term: &Self::Term, replacements: &[(Self::Term, Self::Term)]) -> SolverResult<Self::Term>;
        unsat_core() -> SolverResult<Vec<Self::Term>>;
        model_eval(term: &Self::Term) -> SolverResult<Option<ModelValue>>;
    }
    fn resolve_output_model(&mut self) -> ModelFuture<'_> {
        self.inner.resolve_output_model()
    }
    fn set_query_context(&mut self, kind: &str, name: &str) {
        self.segments.push(Segment {
            kind: kind.to_owned(),
            name: name.to_owned(),
            answers: String::new(),
        });
        self.inner.set_query_context(kind, name);
    }
    fn push(&mut self) {
        self.absorb("push");
        self.inner.push();
    }
    fn pop(&mut self, levels: u32) -> SolverResult<()> {
        self.absorb(&format!("pop {levels}"));
        self.inner.pop(levels)
    }
    fn reset(&mut self) -> SolverResult<()> {
        self.absorb("reset");
        self.inner.reset()
    }
    fn assert(&mut self, term: &Self::Term) -> SolverResult<()> {
        self.absorb(&format!("assert {term:?}"));
        self.inner.assert(term)
    }
    fn assert_and_track(&mut self, term: &Self::Term, tracker: &Self::Term) -> SolverResult<()> {
        self.absorb(&format!("track {term:?} {tracker:?}"));
        self.inner.assert_and_track(term, tracker)
    }
    fn check(&mut self) -> CheckFuture<'_> {
        self.absorb("check");
        Box::pin(async move {
            let result = self.inner.check().await;
            self.answer(&result);
            result
        })
    }
    fn check_assuming(&mut self, assumptions: &[Self::Term]) -> CheckFuture<'_> {
        self.absorb(&format!("assuming {assumptions:?}"));
        let assumptions = assumptions.to_vec();
        Box::pin(async move {
            let result = self.inner.check_assuming(&assumptions).await;
            self.answer(&result);
            result
        })
    }
}

/// The outcome, the checks per `set_query_context` kind, the `partial_op`
/// segments in order (name and answers), and the digest of every query.
fn record(source: &str, depth: usize) -> String {
    let model = model(source);
    let mut solver = RecordingSolver::new();
    let outcome = match block_on(fsl_verifier::verify_bounded(&model, &mut solver, depth)) {
        Ok(result) => match result.violation {
            Some(violation) => format!(
                "violated {} {} step {}",
                violation.kind, violation.name, violation.step
            ),
            None => "verified".to_owned(),
        },
        Err(error) => format!("error {error}"),
    };
    let mut per_kind = BTreeMap::<&str, usize>::new();
    for segment in &solver.segments {
        *per_kind.entry(&segment.kind).or_default() += segment.answers.len();
    }
    per_kind.retain(|_, checks| *checks > 0);
    let mut rendered = format!("{outcome}\n{per_kind:?}\n");
    for segment in solver.segments.iter().filter(|s| s.kind == "partial_op") {
        writeln!(rendered, "{}:{}", segment.name, segment.answers).unwrap();
    }
    write!(rendered, "{:016x}", solver.digest).unwrap();
    rendered
}

const SAFE: &str = r"
spec ProbeSequence {
  type Item = 0..2
  type D = 1..2
  state { queue: Seq<Item, 2>, d: D, q: Item, n: Item }
  init { queue = Seq {}  d = 1  q = 0  n = 0 }
  action put(v: Item) { requires queue.size() < 2  queue = queue.push(v) }
  action peek() {
    requires queue.size() > 0
    requires queue.head() <= 2
    q = 2 / d
    ensures queue.size() == 0 or queue.head() >= 0
  }
  action tick() { requires n < 2  n = n + 1 }
}
";

const GUARD_PARTIAL: &str = r"
spec GuardPartial {
  type Item = 0..1
  state { queue: Seq<Item, 1>, n: Item }
  init { queue = Seq {}  n = 0 }
  action tick() { requires n < 1  n = n + 1 }
  action inspect() { requires n == 1  requires queue.head() == 0  queue = queue }
}
";

const BODY_PARTIAL: &str = r"
spec BodyPartial {
  type D = 0..2
  state { d: D, q: Int }
  init { d = 1  q = 0 }
  action zero() { d = 0 }
  action div() { q = 1 / d }
}
";

/// The search's typed check meets the guard overflow first: `a` has a
/// division candidate.
const SEARCH_GUARD_NON_PARTIAL: &str = r"
spec SearchGuardNonPartial {
  state { junk: Int, x: Int, d: Int }
  init { x = 9223372036854775807  d = 1 }
  action a() { requires x + 1 > 0 and 10 / d > 0  junk = 1 }
}
";

/// The definedness pass meets `a`'s guard overflow: `a` has no candidate.
const PASS_GUARD_NON_PARTIAL: &str = r"
spec PassGuardNonPartial {
  state { junk: Int, x: Int, d: Int }
  init { x = 9223372036854775807 }
  action a() { requires x + 1 > 0  junk = 1 }
  action b() { requires 10 / d > 0  junk = 2 }
}
";

const SEARCH_BODY_NON_PARTIAL: &str = r"
spec SearchBodyNonPartial {
  type K = 0..3
  type V = 0..1
  type I = 0..5
  type D = 0..2
  state { junk: Bool, m: Map<K, V>, i: I, d: D, q: V }
  init {
    forall k: K { m[k] = 0 }
    i = 0
    d = 1
    q = 0
  }
  action step() { requires i < 5  i = i + 1 }
  action write() {
    m[i] = 1
    q = 1 / d
  }
}
";

const PASS_BODY_NON_PARTIAL: &str = r"
spec PassBodyNonPartial {
  state { junk: Bool, x: Int }
  init { x = 0 }
  action inc() { x = x + 2305843009213693952 }
  invariant NonNegative { x >= 0 }
}
";

const ENSURES_PARTIAL: &str = r"
spec EnsuresPartial {
  type Item = 0..1
  state { queue: Seq<Item, 1>, n: Item }
  init { queue = Seq {}  n = 0 }
  action tick() { requires n < 1  n = n + 1 }
  action inspect() { requires n == 1  queue = queue  ensures queue.head() == 0 }
}
";

const EXPECTED_SAFE: &str = r#"verified
{"action_coverage": 4, "deadlock": 4, "ensures": 9, "init": 1, "partial_op": 42, "type_bound": 16, "vacuity": 5}
peek:uuuu
peek:uuuu
peek:uuuu
actions:suuuuss
actions:u
put:
put:
put:
peek:uu
tick:
actions:suuuuss
actions:u
put:
put:
put:
peek:uu
tick:
actions:suuuuss
actions:u
put:
put:
put:
peek:uu
tick:
bda93ceb8a496cfb"#;

const EXPECTED_GUARD_PARTIAL: &str = r#"violated partial_op _partial_inspect step 2
{"action_coverage": 2, "deadlock": 1, "init": 1, "partial_op": 21, "type_bound": 4}
inspect:uuuu
inspect:ss
actions:suus
actions:u
tick:
inspect:u
actions:suus
actions:s
tick:uu
inspect:ss
fdf59d4cb9705df0"#;

const EXPECTED_BODY_PARTIAL: &str = r#"violated partial_op _partial_div step 2
{"action_coverage": 2, "deadlock": 1, "init": 1, "partial_op": 18, "type_bound": 4}
div:uuuu
div:uuss
actions:u
actions:u
zero:
div:u
actions:u
actions:s
zero:uu
div:uss
57e12371a240fcd4"#;

const EXPECTED_SEARCH_GUARD_NON_PARTIAL: &str = r#"error action 'a' guard evaluation has a non-partial failure
{"init": 1, "partial_op": 9, "type_bound": 3}
a:us
actions:suss
actions:s
a:us
249296658ba69aa0"#;

const EXPECTED_PASS_GUARD_NON_PARTIAL: &str = r#"error action 'a' guard evaluation has a non-partial failure
{"init": 1, "partial_op": 8, "type_bound": 3}
b:ss
actions:ssss
actions:s
a:s
7ed018754f74ba63"#;

const EXPECTED_SEARCH_BODY_NON_PARTIAL: &str = r#"error action 'write' body evaluation has a non-partial failure
{"action_coverage": 2, "deadlock": 4, "init": 1, "partial_op": 39, "type_bound": 25}
write:uuuu
write:uuuu
write:uuuu
write:uuuu
write:uuus
actions:u
actions:u
step:
write:u
actions:u
actions:u
step:
write:u
actions:u
actions:u
step:
write:u
actions:u
actions:u
step:
write:u
actions:u
actions:s
step:uu
write:uus
ca958458cfdaabc1"#;

const EXPECTED_PASS_BODY_NON_PARTIAL: &str = r#"error action 'inc' body evaluation has a non-partial failure
{"action_coverage": 1, "deadlock": 5, "init": 1, "invariant": 5, "partial_op": 18, "type_bound": 10, "vacuity": 1}
NonNegative:u
NonNegative:u
NonNegative:u
NonNegative:u
NonNegative:u
actions:u
actions:u
inc:
actions:ss
actions:u
inc:
actions:ss
actions:u
inc:
actions:ss
actions:s
inc:us
67f9d19bbc52c906"#;

const EXPECTED_ENSURES_PARTIAL: &str = r#"violated partial_op _partial_inspect step 2
{"action_coverage": 3, "deadlock": 2, "ensures": 5, "init": 1, "partial_op": 18, "type_bound": 6}
inspect:uuuu
inspect:uuuu
actions:suus
actions:u
tick:
inspect:
actions:suus
actions:u
tick:
inspect:
cb2fabe1ad3dcdd3"#;

#[test]
fn definedness_probe_sequences_are_unchanged() {
    let cases: [(&str, &str, usize, &str); 8] = [
        ("safe", SAFE, 3, EXPECTED_SAFE),
        ("guard-partial", GUARD_PARTIAL, 3, EXPECTED_GUARD_PARTIAL),
        ("body-partial", BODY_PARTIAL, 3, EXPECTED_BODY_PARTIAL),
        (
            "search-guard-non-partial",
            SEARCH_GUARD_NON_PARTIAL,
            3,
            EXPECTED_SEARCH_GUARD_NON_PARTIAL,
        ),
        (
            "pass-guard-non-partial",
            PASS_GUARD_NON_PARTIAL,
            3,
            EXPECTED_PASS_GUARD_NON_PARTIAL,
        ),
        (
            "search-body-non-partial",
            SEARCH_BODY_NON_PARTIAL,
            8,
            EXPECTED_SEARCH_BODY_NON_PARTIAL,
        ),
        (
            "pass-body-non-partial",
            PASS_BODY_NON_PARTIAL,
            4,
            EXPECTED_PASS_BODY_NON_PARTIAL,
        ),
        (
            "ensures-partial",
            ENSURES_PARTIAL,
            3,
            EXPECTED_ENSURES_PARTIAL,
        ),
    ];
    for (name, source, depth, expected) in cases {
        let first = record(source, depth);
        let second = record(source, depth);
        assert_eq!(first, second, "{name}: the recording is not deterministic");
        assert_eq!(first, expected, "{name}");
    }
}

/// Full BMC evidence from the base, rather than only query text. Shortening
/// the search's guard-undefined term lifetime changes this deadlock witness.
/// The solver is `verdicts_only`: a re-solved witness (#1940) no longer
/// depends on the session's term history, so only the session's own model
/// still shows a changed lifetime.
#[test]
fn search_probe_term_lifetimes_preserve_witnesses() {
    // Base measurements (d1df480b / 0cb57b69), before #1874:
    // Linux aarch64, this machine, bundled Z3: 15723122683289713882
    // Linux x86_64, CI ubuntu-latest, bundled Z3: 15856676758750849773
    // Windows x86_64, CI windows-latest, native Z3 4.16: 15856676758750849773
    // macOS aarch64, CI macos-15, native Z3 4.16: 15856676758750849773
    let expected = if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some(15_723_122_683_289_713_882)
    } else if cfg!(all(
        any(target_os = "linux", target_os = "windows"),
        target_arch = "x86_64"
    )) || cfg!(all(target_os = "macos", target_arch = "aarch64"))
    {
        Some(15_856_676_758_750_849_773)
    } else {
        None
    };
    let mut first = None;
    for _ in 0..2 {
        // Z3's default context is thread-local. Give each run a fresh context
        // so previous runs' AST allocation history cannot select the witness.
        let digest = std::thread::spawn(|| {
            let model = model(include_str!(
                "../../../examples/gallery/adversarial/option_struct_set_seq_combo.fsl"
            ));
            let mut solver = fsl_solver_z3::Z3Solver::new().unwrap().verdicts_only();
            let result = block_on(fsl_verifier::verify_bounded(&model, &mut solver, 8)).unwrap();
            let mut digest = 0xcbf2_9ce4_8422_2325_u64;
            for byte in format!("{result:?}").bytes() {
                digest = (digest ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
            digest
        })
        .join()
        .unwrap();
        if let Some(first) = first {
            assert_eq!(digest, first, "full BMC evidence is not deterministic");
        }
        first = Some(digest);
        if let Some(expected) = expected {
            assert_eq!(digest, expected, "full BMC evidence differs from base");
        } else {
            eprintln!(
                "unmeasured platform {} {}: full BMC evidence digest {digest}",
                std::env::consts::OS,
                std::env::consts::ARCH
            );
        }
    }
}
