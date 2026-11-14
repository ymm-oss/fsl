// SPDX-License-Identifier: Apache-2.0

//! Every test that spawns the native `fslc` binary isolates its verify cache
//! (issue #1872).
//!
//! The verify cache is on by default and otherwise lives in the developer's
//! (or CI runner's) cache directory, so a child that inherits the
//! environment can see what an earlier run left there. A Rust source file
//! under `rust/fslc/tests` that names the binary variable must either use
//! `support/fslc_cli.rs` or set up its own cache (`FSLC_CACHE` to `off`, or
//! its own `FSLC_CACHE_DIR`). A literal `Command::new(env!(..))` of the
//! binary is accepted only with its own cache setup, so a file that already
//! uses the helper cannot keep a bare spawn beside it. There is no allowlist.
//!
//! Matching runs on tokens, so a comment or doc string that names the
//! binary is neither a spawn nor a cache setup.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use proc_macro2::{Delimiter, TokenStream, TokenTree};

// Spelled in two parts so this file does not name the binary variable itself.
const BINARY_VARIABLE: &str = concat!("CARGO_BIN_EXE_", "fslc");

/// The strongest cache setup seen in a file; a later, weaker one does not
/// lower it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Setup {
    #[default]
    None,
    Helper,
    OwnCache,
}

#[derive(Debug, Default)]
struct Findings {
    names_binary: bool,
    bare_spawn: bool,
    setup: Setup,
}

impl Findings {
    fn raise(&mut self, setup: Setup) {
        self.setup = self.setup.max(setup);
    }

    fn violation(&self) -> Option<&'static str> {
        match self.setup {
            Setup::OwnCache => None,
            _ if self.bare_spawn => {
                Some("bare Command::new(env!(..)) of fslc without its own cache setup")
            }
            Setup::None if self.names_binary => {
                Some("names the fslc binary without support/fslc_cli.rs or its own cache setup")
            }
            _ => None,
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

fn scan(stream: TokenStream, findings: &mut Findings) {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    for (index, token) in tokens.iter().enumerate() {
        let next = |offset: usize| tokens.get(index + offset);
        match token {
            TokenTree::Group(group) => scan(group.stream(), findings),
            TokenTree::Literal(_) => match string_value(token).as_deref() {
                Some(name) if name == BINARY_VARIABLE => findings.names_binary = true,
                Some("FSLC_CACHE_DIR") if is_punct(next(1), ',') => {
                    findings.raise(Setup::OwnCache);
                }
                Some("FSLC_CACHE")
                    if is_punct(next(1), ',')
                        && next(2).and_then(string_value).as_deref() == Some("off") =>
                {
                    findings.raise(Setup::OwnCache);
                }
                _ => {}
            },
            TokenTree::Ident(ident) if ident == "fslc_cli" && is_punct(next(1), ':') => {
                findings.raise(Setup::Helper);
            }
            TokenTree::Ident(ident)
                if ident == "Command"
                    && is_punct(next(1), ':')
                    && is_punct(next(2), ':')
                    && is_ident(next(3), "new")
                    && is_binary_argument(next(4)) =>
            {
                findings.bare_spawn = true;
            }
            _ => {}
        }
    }
}

fn findings(source: &str) -> Findings {
    let stream = TokenStream::from_str(source).expect("tokenize test source");
    let mut findings = Findings::default();
    scan(stream, &mut findings);
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
        if found.names_binary || found.setup == Setup::Helper {
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
    ] {
        let found = findings(source);
        assert_eq!(
            found.violation().is_some(),
            rejected,
            "{label}: produced {found:?}"
        );
    }
}
