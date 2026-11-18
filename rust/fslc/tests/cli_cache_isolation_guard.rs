// SPDX-License-Identifier: Apache-2.0

//! Every test that spawns the native `fslc` binary isolates its verify cache
//! (issue #1872).
//!
//! The verify cache is on by default and otherwise lives in the developer's
//! (or CI runner's) cache directory, so a child that inherits the
//! environment can see what an earlier run left there. Two rules hold for
//! every Rust source file under `rust/fslc/tests`, with no allowlist:
//!
//! - A literal `Command::new(env!(..))` of the binary is accepted only when
//!   the function body that holds it sets up its own cache (`FSLC_CACHE` to
//!   `off`, or its own `FSLC_CACHE_DIR`). A setup in another function of the
//!   same file does not cover it.
//! - A file that names the binary variable in any other way (a path handed
//!   to a script or embedded in a manifest) uses `support/fslc_cli.rs` or
//!   sets up its own cache somewhere in the file.
//!
//! Matching runs on tokens, so a comment or doc string that names the
//! binary is neither a spawn nor a cache setup.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use proc_macro2::{Delimiter, TokenStream, TokenTree};

// Spelled in two parts so this file does not name the binary variable itself.
const BINARY_VARIABLE: &str = concat!("CARGO_BIN_EXE_", "fslc");

#[derive(Debug, Default)]
struct Findings {
    names_binary: bool,
    uses_helper: bool,
    own_cache_in_file: bool,
    uncovered_spawn_lines: Vec<usize>,
}

impl Findings {
    fn violation(&self) -> Option<String> {
        if !self.uncovered_spawn_lines.is_empty() {
            Some(format!(
                "bare Command::new(env!(..)) of fslc in a function without its own cache setup \
                 at line(s) {:?}",
                self.uncovered_spawn_lines
            ))
        } else if self.names_binary && !self.uses_helper && !self.own_cache_in_file {
            Some("names the fslc binary without support/fslc_cli.rs or its own cache setup".into())
        } else {
            None
        }
    }
}

/// What a token subtree holds that a function body above it resolves.
#[derive(Default)]
struct Pending {
    own_cache: bool,
    spawn_lines: Vec<usize>,
}

impl Pending {
    fn absorb(&mut self, child: Self) {
        self.own_cache |= child.own_cache;
        self.spawn_lines.extend(child.spawn_lines);
    }

    fn resolve(self, findings: &mut Findings) {
        if !self.own_cache {
            findings.uncovered_spawn_lines.extend(self.spawn_lines);
        }
    }
}

fn string_value(token: &TokenTree) -> Option<String> {
    let TokenTree::Literal(literal) = token else {
        return None;
    };
    match syn::Lit::new(literal.clone()) {
        syn::Lit::Str(string) => Some(string.value()),
        _ => None,
    }
}

fn is_punct(token: Option<&TokenTree>, character: char) -> bool {
    matches!(token, Some(TokenTree::Punct(punct)) if punct.as_char() == character)
}

fn is_ident(token: Option<&TokenTree>, name: &str) -> bool {
    matches!(token, Some(TokenTree::Ident(ident)) if ident == name)
}

/// `( env ! ( "<binary variable>" ) )`
fn is_binary_argument(token: Option<&TokenTree>) -> bool {
    let Some(TokenTree::Group(group)) = token else {
        return false;
    };
    if group.delimiter() != Delimiter::Parenthesis {
        return false;
    }
    let inner: Vec<TokenTree> = group.stream().into_iter().collect();
    let [env, bang, TokenTree::Group(argument)] = inner.as_slice() else {
        return false;
    };
    let argument: Vec<TokenTree> = argument.stream().into_iter().collect();
    is_ident(Some(env), "env")
        && is_punct(Some(bang), '!')
        && matches!(argument.as_slice(), [name] if string_value(name).as_deref() == Some(BINARY_VARIABLE))
}

/// Scan one token list. A brace group that follows an `fn` signature is a
/// function body: its spawns are resolved against its own cache setup and
/// do not reach the caller. Every other group passes its spawns up.
fn scan(stream: TokenStream, findings: &mut Findings) -> Pending {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    let mut pending = Pending::default();
    let mut in_signature = false;
    for (index, token) in tokens.iter().enumerate() {
        let next = |offset: usize| tokens.get(index + offset);
        match token {
            TokenTree::Group(group) => {
                let child = scan(group.stream(), findings);
                if group.delimiter() == Delimiter::Brace && in_signature {
                    pending.own_cache |= child.own_cache;
                    child.resolve(findings);
                } else {
                    pending.absorb(child);
                }
                if group.delimiter() == Delimiter::Brace {
                    in_signature = false;
                }
            }
            TokenTree::Ident(ident) if ident == "fn" => in_signature = true,
            TokenTree::Punct(punct) if punct.as_char() == ';' => in_signature = false,
            TokenTree::Literal(_) => match string_value(token).as_deref() {
                Some(name) if name == BINARY_VARIABLE => findings.names_binary = true,
                Some("FSLC_CACHE_DIR") if is_punct(next(1), ',') => pending.own_cache = true,
                Some("FSLC_CACHE")
                    if is_punct(next(1), ',')
                        && next(2).and_then(string_value).as_deref() == Some("off") =>
                {
                    pending.own_cache = true;
                }
                _ => {}
            },
            TokenTree::Ident(ident) if ident == "fslc_cli" && is_punct(next(1), ':') => {
                findings.uses_helper = true;
            }
            TokenTree::Ident(ident)
                if ident == "Command"
                    && is_punct(next(1), ':')
                    && is_punct(next(2), ':')
                    && is_ident(next(3), "new")
                    && is_binary_argument(next(4)) =>
            {
                pending.spawn_lines.push(ident.span().start().line);
            }
            _ => {}
        }
    }
    pending
}

fn findings(source: &str) -> Findings {
    let stream = TokenStream::from_str(source).expect("tokenize test source");
    let mut findings = Findings::default();
    let file = scan(stream, &mut findings);
    findings.own_cache_in_file |= file.own_cache;
    file.resolve(&mut findings);
    findings
}

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read tests directory") {
        let path = entry.expect("read tests entry").path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn every_fslc_spawn_isolates_the_verify_cache() {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut files = Vec::new();
    collect_rust_files(&tests, &mut files);
    files.sort();

    let mut spawning = 0;
    let mut violations = Vec::new();
    for path in &files {
        let source = std::fs::read_to_string(path).expect("read test source");
        let found = findings(&source);
        if found.names_binary || found.uses_helper {
            spawning += 1;
        }
        if let Some(reason) = found.violation() {
            let relative = path.strip_prefix(&tests).expect("under tests/");
            violations.push(format!("{}: {reason}", relative.display()));
        }
    }

    assert!(
        spawning >= 300,
        "only {spawning} of {} files under {} spawn fslc; the scan is looking in the wrong place",
        files.len(),
        tests.display()
    );
    assert!(
        violations.is_empty(),
        "spawn fslc through support/fslc_cli.rs (or set FSLC_CACHE=off / a private \
         FSLC_CACHE_DIR):\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_classifier_rejects_bare_spawns_and_accepts_isolated_ones() {
    let bare = r#"fn run() { Command::new(env!("CARGO_BIN_EXE_fslc")).output(); }"#;
    let qualified = r#"fn run() { std::process::Command::new(env!("CARGO_BIN_EXE_fslc")); }"#;
    let path_only = r#"fn run() { let binary = env!("CARGO_BIN_EXE_fslc"); }"#;
    let helper = r"fn run() { fslc_cli::command().output(); }";
    let helper_with_bare = r#"fn run() {
        fslc_cli::command().output();
        Command::new(env!("CARGO_BIN_EXE_fslc")).output();
    }"#;
    let helper_isolating_a_path = r#"fn run() {
        let binary = env!("CARGO_BIN_EXE_fslc");
        fslc_cli::isolate(&mut Command::new(binary)).output();
    }"#;
    let off = r#"fn run() { Command::new(env!("CARGO_BIN_EXE_fslc")).env("FSLC_CACHE", "off"); }"#;
    let own_dir =
        r#"fn run() { Command::new(env!("CARGO_BIN_EXE_fslc")).env("FSLC_CACHE_DIR", dir); }"#;
    let removal_only = r#"fn run() {
        Command::new(env!("CARGO_BIN_EXE_fslc")).env_remove("FSLC_CACHE").env_remove("FSLC_CACHE_DIR");
    }"#;
    let commented = r#"
        // Command::new(env!("CARGO_BIN_EXE_fslc")).env("FSLC_CACHE", "off")
        /// fslc_cli::command()
        fn run() { Command::new(env!("CARGO_BIN_EXE_fslc")).output(); }
    "#;
    let unrelated = r#"fn run() { Command::new("git").env("FSLC_CACHE", "on"); }"#;
    let setup_in_another_function = r#"
        fn run() { Command::new(env!("CARGO_BIN_EXE_fslc")).output(); }
        fn run_cached(dir: &Path) { Command::new(env!("CARGO_BIN_EXE_fslc")).env("FSLC_CACHE_DIR", dir); }
    "#;
    let setup_in_the_same_function = r#"fn run() {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
        command.env("FSLC_CACHE", "off");
    }"#;
    let closure_in_a_function = r#"fn runs() {
        let run = |args| Command::new(env!("CARGO_BIN_EXE_fslc")).args(args).env("FSLC_CACHE", "off");
    }"#;
    let method_without_setup = r#"impl Fixture {
        fn cached(&self) { Command::new(env!("CARGO_BIN_EXE_fslc")).env("FSLC_CACHE_DIR", &self.0); }
        fn run(&self) { Command::new(env!("CARGO_BIN_EXE_fslc")).output(); }
    }"#;
    let path_with_setup_elsewhere = r#"
        fn binary() -> &'static str { env!("CARGO_BIN_EXE_fslc") }
        fn run() { Command::new(binary()).env("FSLC_CACHE", "off"); }
    "#;

    for (label, source, rejected) in [
        ("bare", bare, true),
        ("qualified", qualified, true),
        ("path_only", path_only, true),
        ("helper", helper, false),
        ("helper_with_bare", helper_with_bare, true),
        ("helper_isolating_a_path", helper_isolating_a_path, false),
        ("off", off, false),
        ("own_dir", own_dir, false),
        ("removal_only", removal_only, true),
        ("commented", commented, true),
        ("unrelated", unrelated, false),
        ("setup_in_another_function", setup_in_another_function, true),
        (
            "setup_in_the_same_function",
            setup_in_the_same_function,
            false,
        ),
        ("closure_in_a_function", closure_in_a_function, false),
        ("method_without_setup", method_without_setup, true),
        (
            "path_with_setup_elsewhere",
            path_with_setup_elsewhere,
            false,
        ),
    ] {
        let found = findings(source);
        assert_eq!(
            found.violation().is_some(),
            rejected,
            "{label}: produced {found:?}"
        );
    }
}
