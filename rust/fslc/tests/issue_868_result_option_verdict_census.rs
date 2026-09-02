// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Drift control for #868: `Result<Option<V>, E>` does not inherit `V`'s
//! `#[must_use]`, so a successful verdict can otherwise disappear at a
//! statement boundary. This census classifies every such return signature by
//! its defining function, then rejects direct statement-position discards of
//! verdict carriers. Direct shapes and source-declared aliases are resolved
//! recursively; an alias whose right-hand side cannot be resolved is itself a
//! required classification decision. The classification is bidirectionally
//! live: a newly discovered signature needs an entry and a stale entry is an
//! error. External macro invocations are deliberately outside this
//! token-based census because their generated signatures are not present in
//! the invocation's source tokens.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Shape {
    Direct,
    Solver,
    Tuple,
    Unresolved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Role {
    Ordinary,
    Verdict,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FoundSignature {
    path: String,
    line: usize,
    function: String,
    shape: Shape,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SignatureIdentity {
    path: String,
    function: String,
    shape: Shape,
}

impl FoundSignature {
    fn identity(&self) -> SignatureIdentity {
        SignatureIdentity {
            path: self.path.clone(),
            function: self.function.clone(),
            shape: self.shape,
        }
    }
}

#[derive(Clone, Copy)]
struct Classification {
    path: &'static str,
    line: usize,
    function: &'static str,
    shape: Shape,
    role: Role,
}

impl Classification {
    fn signature(self) -> FoundSignature {
        FoundSignature {
            path: self.path.to_owned(),
            line: self.line,
            function: self.function.to_owned(),
            shape: self.shape,
        }
    }

    fn identity(self) -> SignatureIdentity {
        self.signature().identity()
    }
}

macro_rules! result_shape {
    (ResultOption) => {
        Shape::Direct
    };
    (SolverResultOption) => {
        Shape::Solver
    };
    (ResultTupleOption) => {
        Shape::Tuple
    };
    (UnresolvedAlias) => {
        Shape::Unresolved
    };
}

macro_rules! entry {
    ($role:ident, $path:literal, $line:literal, $function:literal, $shape:ident) => {
        Classification {
            path: $path,
            line: $line,
            function: $function,
            shape: result_shape!($shape),
            role: Role::$role,
        }
    };
}

// This is deliberately keyed by defining function identity, not an outcome
// type-name suffix. The liveness test below makes it a total, non-decorative
// classification of every discovered signature.
const CLASSIFICATIONS: &[Classification] = &[
    entry!(
        Verdict,
        "rust/fsl-runtime/src/explicit.rs",
        291,
        "record_reachables",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fsl-runtime/src/explicit.rs",
        566,
        "compute_binder_values",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fsl-runtime/src/lib.rs",
        1121,
        "current_violation",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fsl-runtime/src/lib.rs",
        1134,
        "current_violation_selected",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fsl-runtime/src/lib.rs",
        1185,
        "observe",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fsl-runtime/src/lib.rs",
        1193,
        "observe_inner",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fsl-runtime/src/lib.rs",
        2234,
        "first_self_violation",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fsl-runtime/src/lib.rs",
        3199,
        "record_reachables",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fsl-runtime/src/lib.rs",
        3238,
        "check_state",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fsl-runtime/src/lib.rs",
        3248,
        "check_state_selected",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fsl-runtime/src/lib.rs",
        3260,
        "check_state_selected_inner",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fsl-runtime/src/lib.rs",
        348,
        "violating_bindings",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fsl-runtime/src/lib.rs",
        354,
        "search",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fsl-runtime/src/lib.rs",
        1468,
        "refinement_action_instance",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fsl-runtime/src/lib.rs",
        3101,
        "response_pending_at",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fsl-runtime/src/lib.rs",
        3110,
        "response_pending_at_inner",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fsl-runtime/src/lib.rs",
        3387,
        "evaluate_action_guards",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fsl-verifier/src/bmc.rs",
        498,
        "check_state_properties",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fsl-verifier/src/bmc.rs",
        844,
        "check_action_partial_operations",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fsl-verifier/src/bmc.rs",
        1215,
        "check_leadsto_stagnation",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fsl-verifier/src/bmc.rs",
        1293,
        "check_leadsto_deadlines",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fsl-verifier/src/bmc.rs",
        1362,
        "check_leadstos",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fsl-verifier/src/vacuity.rs",
        617,
        "urgency_freeze",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fsl-verifier/src/eval.rs",
        1843,
        "binder_where",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fsl-core/src/domain_lowering.rs",
        553,
        "enum_value",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fsl-core/src/dialect.rs",
        3043,
        "requirements_trace_contract",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fsl-core/src/dialect.rs",
        3165,
        "governance_contract",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fsl-core/src/refinement.rs",
        1249,
        "requirements_implements",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fslc/src/verification_output.rs",
        253,
        "requirements_implements_output",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fslc/src/verification_output.rs",
        302,
        "governance_output",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fslc/src/verification_output.rs",
        328,
        "governance_output_async",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fslc/src/verification_output.rs",
        470,
        "requirement_step_match_values",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fslc/src/verification_output.rs",
        579,
        "validate_requirement_trace_source",
        ResultTupleOption
    ),
    entry!(
        Verdict,
        "rust/fslc/src/main.rs",
        14872,
        "validate_requirement_traces_from_source",
        ResultTupleOption
    ),
    entry!(
        Verdict,
        "rust/fslc/src/main.rs",
        14783,
        "validate_requirement_trace_source",
        ResultTupleOption
    ),
    entry!(
        Ordinary,
        "rust/fslc/src/approval.rs",
        272,
        "reviewed_artifact_digest",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fslc/src/verification.rs",
        172,
        "selected_transition_induction_model",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fslc/src/verification.rs",
        340,
        "monotone_direction",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fslc/src/literate_access.rs",
        111,
        "materialize_literate",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fslc/src/literate_access.rs",
        409,
        "literate_access",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fslc/src/main.rs",
        247,
        "parse_optional_output",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fslc/src/main.rs",
        2316,
        "load_glossary",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fslc/src/main.rs",
        2355,
        "load_evidence",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fslc/src/main.rs",
        2427,
        "load_approvals",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fslc/src/main.rs",
        2804,
        "load_approvals_for_check",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fslc/src/main.rs",
        14937,
        "governance_result_from_source",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fslc/src/main.rs",
        14968,
        "implements_result_from_source",
        ResultOption
    ),
    entry!(
        Verdict,
        "rust/fsl-wasm/src/lib.rs",
        223,
        "governance_output",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fsl-solver/src/lib.rs",
        290,
        "model_eval",
        SolverResultOption
    ),
    entry!(
        Ordinary,
        "rust/fsl-solver-z3/src/lib.rs",
        200,
        "evaluate_model",
        SolverResultOption
    ),
    entry!(
        Ordinary,
        "rust/fsl-solver-z3/src/lib.rs",
        496,
        "model_eval",
        SolverResultOption
    ),
    entry!(
        Ordinary,
        "rust/fsl-solver-z3js/src/lib.rs",
        411,
        "model_eval",
        SolverResultOption
    ),
    entry!(
        Ordinary,
        "rust/fslc/tests/solver_fail_closed.rs",
        193,
        "model_eval",
        SolverResultOption
    ),
    entry!(
        Ordinary,
        "rust/fslc/tests/support/fifo_snapshot.rs",
        370,
        "try_wait",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fslc/tests/typed_agreement/engines.rs",
        187,
        "property_location",
        ResultOption
    ),
    entry!(
        Ordinary,
        "rust/fsl-solver/src/lib.rs",
        287,
        "check",
        UnresolvedAlias
    ),
    entry!(
        Ordinary,
        "rust/fsl-solver/src/lib.rs",
        288,
        "check_assuming",
        UnresolvedAlias
    ),
];

macro_rules! unresolved_ordinary {
    ($path:literal, $line:literal, $function:literal) => {
        entry!(Ordinary, $path, $line, $function, UnresolvedAlias)
    };
}

// These source-declared aliases cannot be reduced to a direct optional-result
// shape. They are explicitly ordinary rather than silently omitted: a return
// whose alias becomes relevant to optional verdicts must be reconsidered here.
const UNRESOLVED_ORDINARY_CLASSIFICATIONS: &[Classification] = &[
    unresolved_ordinary!("rust/fsl-core/src/domain.rs", 417, "build_normalize_scope"),
    unresolved_ordinary!("rust/fsl-core/src/domain_lowering.rs", 2712, "saga_scope"),
    unresolved_ordinary!(
        "rust/fsl-core/src/lib.rs",
        1648,
        "without_indexed_replacement"
    ),
    unresolved_ordinary!("rust/fsl-core/src/typecheck.rs", 58, "base_env"),
    unresolved_ordinary!("rust/fsl-solver-z3/src/lib.rs", 235, "bool_value"),
    unresolved_ordinary!("rust/fsl-solver-z3/src/lib.rs", 239, "int_value"),
    unresolved_ordinary!("rust/fsl-solver-z3js/src/lib.rs", 186, "bool_value"),
    unresolved_ordinary!("rust/fsl-solver-z3js/src/lib.rs", 190, "int_value"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 259, "add"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 247, "and"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 285, "assert"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 286, "assert_and_track"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 269, "const_array"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 244, "constant"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 262, "div"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 250, "equal"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 267, "ge"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 266, "gt"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 249, "implies"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 251, "ite"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 265, "le"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 264, "lt"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 263, "modulo"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 261, "mul"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 258, "neg"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 246, "not"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 248, "or"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 284, "pop"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 270, "select"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 271, "store"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 260, "sub"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 277, "substitute"),
    unresolved_ordinary!("rust/fsl-solver/src/lib.rs", 289, "unsat_core"),
    unresolved_ordinary!("rust/fsl-tools/src/db.rs", 413, "initial_column_states"),
    unresolved_ordinary!("rust/fsl-tools/src/typestate.rs", 636, "and_states"),
    unresolved_ordinary!("rust/fsl-tools/src/typestate.rs", 670, "enum_guard_states"),
    unresolved_ordinary!(
        "rust/fsl-tools/src/typestate.rs",
        815,
        "option_guard_states"
    ),
    unresolved_ordinary!("rust/fsl-tools/src/typestate.rs", 659, "or_guard_states"),
    unresolved_ordinary!(
        "rust/fslc/src/verification.rs",
        2192,
        "execute_cli_verification"
    ),
    unresolved_ordinary!(
        "rust/fslc/src/verification.rs",
        2323,
        "finalize_cli_verification"
    ),
    unresolved_ordinary!("rust/fslc/src/verification.rs", 1809, "run_verify_cli"),
    unresolved_ordinary!(
        "rust/fslc/src/verification.rs",
        1821,
        "run_verify_cli_from_source"
    ),
    unresolved_ordinary!(
        "rust/fslc/tests/issue_868_result_option_verdict_census.rs",
        750,
        "alias_definitions"
    ),
    unresolved_ordinary!(
        "rust/fslc/tests/issue_868_result_option_verdict_census.rs",
        783,
        "merge_alias_definitions"
    ),
    unresolved_ordinary!("rust/fslc/tests/solver_fail_closed.rs", 53, "bool_value"),
    unresolved_ordinary!("rust/fslc/tests/solver_fail_closed.rs", 57, "int_value"),
];

#[derive(Clone, Debug, Eq, PartialEq)]
struct Token {
    text: String,
    line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ConsumerFindingKind {
    DiscardedStatement,
    IndirectReference,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ConsumerFinding {
    path: String,
    line: usize,
    function: String,
    kind: ConsumerFindingKind,
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn rust_sources() -> Vec<PathBuf> {
    fn visit(directory: &Path, paths: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(directory).expect("read source directory") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name != "target") {
                    visit(&path, paths);
                }
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                paths.push(path);
            }
        }
    }

    let mut paths = Vec::new();
    visit(&repository_root().join("rust"), &mut paths);
    paths.sort();
    paths
}

fn raw_string_end(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut cursor = start + 1;
    while bytes.get(cursor) == Some(&b'#') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'"') {
        return None;
    }
    Some((cursor + 1, cursor - start - 1))
}

fn skip_quoted(bytes: &[u8], cursor: &mut usize, line: &mut usize, quote: u8) {
    *cursor += 1;
    while let Some(&byte) = bytes.get(*cursor) {
        *cursor += 1;
        if byte == b'\\' {
            *cursor += 1;
        } else if byte == b'\n' {
            *line += 1;
        } else if byte == quote {
            break;
        }
    }
}

fn tokenize(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut cursor = 0;
    let mut line = 1;
    while let Some(&byte) = bytes.get(cursor) {
        if byte.is_ascii_whitespace() {
            if byte == b'\n' {
                line += 1;
            }
            cursor += 1;
        } else if bytes.get(cursor..cursor + 2) == Some(b"//") {
            cursor += 2;
            while bytes
                .get(cursor)
                .is_some_and(|candidate| *candidate != b'\n')
            {
                cursor += 1;
            }
        } else if bytes.get(cursor..cursor + 2) == Some(b"/*") {
            let mut depth = 1;
            cursor += 2;
            while depth > 0 && cursor < bytes.len() {
                match bytes.get(cursor..cursor + 2) {
                    Some(b"/*") => {
                        depth += 1;
                        cursor += 2;
                    }
                    Some(b"*/") => {
                        depth -= 1;
                        cursor += 2;
                    }
                    _ => {
                        if bytes[cursor] == b'\n' {
                            line += 1;
                        }
                        cursor += 1;
                    }
                }
            }
        } else if byte == b'"' {
            skip_quoted(bytes, &mut cursor, &mut line, byte);
        } else if let Some(raw_start) = (byte == b'r').then_some(cursor).or_else(|| {
            (byte == b'b' && bytes.get(cursor + 1) == Some(&b'r')).then_some(cursor + 1)
        }) && let Some((content, hashes)) = raw_string_end(bytes, raw_start)
        {
            cursor = content;
            loop {
                if bytes.get(cursor) == Some(&b'"')
                    && bytes.get(cursor + 1..cursor + 1 + hashes) == Some(&vec![b'#'; hashes])
                {
                    cursor += hashes + 1;
                    break;
                }
                if bytes.get(cursor) == Some(&b'\n') {
                    line += 1;
                }
                cursor += 1;
            }
        } else if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = cursor;
            cursor += 1;
            while bytes
                .get(cursor)
                .is_some_and(|candidate| candidate.is_ascii_alphanumeric() || *candidate == b'_')
            {
                cursor += 1;
            }
            tokens.push(Token {
                text: source[start..cursor].to_owned(),
                line,
            });
        } else {
            tokens.push(Token {
                text: char::from(byte).to_string(),
                line,
            });
            cursor += 1;
        }
    }
    tokens
}

#[derive(Clone)]
struct AliasDefinition {
    path: String,
    rhs: Vec<Token>,
}

type AliasDefinitions = BTreeMap<String, Vec<AliasDefinition>>;

fn path_final_segment(tokens: &[Token], start: usize) -> Option<(String, usize)> {
    let mut index = start;
    if tokens.get(index).is_some_and(|token| token.text == ":")
        && tokens.get(index + 1).is_some_and(|token| token.text == ":")
    {
        index += 2;
    }
    let mut final_segment = None;
    loop {
        let token = tokens.get(index)?;
        if !token
            .text
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
            && token.text != "_"
        {
            return final_segment.map(|segment| (segment, index));
        }
        final_segment = Some(token.text.clone());
        index += 1;
        if tokens.get(index).is_some_and(|token| token.text == ":")
            && tokens.get(index + 1).is_some_and(|token| token.text == ":")
        {
            index += 2;
        } else {
            return final_segment.map(|segment| (segment, index));
        }
    }
}

fn option_path_end(tokens: &[Token], start: usize) -> Option<usize> {
    let (name, end) = path_final_segment(tokens, start)?;
    (name == "Option" && tokens.get(end).is_some_and(|token| token.text == "<")).then_some(end)
}

fn result_shape(tokens: &[Token]) -> Option<(Shape, usize)> {
    for (index, token) in tokens.iter().enumerate() {
        let alias = token.text == "SolverResult";
        if !alias && token.text != "Result" {
            continue;
        }
        if tokens.get(index + 1).is_none_or(|next| next.text != "<") {
            continue;
        }
        if option_path_end(tokens, index + 2).is_some() {
            return Some((if alias { Shape::Solver } else { Shape::Direct }, index));
        }
        if !alias
            && tokens.get(index + 2).is_some_and(|next| next.text == "(")
            && option_path_end(tokens, index + 3).is_some()
        {
            return Some((Shape::Tuple, index));
        }
    }
    None
}

fn alias_definitions(path: &str, tokens: &[Token]) -> AliasDefinitions {
    let mut aliases = AliasDefinitions::new();
    for (index, _) in tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| token.text == "type")
    {
        let Some(name) = tokens.get(index + 1) else {
            continue;
        };
        let Some(equals) = (index + 2..tokens.len())
            .find(|&candidate| matches!(tokens[candidate].text.as_str(), "=" | ";" | "{"))
        else {
            continue;
        };
        if tokens[equals].text != "=" {
            continue;
        }
        let Some(end) = (equals + 1..tokens.len()).find(|&candidate| tokens[candidate].text == ";")
        else {
            continue;
        };
        aliases
            .entry(name.text.clone())
            .or_default()
            .push(AliasDefinition {
                path: path.to_owned(),
                rhs: tokens[equals + 1..end].to_vec(),
            });
    }
    aliases
}

fn merge_alias_definitions(sources: &[(String, String)]) -> AliasDefinitions {
    let mut merged = AliasDefinitions::new();
    for (path, source) in sources {
        for (name, definitions) in alias_definitions(path, &tokenize(source)) {
            merged.entry(name).or_default().extend(definitions);
        }
    }
    merged
}

fn module_matches(path: &str, module: &str) -> bool {
    Path::new(path)
        .file_stem()
        .is_some_and(|stem| stem == module)
}

fn alias_definition<'a>(
    source_path: &str,
    segments: &[String],
    aliases: &'a AliasDefinitions,
) -> Option<&'a AliasDefinition> {
    let name = segments.last()?;
    let definitions = aliases.get(name)?;
    let module = segments
        .len()
        .checked_sub(2)
        .and_then(|index| segments.get(index));
    let matching: Vec<_> = definitions
        .iter()
        .filter(|definition| {
            definition.path == source_path
                || module.is_some_and(|module| module_matches(&definition.path, module))
        })
        .collect();
    (matching.len() == 1).then(|| matching[0])
}

fn named_path(tokens: &[Token], start: usize) -> Option<(Vec<String>, usize)> {
    let mut index = start;
    let mut segments = Vec::new();
    loop {
        let token = tokens.get(index)?;
        if !token
            .text
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        {
            break;
        }
        segments.push(token.text.clone());
        index += 1;
        if tokens.get(index).is_some_and(|token| token.text == ":")
            && tokens.get(index + 1).is_some_and(|token| token.text == ":")
        {
            index += 2;
        } else {
            break;
        }
    }
    (!segments.is_empty()).then_some((segments, index))
}

fn alias_shape(
    definition: &AliasDefinition,
    aliases: &AliasDefinitions,
    resolving: &mut BTreeSet<(String, String)>,
) -> Shape {
    let name = path_final_segment(&definition.rhs, 0).map(|(name, _)| name);
    let key = (definition.path.clone(), name.clone().unwrap_or_default());
    if !resolving.insert(key.clone()) {
        return Shape::Unresolved;
    }
    let shape = result_shape(&definition.rhs)
        .map(|(shape, _)| shape)
        .or_else(|| {
            let (segments, _) = named_path(&definition.rhs, 0)?;
            alias_definition(&definition.path, &segments, aliases)
                .map(|next| alias_shape(next, aliases, resolving))
        })
        .unwrap_or(Shape::Unresolved);
    resolving.remove(&key);
    shape
}

fn return_shape(
    path: &str,
    tokens: &[Token],
    aliases: &AliasDefinitions,
) -> Option<(Shape, usize)> {
    result_shape(tokens).or_else(|| {
        let (segments, index) = named_path(tokens, 0)?;
        alias_definition(path, &segments, aliases).map(|definition| {
            let mut resolving = BTreeSet::new();
            (alias_shape(definition, aliases, &mut resolving), index)
        })
    })
}

fn signature_after_fn(
    path: &str,
    tokens: &[Token],
    function_index: usize,
    aliases: &AliasDefinitions,
) -> Option<FoundSignature> {
    let function = tokens.get(function_index + 1)?.text.clone();
    let mut parameters = 0;
    let mut arrow = None;
    for index in function_index + 2..tokens.len() {
        match tokens[index].text.as_str() {
            "(" => parameters += 1,
            ")" => parameters -= 1,
            "-" if parameters == 0
                && tokens.get(index + 1).is_some_and(|token| token.text == ">") =>
            {
                arrow = Some(index);
                break;
            }
            "{" | ";" if parameters == 0 => return None,
            _ => {}
        }
    }
    let arrow = arrow?;
    let end = ((arrow + 2)..tokens.len()).find(|&index| {
        tokens
            .get(index)
            .is_some_and(|token| matches!(token.text.as_str(), "{" | ";"))
    })?;
    let (shape, result_index) = return_shape(path, &tokens[arrow + 2..end], aliases)?;
    let result = &tokens[arrow + 2 + result_index];
    Some(FoundSignature {
        path: path.to_owned(),
        line: result.line,
        function,
        shape,
    })
}

fn discover_signatures_with_aliases(
    path: &str,
    source: &str,
    aliases: &AliasDefinitions,
) -> BTreeSet<FoundSignature> {
    let tokens = tokenize(source);
    tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| token.text == "fn")
        .filter_map(|(index, _)| signature_after_fn(path, &tokens, index, aliases))
        .collect()
}

fn discover_signatures(path: &str, source: &str) -> BTreeSet<FoundSignature> {
    let aliases = alias_definitions(path, &tokenize(source));
    discover_signatures_with_aliases(path, source, &aliases)
}

fn repository_relative_identity(relative: &Path) -> String {
    relative.to_string_lossy().replace('\\', "/")
}

fn rust_source_contents() -> Vec<(String, String)> {
    let root = repository_root();
    rust_sources()
        .into_iter()
        .map(|path| {
            let relative = repository_relative_identity(
                path.strip_prefix(&root).expect("source below repository"),
            );
            let source = std::fs::read_to_string(path).expect("read Rust source");
            (relative, source)
        })
        .collect()
}

fn discovered_signatures() -> BTreeSet<FoundSignature> {
    let sources = rust_source_contents();
    let aliases = merge_alias_definitions(&sources);
    sources
        .iter()
        .flat_map(|(path, source)| discover_signatures_with_aliases(path, source, &aliases))
        .collect()
}

fn configured_signatures() -> BTreeMap<SignatureIdentity, Role> {
    let mut configured = BTreeMap::new();
    for classification in CLASSIFICATIONS
        .iter()
        .chain(UNRESOLVED_ORDINARY_CLASSIFICATIONS)
    {
        assert!(
            configured
                .insert(classification.identity(), classification.role)
                .is_none(),
            "duplicate #868 signature classification: {}:{} {}",
            classification.path,
            classification.line,
            classification.function
        );
    }
    configured
}

fn classification_drift(
    discovered: &BTreeSet<SignatureIdentity>,
    configured: &BTreeSet<SignatureIdentity>,
) -> (Vec<SignatureIdentity>, Vec<SignatureIdentity>) {
    let missing = discovered.difference(configured).cloned().collect();
    let stale = configured.difference(discovered).cloned().collect();
    (missing, stale)
}

fn statement_bounds(tokens: &[Token], call: usize) -> Option<(usize, usize)> {
    let start = (0..call)
        .rev()
        .find(|&index| matches!(tokens[index].text.as_str(), ";" | "{" | "}"))
        .map_or(0, |index| index + 1);
    let end =
        (call..tokens.len()).find(|&index| matches!(tokens[index].text.as_str(), ";" | "}"))?;
    (tokens[end].text == ";").then_some((start, end))
}

fn generic_call_open(tokens: &[Token], function: usize) -> Option<usize> {
    if tokens
        .get(function + 1)
        .is_some_and(|token| token.text == "(")
    {
        return Some(function + 1);
    }
    if tokens
        .get(function + 1)
        .is_none_or(|token| token.text != "<")
    {
        return None;
    }
    let mut depth = 0;
    for (index, token) in tokens.iter().enumerate().skip(function + 1) {
        match token.text.as_str() {
            "<" => depth += 1,
            ">" => depth -= 1,
            "(" if depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn consumes_statement(segment: &[Token]) -> bool {
    segment.iter().any(|token| {
        matches!(
            token.text.as_str(),
            "let"
                | "return"
                | "if"
                | "match"
                | "assert"
                | "assert_eq"
                | "assert_ne"
                | "debug_assert"
                | "="
        )
    })
}

fn source_mentions(tokens: &[Token], name: &str) -> bool {
    tokens.iter().any(|token| token.text == name)
}

fn qualified_verification_output_call(tokens: &[Token], function: usize) -> bool {
    let start = function.saturating_sub(6);
    tokens[start..function]
        .iter()
        .any(|token| token.text == "verification_output")
}

fn can_resolve_verdict_call(
    path: &str,
    tokens: &[Token],
    function: usize,
    verdict_definitions: &BTreeMap<String, BTreeSet<String>>,
) -> bool {
    let name = &tokens[function].text;
    if verdict_definitions
        .get(name)
        .is_some_and(|paths| paths.contains(path))
    {
        return true;
    }
    match name.as_str() {
        "observe" => source_mentions(tokens, "BoundedLivenessMonitor"),
        "current_violation" | "current_violation_selected" => source_mentions(tokens, "Monitor"),
        "requirements_implements_output"
        | "governance_output"
        | "governance_output_async"
        | "validate_requirement_trace_source" => {
            qualified_verification_output_call(tokens, function)
        }
        _ => false,
    }
}

fn consumer_findings(
    path: &str,
    source: &str,
    verdict_definitions: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<ConsumerFinding> {
    let tokens = tokenize(source);
    let mut findings = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if !verdict_definitions.contains_key(&token.text)
            || tokens
                .get(index.wrapping_sub(1))
                .is_some_and(|previous| previous.text == "fn")
        {
            continue;
        }
        if !can_resolve_verdict_call(path, &tokens, index, verdict_definitions) {
            continue;
        }
        if generic_call_open(&tokens, index).is_none() {
            findings.push(ConsumerFinding {
                path: path.to_owned(),
                line: token.line,
                function: token.text.clone(),
                kind: ConsumerFindingKind::IndirectReference,
            });
            continue;
        }
        let Some((start, end)) = statement_bounds(&tokens, index) else {
            continue;
        };
        if !consumes_statement(&tokens[start..end]) {
            findings.push(ConsumerFinding {
                path: path.to_owned(),
                line: token.line,
                function: token.text.clone(),
                kind: ConsumerFindingKind::DiscardedStatement,
            });
        }
    }
    findings
}

fn live_consumer_findings() -> Vec<ConsumerFinding> {
    let mut verdict_definitions = BTreeMap::<String, BTreeSet<String>>::new();
    for classification in CLASSIFICATIONS {
        if classification.role == Role::Verdict {
            verdict_definitions
                .entry(classification.function.to_owned())
                .or_default()
                .insert(classification.path.to_owned());
        }
    }
    let root = repository_root();
    rust_sources()
        .into_iter()
        .flat_map(|path| {
            let relative = repository_relative_identity(
                path.strip_prefix(&root).expect("source below repository"),
            );
            let source = std::fs::read_to_string(&path).expect("read Rust source");
            consumer_findings(&relative, &source, &verdict_definitions)
        })
        .collect()
}

#[test]
fn every_result_option_signature_is_classified_and_every_entry_is_live() {
    let discovered = discovered_signatures();
    let discovered = discovered.iter().map(FoundSignature::identity).collect();
    let configured = configured_signatures().into_keys().collect();
    let (missing, stale) = classification_drift(&discovered, &configured);
    assert_eq!(
        (missing, stale),
        (Vec::new(), Vec::new()),
        "#868 signature classification drift: every Result<Option<..>, _> or \
         Result<(Option<..>, _), _> signature needs one classification, and \
         every classification entry must resolve to one signature"
    );
}

#[test]
fn no_verdict_consumer_discards_an_optional_verdict_or_uses_indirection() {
    assert_eq!(
        live_consumer_findings(),
        Vec::new(),
        "#868 verdict consumer drift: a statement-position Result<Option<verdict>, _> \
         call must inspect Some/None; function-pointer and indirect references are forbidden"
    );
}

#[test]
fn synthetic_verdict_discard_is_detected() {
    let source = "fn synthetic_probe() -> Result<Option<Signal>, Error> { Ok(None) }\n\
                  fn consumer() -> Result<(), Error> { synthetic_probe()?; Ok(()) }\n";
    let discovered = discover_signatures("fixture.rs", source)
        .iter()
        .map(FoundSignature::identity)
        .collect();
    let configured = BTreeSet::from([SignatureIdentity {
        path: "fixture.rs".to_owned(),
        function: "synthetic_probe".to_owned(),
        shape: Shape::Direct,
    }]);
    assert_eq!(
        classification_drift(&discovered, &configured),
        (Vec::new(), Vec::new())
    );
    let findings = consumer_findings(
        "fixture.rs",
        source,
        &BTreeMap::from([(
            "synthetic_probe".to_owned(),
            BTreeSet::from(["fixture.rs".to_owned()]),
        )]),
    );
    assert_eq!(
        findings,
        vec![ConsumerFinding {
            path: "fixture.rs".to_owned(),
            line: 2,
            function: "synthetic_probe".to_owned(),
            kind: ConsumerFindingKind::DiscardedStatement,
        }]
    );
}

fn alias_discard_findings(
    path: &str,
    source: &str,
    aliases: &AliasDefinitions,
    function: &str,
) -> Vec<ConsumerFinding> {
    let discovered = discover_signatures_with_aliases(path, source, aliases);
    assert!(
        discovered
            .iter()
            .map(FoundSignature::identity)
            .any(|identity| identity
                == SignatureIdentity {
                    path: path.to_owned(),
                    function: function.to_owned(),
                    shape: Shape::Direct,
                }),
        "alias return must resolve to the direct Result<Option<..>, _> shape: {discovered:?}"
    );
    consumer_findings(
        path,
        source,
        &BTreeMap::from([(function.to_owned(), BTreeSet::from([path.to_owned()]))]),
    )
}

#[test]
fn alias_verdict_discard_is_detected() {
    let source = "type VerdictResult<T> = Result<Option<T>, Error>;\n\
                  fn verdict() -> VerdictResult<Signal> { Ok(None) }\n\
                  fn consumer() -> Result<(), Error> { verdict()?; Ok(()) }\n";
    assert_eq!(
        alias_discard_findings(
            "fixture.rs",
            source,
            &alias_definitions("fixture.rs", &tokenize(source)),
            "verdict",
        ),
        vec![ConsumerFinding {
            path: "fixture.rs".to_owned(),
            line: 3,
            function: "verdict".to_owned(),
            kind: ConsumerFindingKind::DiscardedStatement,
        }]
    );
}

#[test]
fn cross_module_and_nested_alias_verdict_discards_are_detected() {
    let alias_source = "pub type VerdictResult<T> = Result<Option<T>, Error>;\n\
                        pub type NestedResult<T> = VerdictResult<T>;\n";
    let consumer_source = "fn cross_module() -> aliases::VerdictResult<Signal> { Ok(None) }\n\
                           fn nested() -> aliases::NestedResult<Signal> { Ok(None) }\n\
                           fn consumer() -> Result<(), Error> {\n\
                               cross_module()?;\n\
                               nested()?;\n\
                               Ok(())\n\
                           }\n";
    let sources = vec![
        ("aliases.rs".to_owned(), alias_source.to_owned()),
        ("consumer.rs".to_owned(), consumer_source.to_owned()),
    ];
    let aliases = merge_alias_definitions(&sources);
    let discovered = discover_signatures_with_aliases("consumer.rs", consumer_source, &aliases);
    assert_eq!(
        discovered
            .iter()
            .map(FoundSignature::identity)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            SignatureIdentity {
                path: "consumer.rs".to_owned(),
                function: "cross_module".to_owned(),
                shape: Shape::Direct,
            },
            SignatureIdentity {
                path: "consumer.rs".to_owned(),
                function: "nested".to_owned(),
                shape: Shape::Direct,
            },
        ])
    );
    assert_eq!(
        consumer_findings(
            "consumer.rs",
            consumer_source,
            &BTreeMap::from([
                (
                    "cross_module".to_owned(),
                    BTreeSet::from(["consumer.rs".to_owned()]),
                ),
                (
                    "nested".to_owned(),
                    BTreeSet::from(["consumer.rs".to_owned()]),
                ),
            ]),
        ),
        vec![
            ConsumerFinding {
                path: "consumer.rs".to_owned(),
                line: 4,
                function: "cross_module".to_owned(),
                kind: ConsumerFindingKind::DiscardedStatement,
            },
            ConsumerFinding {
                path: "consumer.rs".to_owned(),
                line: 5,
                function: "nested".to_owned(),
                kind: ConsumerFindingKind::DiscardedStatement,
            },
        ]
    );
}

#[test]
fn qualified_option_verdict_discard_is_detected() {
    let source = "fn std_verdict() -> Result<std::option::Option<Signal>, Error> { Ok(None) }\n\
                  fn core_verdict() -> Result<core::option::Option<Signal>, Error> { Ok(None) }\n\
                  fn consumer() -> Result<(), Error> {\n\
                      std_verdict()?;\n\
                      core_verdict()?;\n\
                      Ok(())\n\
                  }\n";
    let aliases = alias_definitions("fixture.rs", &tokenize(source));
    let discovered = discover_signatures_with_aliases("fixture.rs", source, &aliases);
    assert_eq!(
        discovered
            .iter()
            .map(FoundSignature::identity)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            SignatureIdentity {
                path: "fixture.rs".to_owned(),
                function: "core_verdict".to_owned(),
                shape: Shape::Direct,
            },
            SignatureIdentity {
                path: "fixture.rs".to_owned(),
                function: "std_verdict".to_owned(),
                shape: Shape::Direct,
            },
        ])
    );
    assert_eq!(
        consumer_findings(
            "fixture.rs",
            source,
            &BTreeMap::from([
                (
                    "core_verdict".to_owned(),
                    BTreeSet::from(["fixture.rs".to_owned()]),
                ),
                (
                    "std_verdict".to_owned(),
                    BTreeSet::from(["fixture.rs".to_owned()]),
                ),
            ]),
        ),
        vec![
            ConsumerFinding {
                path: "fixture.rs".to_owned(),
                line: 4,
                function: "std_verdict".to_owned(),
                kind: ConsumerFindingKind::DiscardedStatement,
            },
            ConsumerFinding {
                path: "fixture.rs".to_owned(),
                line: 5,
                function: "core_verdict".to_owned(),
                kind: ConsumerFindingKind::DiscardedStatement,
            },
        ]
    );
}

#[test]
fn unresolvable_alias_return_requires_a_classification() {
    let source = "type Unresolved<T> = Result<external::Missing<T>, Error>;\n\
                  fn unresolved() -> Unresolved<Signal> { unreachable!() }\n";
    let discovered = discover_signatures("fixture.rs", source)
        .iter()
        .map(FoundSignature::identity)
        .collect();
    let missing = SignatureIdentity {
        path: "fixture.rs".to_owned(),
        function: "unresolved".to_owned(),
        shape: Shape::Unresolved,
    };
    assert_eq!(
        classification_drift(&discovered, &BTreeSet::new()),
        (vec![missing], Vec::new())
    );
}

#[test]
fn stale_classification_entry_is_detected() {
    let source = "fn existing() -> Result<Option<Signal>, Error> { Ok(None) }\n";
    let discovered = discover_signatures("fixture.rs", source)
        .iter()
        .map(FoundSignature::identity)
        .collect();
    let existing = SignatureIdentity {
        path: "fixture.rs".to_owned(),
        function: "existing".to_owned(),
        shape: Shape::Direct,
    };
    let stale = SignatureIdentity {
        path: "fixture.rs".to_owned(),
        function: "removed".to_owned(),
        shape: Shape::Direct,
    };
    let configured = BTreeSet::from([existing, stale.clone()]);
    assert_eq!(
        classification_drift(&discovered, &configured),
        (Vec::new(), vec![stale])
    );
}

#[test]
fn repository_relative_identity_normalizes_windows_and_preserves_forward_slashes() {
    assert_eq!(
        repository_relative_identity(Path::new(r"rust\fsl-runtime\src\lib.rs")),
        "rust/fsl-runtime/src/lib.rs"
    );
    assert_eq!(
        repository_relative_identity(Path::new("rust/fsl-runtime/src/lib.rs")),
        "rust/fsl-runtime/src/lib.rs"
    );
}

#[test]
fn unnormalized_windows_path_identity_reports_missing_and_stale_classifications() {
    let unnormalized = SignatureIdentity {
        path: r"rust\fsl-runtime\src\lib.rs".to_owned(),
        function: "probe".to_owned(),
        shape: Shape::Direct,
    };
    let normalized = SignatureIdentity {
        path: "rust/fsl-runtime/src/lib.rs".to_owned(),
        function: "probe".to_owned(),
        shape: Shape::Direct,
    };
    assert_eq!(
        classification_drift(
            &BTreeSet::from([unnormalized.clone()]),
            &BTreeSet::from([normalized.clone()]),
        ),
        (vec![unnormalized], vec![normalized])
    );
}

#[test]
fn normalized_windows_path_identity_resolves_classification_drift() {
    let normalized_path = repository_relative_identity(Path::new(r"rust\fsl-runtime\src\lib.rs"));
    let discovered = BTreeSet::from([SignatureIdentity {
        path: normalized_path,
        function: "probe".to_owned(),
        shape: Shape::Direct,
    }]);
    let configured = BTreeSet::from([SignatureIdentity {
        path: "rust/fsl-runtime/src/lib.rs".to_owned(),
        function: "probe".to_owned(),
        shape: Shape::Direct,
    }]);
    assert_eq!(
        classification_drift(&discovered, &configured),
        (Vec::new(), Vec::new())
    );
}

#[test]
fn normalized_windows_consumer_path_detects_a_discarded_verdict() {
    let path = repository_relative_identity(Path::new(r"rust\fsl-runtime\src\lib.rs"));
    let source = "fn synthetic_probe() -> Result<Option<Signal>, Error> { Ok(None) }\n\
                  fn consumer() -> Result<(), Error> { synthetic_probe()?; Ok(()) }\n";
    let verdict_definitions = BTreeMap::from([(
        "synthetic_probe".to_owned(),
        BTreeSet::from(["rust/fsl-runtime/src/lib.rs".to_owned()]),
    )]);
    assert_eq!(
        consumer_findings(&path, source, &verdict_definitions),
        vec![ConsumerFinding {
            path: "rust/fsl-runtime/src/lib.rs".to_owned(),
            line: 2,
            function: "synthetic_probe".to_owned(),
            kind: ConsumerFindingKind::DiscardedStatement,
        }]
    );
}
