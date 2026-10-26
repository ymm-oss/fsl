// SPDX-License-Identifier: Apache-2.0

//! Contract-driven argv membership, before command-local typed interpretation.
//! Only diagnostic rendering and the existing unstable overlay are command-specific;
//! stable option ownership, arity, choices and positional limits come from metadata.

use fsl_syntax::{ParseOptions, UnstableFeature};
use serde_json::{Value, json};

pub(crate) fn validate_argv(
    contract: &str,
    argv: &[String],
    options: ParseOptions,
) -> Result<(), String> {
    let contract: Value = serde_json::from_str(contract).expect("valid embedded CLI contract");
    let mut node = &contract["root"];
    let mut index = 0;
    while !children(node).is_empty() {
        let Some(word) = argv.get(index) else {
            // Required subcommands and their usage text belong to the dispatcher.
            return Ok(());
        };
        if let Some(child) = children(node).iter().find(|child| {
            child["path"].as_array().and_then(|path| path.last()) == Some(&json!(word))
        }) {
            node = child;
            index += 1;
        } else {
            // Root metadata options are not inherited by children. A bare unknown
            // command keeps the dispatcher's diagnostic, just like a dash option.
            if node["path"].as_array().is_some_and(Vec::is_empty)
                && actions(node).iter().any(|action| has_flag(action, word))
            {
                break;
            }
            return Err(group_unknown(&label(node), word));
        }
    }
    let command = label(node);
    let mut actions = actions(node).to_vec();
    let gated = options.unstable().contains(UnstableFeature::Fsl5);
    if gated {
        unstable_overlay(&command, &mut actions);
    }
    validate_tokens(&command, &actions, &argv[index..], options)
}

fn children(node: &Value) -> &[Value] {
    node["commands"].as_array().expect("CLI commands")
}

fn actions(node: &Value) -> &[Value] {
    node["actions"].as_array().expect("CLI actions")
}

fn label(node: &Value) -> String {
    node["path"]
        .as_array()
        .expect("CLI path")
        .iter()
        .map(|part| part.as_str().expect("CLI path segment"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn has_flag(action: &Value, flag: &str) -> bool {
    action["flags"]
        .as_array()
        .is_some_and(|flags| flags.iter().any(|value| value.as_str() == Some(flag)))
}

fn unstable_overlay(command: &str, actions: &mut Vec<Value>) {
    let extras = match command {
        "explain" => json!([
            {"flags":["--names"],"choices":["canonical","alias"]},
            {"flags":["--lowering"],"nargs":0}
        ]),
        "fmt" => json!([
            {"flags":["--lang"],"choices":["ja","en"]},
            {"flags":["--names"],"choices":["alias","canonical"]}
        ]),
        "lint" => json!([{"flags":["--strict-names"],"nargs":0}]),
        _ => json!([]),
    };
    actions.extend(extras.as_array().expect("unstable overlay").iter().cloned());
    // Edition's numeric aliases are also already gated in Edition::parse_option.
    for action in actions
        .iter_mut()
        .filter(|action| has_flag(action, "--edition"))
    {
        action["choices"]
            .as_array_mut()
            .expect("edition choices")
            .extend([json!("4"), json!("5")]);
    }
}

fn positional_capacity(actions: &[Value]) -> Option<u64> {
    actions
        .iter()
        .filter(|action| action["positional"] == true)
        .try_fold(0, |sum, action| {
            let capacity = match action.get("nargs") {
                None => 1,
                Some(Value::String(nargs)) if nargs == "?" => 1,
                Some(Value::String(nargs)) if matches!(nargs.as_str(), "+" | "*") => return None,
                Some(nargs) => nargs
                    .as_u64()
                    .expect("supported positional nargs in CLI contract"),
            };
            Some(sum + capacity)
        })
}

fn validate_tokens(
    command: &str,
    actions: &[Value],
    argv: &[String],
    options: ParseOptions,
) -> Result<(), String> {
    let capacity = positional_capacity(actions);
    let mut positionals = Vec::new();
    let mut index = 0;
    while let Some(token) = argv.get(index) {
        index += 1;
        if token == "--" {
            positionals.extend(argv[index..].iter());
            break;
        }
        if token == "-" || !token.starts_with('-') {
            positionals.push(token);
            continue;
        }
        let Some(action) = actions.iter().find(|action| has_flag(action, token)) else {
            return Err(unknown_option(command, token, index == argv.len()));
        };
        let arity = action.get("nargs").map_or(1, |nargs| {
            nargs
                .as_u64()
                .expect("numeric option nargs in CLI contract")
        });
        assert!(arity <= 1, "unsupported option nargs in CLI contract");
        if arity == 0 {
            continue;
        }
        let Some(value) = argv.get(index).filter(|value| value.as_str() != "--") else {
            return Err(missing_value(command, token));
        };
        index += 1;
        if let Some(choices) = action["choices"].as_array()
            && !choices.iter().any(|choice| choice.as_str() == Some(value))
        {
            return Err(choice_message(command, token, value, options));
        }
    }
    if let Some(capacity) = capacity
        && let Some(extra) =
            positionals.get(usize::try_from(capacity).expect("positional capacity"))
    {
        return Err(extra_positional(command, extra));
    }
    Ok(())
}

fn missing_value(command: &str, flag: &str) -> String {
    match (command, flag) {
        ("mutate", "--from") => "--from requires a JSONL path".to_owned(),
        ("db observe", "--trace") => "--trace requires a path".to_owned(),
        ("domain replay", "--logs") => "--logs requires a path".to_owned(),
        (_, "-o" | "--output") => {
            if matches!(
                command,
                "approval create" | "document generate" | "document claims"
            ) {
                "--output requires a value".to_owned()
            } else {
                "--output requires a path".to_owned()
            }
        }
        _ => crate::cli_options::required_option_value(&mut std::iter::empty(), flag).unwrap_err(),
    }
}

fn group_unknown(command: &str, word: &str) -> String {
    match command {
        "" => format!("unknown command '{word}'"),
        "compat" => "usage: fslc compat check DBSYSTEM [--include-ai]".to_owned(),
        "causal" => format!(
            "unknown causal subcommand '{word}' (expected check | analyze | verify-expectations | observe-expectations | diff | ledger; there is deliberately no 'causal verify')"
        ),
        _ => format!("unknown {command} subcommand '{word}'"),
    }
}

fn unknown_option(command: &str, option: &str, terminal: bool) -> String {
    // Retain the old helper labels until the separately reviewed diagnostic phase.
    let label = match command {
        "ai check" | "domain check" => "specialized check",
        "domain expand" => "output",
        "scenarios" => "verify",
        "db observe" => return "unexpected db observe argument".to_owned(),
        "ai compare" if terminal => return format!("{option} requires a value"),
        _ => command,
    };
    format!("unknown {label} option '{option}'")
}

fn extra_positional(command: &str, extra: &str) -> String {
    match command {
        "kernel" | "conformance" | "testplan" | "chain" => {
            format!("unexpected {command} argument '{extra}'")
        }
        "diff" => "diff requires OLD NEW or --git BASE..HEAD [SPEC]".to_owned(),
        "version" | "" => format!("unexpected version argument '{extra}'"),
        _ => unknown_option(command, extra, false),
    }
}

// These are compatibility renderers, not acceptance tables. Membership is
// determined exclusively by the current action's choices above.
fn choice_message(command: &str, flag: &str, value: &str, options: ParseOptions) -> String {
    match (command, flag) {
        (_, "--kernel-version") => fsl_core::PublicKernelVersion::parse(value).unwrap_err().to_string(),
        (_, "--edition") => fsl_syntax::Edition::parse_option(value, &options).unwrap_err(),
        (_, "--deadlock") => "--deadlock must be warn, error, or ignore".to_owned(),
        (_, "--vacuity") => "--vacuity must be warn, error, or ignore".to_owned(),
        ("verify", "--engine") => "--engine must be bmc, induction, explicit, or auto".to_owned(),
        ("counterexample export", "--engine") => "--engine must be bmc, explicit, auto, or induction".to_owned(),
        (_, "--engine") => "--engine must be bmc or induction".to_owned(),
        ("analyze", "--projection") => format!("unsupported projection: {value}"),
        ("analyze", "--profile") => format!("unsupported profile: {value}"),
        ("analyze", "--export") => format!("unsupported analyze export: {value}"),
        ("analyze", "--format") => format!("unsupported analyze format: {value}"),
        ("causal analyze", "--projection") => format!("unknown causal projection '{value}'"),
        ("causal analyze", "--profile") => format!("unknown causal profile '{value}'"),
        ("causal analyze", "--format") => format!("unsupported causal analyze format: {value}"),
        ("causal diff", "--format") => "causal diff supports only --format json".to_owned(),
        ("causal ledger", "--format") => "causal ledger supports only --format json".to_owned(),
        ("db import", "--source") => "--source must be auto, sql, or prisma".to_owned(),
        ("domain generate", "--target") => format!("unsupported domain generation target: {value}"),
        ("domain generate", "--profile") => "--profile must be functional-ddd".to_owned(),
        ("testgen" | "domain testgen", "--target") => format!("unknown testgen target '{value}'; expected pytest, vitest, swift, kotlin, dart, or phpunit"),
        ("approval create", "--kind") => "--kind must be ledger, html, scenarios, or requirements_document".to_owned(),
        (_, "--view") => "--view must be requirements ('business'/'design' are reserved until docs/design/DESIGN-document-dialect-adapters.md's activation contract is met, issue #334)".to_owned(),
        (_, "--lang") => "--lang must be ja or en".to_owned(),
        ("explain", "--names") => "--names must be canonical or alias".to_owned(),
        ("fmt", "--names") => "--names must be alias or canonical".to_owned(),
        _ => panic!("missing CLI choices diagnostic for {command} {flag}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(argv: &[&str]) -> Result<(), String> {
        validate_with(argv, ParseOptions::stable())
    }

    fn validate_with(argv: &[&str], options: ParseOptions) -> Result<(), String> {
        validate_argv(
            crate::CLI_CONTRACT,
            &argv.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>(),
            options,
        )
    }

    #[test]
    fn consumes_only_owned_option_values_and_exact_aliases() {
        for flag in ["-o", "--output"] {
            assert_eq!(
                validate(&["testgen", flag, "--not-an-option-here", "spec.fsl"]),
                Ok(())
            );
        }
        assert_eq!(
            validate(&["explain", "--max-mutants", "2", "spec.fsl"]),
            Err("unknown explain option '--max-mutants'".to_owned())
        );
        for flag in ["--output=out", "-oout", "-oo"] {
            assert_eq!(
                validate(&["testgen", "spec.fsl", flag]),
                Err(format!("unknown testgen option '{flag}'"))
            );
        }
        assert_eq!(validate(&["verify", "--depth", "-1", "spec.fsl"]), Ok(()));
    }

    #[test]
    fn terminator_makes_all_remaining_tokens_positional() {
        assert_eq!(validate(&["check", "--", "-missing.fsl"]), Ok(()));
        assert_eq!(
            validate(&["check", "--", "a", "--depth"]),
            Err("unknown check option '--depth'".to_owned())
        );
        // A required value before -- is missing, not a literal choice or path.
        assert_eq!(
            validate(&["verify", "spec.fsl", "--deadlock", "--", "warn"]),
            Err("--deadlock requires a value".to_owned())
        );
        assert_eq!(
            validate(&["ai", "compare", "--from", "--"]),
            Err("--from requires a value".to_owned())
        );
        assert_eq!(
            validate(&["testgen", "-o", "--", "out"]),
            Err("--output requires a path".to_owned())
        );
    }

    #[test]
    fn invalid_choice_cannot_be_hidden_by_a_later_valid_repeat() {
        for argv in [
            vec![
                "db",
                "check",
                "spec.fsl",
                "--deadlock",
                "invalid",
                "--deadlock",
                "warn",
            ],
            vec![
                "db",
                "check",
                "spec.fsl",
                "--deadlock",
                "warn",
                "--deadlock",
                "invalid",
            ],
        ] {
            assert_eq!(
                validate(&argv),
                Err("--deadlock must be warn, error, or ignore".to_owned())
            );
        }
        assert_eq!(
            validate(&["kernel", "--kernel-version", "9", "--unknown", "spec.fsl"]),
            Err("unsupported public Kernel major '9'; supported majors are 1 and 2".to_owned())
        );
        assert_eq!(
            validate(&[
                "testgen", "--target", "pytest", "--target", "vitest", "spec.fsl"
            ]),
            Ok(())
        );
    }

    #[test]
    fn optional_positionals_have_limits_but_star_and_plus_do_not() {
        assert_eq!(validate(&["diff", "a", "b"]), Ok(()));
        assert_eq!(
            validate(&["diff", "a", "b", "c"]),
            Err("diff requires OLD NEW or --git BASE..HEAD [SPEC]".to_owned())
        );
        assert_eq!(
            validate(&["chain", "a", "b"]),
            Err("unexpected chain argument 'b'".to_owned())
        );
        assert_eq!(
            validate(&["version", "extra"]),
            Err("unexpected version argument 'extra'".to_owned())
        );
        assert_eq!(validate(&["refine", "a", "b", "c", "d"]), Ok(()));
        assert_eq!(validate(&["analyze", "a", "b", "c", "d"]), Ok(()));
        assert_eq!(
            positional_capacity(&[
                json!({"positional":true,"nargs":"?"}),
                json!({"positional":true,"nargs":"*"})
            ]),
            None
        );
        assert_eq!(
            positional_capacity(&[
                json!({"positional":true,"nargs":3}),
                json!({"positional":true,"nargs":"?"})
            ]),
            Some(4)
        );
    }

    #[test]
    fn command_resolution_stops_at_the_leaf_and_does_not_inherit_root_flags() {
        assert_eq!(validate(&["domain", "analyze", "check"]), Ok(()));
        assert_eq!(
            validate(&["check", "--version", "spec.fsl"]),
            Err("unknown check option '--version'".to_owned())
        );
        assert_eq!(
            validate(&["domain", "--unknown"]),
            Err("unknown domain subcommand '--unknown'".to_owned())
        );
        assert_eq!(
            validate(&["compat", "bogus"]),
            Err("usage: fslc compat check DBSYSTEM [--include-ai]".to_owned())
        );
    }

    #[test]
    fn unstable_overlay_is_command_local_and_does_not_bypass_stable_validation() {
        let options = ParseOptions::with_unstable(
            fsl_syntax::UnstableFeatures::NONE.with(UnstableFeature::Fsl5),
        );
        for argv in [
            vec!["explain", "spec.fsl", "--names", "canonical", "--lowering"],
            vec!["fmt", "spec.fsl", "--lang", "en", "--names", "alias"],
            vec!["lint", "spec.fsl", "--strict-names"],
            vec!["check", "spec.fsl", "--edition", "5"],
        ] {
            assert!(validate(&argv).is_err(), "{argv:?}");
            assert_eq!(validate_with(&argv, options), Ok(()), "{argv:?}");
        }
        assert_eq!(
            validate_with(&["testgen", "--engine", "bmc", "spec.fsl"], options),
            Err("unknown testgen option '--engine'".to_owned())
        );
        assert_eq!(
            validate_with(&["fmt", "--names", "invalid", "spec.fsl"], options),
            Err("--names must be alias or canonical".to_owned())
        );
    }

    #[test]
    #[should_panic(expected = "unsupported option nargs in CLI contract")]
    fn unsupported_option_arity_requires_an_explicit_schema_decision() {
        let _ = validate_tokens(
            "test",
            &[json!({"flags":["--pair"],"nargs":2})],
            &["--pair".to_owned()],
            ParseOptions::stable(),
        );
    }
}
