// SPDX-License-Identifier: Apache-2.0

//! #1546: every model consumer rejects an empty enum at its declaration,
//! including unused declarations and tool-less AI components.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
}

struct Fixture {
    path: PathBuf,
    generated: PathBuf,
}

impl Fixture {
    fn new(source: &str) -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let scratch = root().join(format!(
            ".codex/tmp/issue-1546-{}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir_all(&scratch).expect("create fixture directory");
        let path = scratch.join("spec.fsl");
        std::fs::write(&path, source).expect("write fixture");
        Self {
            path,
            generated: scratch.join("generated.test.ts"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Consumer {
    Check,
    Bmc,
    Induction,
    Explicit,
    Scenarios,
    Testgen,
    AiCheck,
    DbCheck,
}

fn run(fixture: &Fixture, consumer: Consumer) -> (Value, i32) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    match consumer {
        Consumer::Check => command.arg("check"),
        Consumer::AiCheck => command.args(["ai", "check"]),
        Consumer::DbCheck => command.args(["db", "check"]),
        Consumer::Scenarios => command.arg("scenarios"),
        Consumer::Testgen => command.arg("testgen"),
        Consumer::Bmc | Consumer::Induction | Consumer::Explicit => command.arg("verify"),
    };
    command.arg(&fixture.path);
    if !matches!(consumer, Consumer::Check) {
        command.args(["--depth", "2", "--deadlock", "ignore"]);
    }
    match consumer {
        Consumer::Bmc => command.args(["--engine", "bmc"]),
        Consumer::Induction => command.args(["--engine", "induction"]),
        Consumer::Explicit => command.args(["--engine", "explicit"]),
        Consumer::Testgen => command
            .args(["--target", "vitest", "--allow-unwired", "-o"])
            .arg(&fixture.generated),
        _ => &mut command,
    };
    let output = command
        .env_remove("FSLC_CACHE")
        .current_dir(root())
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{consumer:?}: invalid JSON: {error}; stdout={}; stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn assert_type_error(source: &str, consumer: Consumer, message: &str, line: u64, column: u64) {
    let fixture = Fixture::new(source);
    let (value, status) = run(&fixture, consumer);
    // Print every produced envelope before asserting so fail-first evidence
    // preserves a wrong message/location even when the exit already differs.
    println!("{consumer:?}: produced exit={status}, output={value}");
    assert_eq!(status, 2, "{consumer:?}: {value}");
    assert_eq!(value["result"], "error", "{value}");
    assert_eq!(value["kind"], "type", "{value}");
    assert_eq!(value["message"], message, "{value}");
    assert_eq!(
        value["loc"],
        json!({"line": line, "column": column}),
        "{value}"
    );
    assert!(
        !fixture.generated.exists(),
        "rejection must not write a harness"
    );
}

fn spec(declarations: &str, state: &str, init: &str, action: &str) -> String {
    format!(
        "spec EmptyEnum {{\n{declarations}\n  state {{ n: 0..1 {state} }}\n  init {{ n = 0 {init} }}\n  action tick({action}) {{ n = n }}\n  invariant I {{ n <= 1 }}\n}}\n"
    )
}

macro_rules! empty_shape {
    ($module:ident, $declarations:expr, $state:expr, $init:expr, $action:expr, $name:expr, $line:expr) => {
        mod $module {
            use super::*;

            fn rejects(consumer: Consumer) {
                let source = spec($declarations, $state, $init, $action);
                assert_type_error(
                    &source,
                    consumer,
                    &format!("enum '{}' has no members", $name),
                    $line,
                    3,
                );
            }

            #[test]
            fn check() {
                rejects(Consumer::Check);
            }
            #[test]
            fn bmc() {
                rejects(Consumer::Bmc);
            }
            #[test]
            fn induction() {
                rejects(Consumer::Induction);
            }
            #[test]
            fn explicit() {
                rejects(Consumer::Explicit);
            }
            #[test]
            fn scenarios() {
                rejects(Consumer::Scenarios);
            }
            #[test]
            fn testgen() {
                rejects(Consumer::Testgen);
            }
        }
    };
}

empty_shape!(state_type, "  enum E { }", ", e: E", "", "", "E", 2);
empty_shape!(map_key, "  enum E { }", ", m: Map<E, Bool>", "", "", "E", 2);
empty_shape!(
    map_value,
    "  enum E { }",
    ", m: Map<Bool, E>",
    "",
    "",
    "E",
    2
);
empty_shape!(action_argument, "  enum E { }", "", "", "e: E", "E", 2);
empty_shape!(unused, "  enum E { }", "", "", "", "E", 2);
empty_shape!(
    seq_element,
    "  enum E { }",
    ", xs: Seq<E, 1>",
    "xs = Seq {}",
    "",
    "E",
    2
);
empty_shape!(
    struct_field,
    "  enum E { }\n  struct Holder { item: E }",
    "",
    "",
    "",
    "E",
    2
);
// Detector for a guard incorrectly checking only the first enum declaration.
empty_shape!(
    second_enum,
    "  enum First { Only }\n  enum Second { }",
    "",
    "",
    "",
    "Second",
    3
);

fn singleton(consumer: Consumer) {
    let source = spec("  enum E { Only }", ", e: E", "e = Only", "");
    let fixture = Fixture::new(&source);
    let (value, status) = run(&fixture, consumer);
    assert_eq!(status, 0, "{consumer:?}: {value}");
    let expected = match consumer {
        Consumer::Check => "ok",
        Consumer::Induction | Consumer::Explicit => "proved",
        Consumer::Scenarios => "scenarios",
        Consumer::Testgen => "generated",
        Consumer::Bmc => "verified",
        Consumer::AiCheck | Consumer::DbCheck => unreachable!("not a dialect fixture"),
    };
    assert_eq!(value["result"], expected, "{value}");
    if matches!(consumer, Consumer::Testgen) {
        assert!(
            fixture.generated.is_file(),
            "successful testgen writes its harness"
        );
    }
}

fn duplicate(consumer: Consumer) {
    let source = spec("  enum E { Only, Only }", "", "", "");
    let (value, status) = run(&Fixture::new(&source), consumer);
    assert_eq!(status, 2, "{consumer:?}: {value}");
    assert_eq!(value["result"], "error", "{value}");
    assert_eq!(value["kind"], "name", "{value}");
    assert_eq!(value["message"], "duplicate enum member 'Only'", "{value}");
    assert_eq!(value["loc"], json!({"line": 2, "column": 18}), "{value}");
}

macro_rules! controls {
    ($module:ident, $assertion:ident) => {
        mod $module {
            use super::*;
            #[test]
            fn check() {
                $assertion(Consumer::Check);
            }
            #[test]
            fn bmc() {
                $assertion(Consumer::Bmc);
            }
            #[test]
            fn induction() {
                $assertion(Consumer::Induction);
            }
            #[test]
            fn explicit() {
                $assertion(Consumer::Explicit);
            }
            #[test]
            fn scenarios() {
                $assertion(Consumer::Scenarios);
            }
            #[test]
            fn testgen() {
                $assertion(Consumer::Testgen);
            }
        }
    };
}

controls!(singleton_control, singleton);
controls!(duplicate_control, duplicate);

macro_rules! ai_shape {
    ($module:ident, $source:expr, $message:expr) => {
        mod $module {
            use super::*;
            #[test]
            fn check() {
                assert_type_error($source, Consumer::Check, $message, 2, 3);
            }
            #[test]
            fn ai_check() {
                assert_type_error($source, Consumer::AiCheck, $message, 2, 3);
            }
            #[test]
            fn bmc() {
                assert_type_error($source, Consumer::Bmc, $message, 2, 3);
            }
            #[test]
            fn induction() {
                assert_type_error($source, Consumer::Induction, $message, 2, 3);
            }
        }
    };
}

// Leading whitespace prevents a generic unlocated 1:1 fallback from satisfying
// the AI declaration-location contract. Name the authored component, not Tool.
ai_shape!(
    ai_no_tools,
    "\n  ai_component NoTools { }\n",
    "ai_component 'NoTools' declares no tools"
);
ai_shape!(
    ai_empty_authority,
    "\n  ai_component EmptyAuthority { authority { } }\n",
    "ai_component 'EmptyAuthority' declares no tools"
);

fn ai_one_tool(consumer: Consumer) {
    let fixture = Fixture::new(
        "\n  ai_component OneTool { tool Search { schema SearchV1; } authority { } }\n",
    );
    let (value, status) = run(&fixture, consumer);
    assert_eq!(status, 0, "{consumer:?}: {value}");
    let expected = match consumer {
        Consumer::Check => "ok",
        Consumer::AiCheck => "verified_under_assumptions",
        Consumer::Bmc => "verified",
        Consumer::Induction => "proved",
        _ => unreachable!("only the four approved AI consumers"),
    };
    assert_eq!(value["result"], expected, "{value}");
}

mod ai_one_tool_control {
    use super::*;
    #[test]
    fn check() {
        ai_one_tool(Consumer::Check);
    }
    #[test]
    fn ai_check() {
        ai_one_tool(Consumer::AiCheck);
    }
    #[test]
    fn bmc() {
        ai_one_tool(Consumer::Bmc);
    }
    #[test]
    fn induction() {
        ai_one_tool(Consumer::Induction);
    }
}

// Review follow-up: generated Column must diagnose the authored database,
// whether there are no tables at all or only a table without columns.
fn database(table: &str) -> String {
    format!(
        "dbsystem Columns {{\n  database app {{\n    schema 0\n{table}\n  }}\n  environment prod {{ schema 0..0; }}\n  check compatibility {{ rule all_active_reads_exist; }}\n}}\n"
    )
}

fn empty_database(table: &str, consumer: Consumer) {
    assert_type_error(
        &database(table),
        consumer,
        "database 'app' declares no columns",
        2,
        3,
    );
}

fn one_column_database(consumer: Consumer) {
    let source = database("    table users { column id: Int present backfilled not_null; }");
    let (value, status) = run(&Fixture::new(&source), consumer);
    println!("{consumer:?}: produced exit={status}, output={value}");
    assert_eq!(status, 0, "{consumer:?}: {value}");
    let expected = match consumer {
        Consumer::Check => "ok",
        Consumer::Bmc => "verified",
        Consumer::Induction => "proved",
        Consumer::DbCheck => "verified_under_assumptions",
        _ => unreachable!("only database consumers"),
    };
    assert_eq!(value["result"], expected, "{value}");
}

macro_rules! database_consumers {
    ($module:ident, $assertion:expr) => {
        mod $module {
            use super::*;
            #[test]
            fn check() {
                $assertion(Consumer::Check);
            }
            #[test]
            fn bmc() {
                $assertion(Consumer::Bmc);
            }
            #[test]
            fn induction() {
                $assertion(Consumer::Induction);
            }
            #[test]
            fn db_check() {
                $assertion(Consumer::DbCheck);
            }
        }
    };
}

database_consumers!(database_no_tables, |consumer| empty_database("", consumer));
database_consumers!(database_empty_table, |consumer| empty_database(
    "    table users { }",
    consumer
));
database_consumers!(database_one_column_control, one_column_database);

// symmetric is the declaration's first token, not the later enum keyword.
empty_shape!(symmetric, "  symmetric enum E { }", "", "", "", "E", 2);

fn symmetric_singleton(consumer: Consumer) {
    let source = spec("  symmetric enum E { Only }", ", e: E", "e = Only", "");
    let fixture = Fixture::new(&source);
    let (value, status) = run(&fixture, consumer);
    println!("{consumer:?}: produced exit={status}, output={value}");
    assert_eq!(status, 0, "{consumer:?}: {value}");
    let expected = match consumer {
        Consumer::Check => "ok",
        Consumer::Bmc => "verified",
        Consumer::Induction | Consumer::Explicit => "proved",
        Consumer::Scenarios => "scenarios",
        Consumer::Testgen => "generated",
        Consumer::AiCheck | Consumer::DbCheck => unreachable!("not a dialect fixture"),
    };
    assert_eq!(value["result"], expected, "{value}");
    if matches!(consumer, Consumer::Testgen) {
        assert!(
            fixture.generated.is_file(),
            "testgen must write its harness"
        );
    }
}

controls!(symmetric_singleton_control, symmetric_singleton);

const COMPOSE_PARENT: &str = "compose Parent {\n  use Child as child from \"child.fsl\"\n}\n";

fn compose_fixture(members: &str) -> (Fixture, PathBuf) {
    let fixture = Fixture::new(COMPOSE_PARENT);
    let child = fixture.path.with_file_name("child.fsl");
    // Six lines before/at the enum, versus just three in the parent. The enum
    // also has a different indent, so neither coordinate can stand in for use.
    std::fs::write(
        &child,
        format!(
            "spec Child {{\n  // Child coordinates must not become parent loc.\n\n\n\n      enum E {{ {members} }}\n  state {{ ready: Bool }}\n  init {{ ready = false }}\n  action tick() {{ ready = ready }}\n}}\n"
        ),
    )
    .expect("write component fixture");
    (fixture, child)
}

fn compose_empty_child(consumer: Consumer) {
    let (fixture, child) = compose_fixture("");
    let (value, status) = run(&fixture, consumer);
    println!("{consumer:?}: produced exit={status}, output={value}");
    assert_eq!(status, 2, "{consumer:?}: {value}");
    assert_eq!(value["result"], "error", "{value}");
    assert_eq!(value["kind"], "type", "{value}");
    assert_eq!(
        value["message"],
        format!(
            "component \"{}\" failed to check (enum 'E' has no members at {}:6:7) at {}:2:3",
            child.display(),
            child.display(),
            fixture.path.display()
        ),
        "{value}"
    );
    assert_eq!(value["loc"], json!({"line": 2, "column": 3}), "{value}");
    let line =
        usize::try_from(value["loc"]["line"].as_u64().expect("loc.line")).expect("line fits usize");
    let column = usize::try_from(value["loc"]["column"].as_u64().expect("loc.column"))
        .expect("column fits usize");
    let text = COMPOSE_PARENT
        .lines()
        .nth(line - 1)
        .expect("loc line exists in parent")
        .chars()
        .skip(column - 1)
        .collect::<String>();
    assert!(text.starts_with("use "), "parent loc points at {text:?}");
}

fn compose_nonempty_child(consumer: Consumer) {
    let (fixture, _) = compose_fixture("Only");
    let (value, status) = run(&fixture, consumer);
    println!("{consumer:?}: produced exit={status}, output={value}");
    assert_eq!(status, 0, "{consumer:?}: {value}");
    let expected = match consumer {
        Consumer::Check => "ok",
        Consumer::Bmc => "verified",
        _ => unreachable!("only compose check/BMC"),
    };
    assert_eq!(value["result"], expected, "{value}");
}

macro_rules! compose_consumers {
    ($module:ident, $assertion:ident) => {
        mod $module {
            use super::*;
            #[test]
            fn check() {
                $assertion(Consumer::Check);
            }
            #[test]
            fn bmc() {
                $assertion(Consumer::Bmc);
            }
        }
    };
}

compose_consumers!(compose_child, compose_empty_child);
compose_consumers!(compose_child_nonempty_control, compose_nonempty_child);

fn ai_error_precedence(consumer: Consumer, authority: bool) {
    let clause = if authority {
        "authority { may_execute Foo; }"
    } else {
        "check hard { rule bogus; }"
    };
    let fixture = Fixture::new(&format!(
        "\n  ai_component NoTools {{\n    {clause}\n  }}\n"
    ));
    let (mut value, status) = run(&fixture, consumer);
    println!("{consumer:?}: produced exit={status}, output={value}");
    assert_eq!(status, 2, "{consumer:?}: {value}");
    // Supplied base JSON includes versions only on these three commands.
    // Package/installed Z3 versions identify the build, not error precedence;
    // check their presence and shape separately before comparing diagnostics.
    if matches!(
        consumer,
        Consumer::Check | Consumer::Bmc | Consumer::Induction
    ) {
        let versions = value
            .as_object_mut()
            .expect("envelope")
            .remove("versions")
            .expect("base check/verify envelope includes versions");
        assert_eq!(versions["verifier"]["name"], "fslc-rust", "{versions}");
        assert_eq!(versions["core"]["name"], "fsl-core", "{versions}");
        assert_eq!(versions["solver"]["backend"], "native-z3", "{versions}");
        for component in ["verifier", "core", "solver"] {
            assert!(
                versions[component]["version"]
                    .as_str()
                    .is_some_and(|version| !version.is_empty()),
                "{versions}"
            );
        }
    }
    // Preserve the entire legacy envelope for each command, not only a shared
    // message substring. Lowering's authority diagnostic has a real origin;
    // specialized check's earlier validation is historically unlocated.
    let mut expected = json!({
        "fsl": "1.0",
        "result": "error",
        "kind": "semantics",
        "message": if authority {
            "unknown tool 'Foo' in authority block"
        } else {
            "unknown ai hard-contract rule 'bogus' at 3:5"
        }
    });
    if authority && matches!(consumer, Consumer::Scenarios | Consumer::Testgen) {
        expected["message"] = json!(format!(
            "unknown tool 'Foo' in authority block at {}:3:17",
            fixture.path.display()
        ));
        expected["loc"] = json!({"line": 3, "column": 17});
    } else if !authority {
        expected["hint"] = json!(
            "supported rules: forbidden_tool_blocked, human_approval_required, tool_authority, tool_precondition_declared, tool_schema_declared"
        );
    }
    assert_eq!(value, expected, "{consumer:?}");
    assert!(
        !fixture.generated.exists(),
        "legacy rejection must not write a harness"
    );
}

macro_rules! ai_precedence_consumers {
    ($module:ident, $authority:expr) => {
        mod $module {
            use super::*;
            #[test]
            fn check() {
                ai_error_precedence(Consumer::Check, $authority);
            }
            #[test]
            fn ai_check() {
                ai_error_precedence(Consumer::AiCheck, $authority);
            }
            #[test]
            fn bmc() {
                ai_error_precedence(Consumer::Bmc, $authority);
            }
            #[test]
            fn induction() {
                ai_error_precedence(Consumer::Induction, $authority);
            }
            #[test]
            fn scenarios() {
                ai_error_precedence(Consumer::Scenarios, $authority);
            }
            #[test]
            fn testgen() {
                ai_error_precedence(Consumer::Testgen, $authority);
            }
        }
    };
}

ai_precedence_consumers!(ai_bad_rule_precedence, false);
ai_precedence_consumers!(ai_unknown_authority_precedence, true);
