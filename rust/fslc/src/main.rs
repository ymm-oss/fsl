// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;
use std::future::Future;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::task::{Context, Poll, Waker};
use std::time::Instant;

use fsl_core::{
    Annotations, FslValue, KernelExpr, KernelLValue, KernelModel, KernelSpec, KernelStatement,
    ParamDef, TypeDef, TypeRef, insert_requirement_metadata, model_warnings, requirement_metadata,
};
use serde_json::{Map, Value, json};

mod approval;
mod code_audit;
mod verification;

use verification::{
    BmcRequest, DeadlockMode, ExplicitRequest, InductionRequest, ModelSelection,
    VerificationEngine, run_auto_filtered, run_bmc_filtered, run_explicit_filtered,
    run_induction_filtered, run_verify_cli,
};

const CLI_CONTRACT: &str = include_str!("../cli-contract.json");
const DEFAULT_EXPLICIT_BUDGET: usize = 1_000_000;

struct LiterateState {
    path: PathBuf,
}

impl Drop for LiterateState {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn materialize_literate(path: &Path) -> Result<Option<LiterateState>, String> {
    if path.extension().and_then(std::ffi::OsStr::to_str) != Some("md") {
        return Ok(None);
    }
    let raw =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let blanked = fsl_syntax::extract_literate_fsl(&raw).ok_or_else(|| {
        format!(
            "{}: Markdown file does not contain any ```fsl fenced code blocks",
            path.display()
        )
    })?;
    let stem = path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("literate");
    // Each CLI process owns its materialization. The original Markdown path is
    // passed separately as the stable verify-cache identity, so physical
    // isolation does not trade away cache hits across invocations.
    let materialized = literate_materialization_path(path, stem, std::process::id());
    std::fs::write(&materialized, &blanked).map_err(|error| error.to_string())?;
    Ok(Some(LiterateState { path: materialized }))
}

fn literate_materialization_path(path: &Path, stem: &str, process_id: u32) -> PathBuf {
    path.with_file_name(format!(".{stem}.literate-{process_id}.fsl"))
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ScopeBounds {
    instances: std::collections::BTreeMap<String, i64>,
    values: std::collections::BTreeMap<String, (i64, i64)>,
}

#[derive(Clone, Debug)]
struct CliVerifyOptions {
    depth: usize,
    deadlock: String,
    engine: String,
    explicit_budget: usize,
    k_ind: usize,
    vacuity: String,
    property: Option<String>,
    exclude_properties: Vec<String>,
    scope: ScopeBounds,
    strict_tags: bool,
    requirements: Option<PathBuf>,
    use_cache: bool,
    lemmas: Vec<String>,
    from_state: Option<PathBuf>,
    edition: String,
}

impl Default for CliVerifyOptions {
    fn default() -> Self {
        Self {
            depth: 8,
            deadlock: "warn".to_owned(),
            engine: "bmc".to_owned(),
            explicit_budget: DEFAULT_EXPLICIT_BUDGET,
            k_ind: 1,
            vacuity: "warn".to_owned(),
            property: None,
            exclude_properties: Vec::new(),
            scope: ScopeBounds::default(),
            strict_tags: false,
            requirements: None,
            use_cache: true,
            lemmas: Vec::new(),
            from_state: None,
            edition: "current".to_owned(),
        }
    }
}

fn required_option_value(
    args: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn parse_instance_override(raw: &str) -> Result<(String, i64), String> {
    let (name, value) = raw
        .split_once('=')
        .ok_or_else(|| format!("invalid --instances value '{raw}': expected NAME=N"))?;
    let value = value
        .parse::<i64>()
        .map_err(|_| format!("invalid --instances value '{raw}': N must be an integer"))?;
    if name.trim().is_empty() || value < 1 {
        return Err(format!(
            "invalid --instances value '{raw}': expected a non-empty NAME and N >= 1"
        ));
    }
    Ok((name.trim().to_owned(), value))
}

fn parse_verify_options(
    args: &mut impl Iterator<Item = String>,
) -> Result<CliVerifyOptions, String> {
    let mut options = CliVerifyOptions::default();
    while let Some(option) = args.next() {
        match option.as_str() {
            "--depth" => {
                options.depth = required_option_value(args, "--depth")?
                    .parse()
                    .map_err(|_| "--depth must be a non-negative integer".to_owned())?;
            }
            "--deadlock" => {
                options.deadlock = required_option_value(args, "--deadlock")?;
                if !matches!(options.deadlock.as_str(), "warn" | "error" | "ignore") {
                    return Err("--deadlock must be warn, error, or ignore".to_owned());
                }
            }
            "--engine" => {
                options.engine = required_option_value(args, "--engine")?;
                if !matches!(
                    options.engine.as_str(),
                    "bmc" | "induction" | "explicit" | "auto"
                ) {
                    return Err("--engine must be bmc, induction, explicit, or auto".to_owned());
                }
            }
            "--explicit-budget" => {
                options.explicit_budget = required_option_value(args, "--explicit-budget")?
                    .parse()
                    .map_err(|_| "--explicit-budget must be a positive integer".to_owned())?;
                if options.explicit_budget == 0 {
                    return Err("--explicit-budget must be a positive integer".to_owned());
                }
            }
            "--k" => {
                options.k_ind = required_option_value(args, "--k")?
                    .parse()
                    .map_err(|_| "--k must be a positive integer".to_owned())?;
                if options.k_ind == 0 {
                    return Err("--k must be a positive integer".to_owned());
                }
            }
            "--vacuity" => {
                options.vacuity = required_option_value(args, "--vacuity")?;
                if !matches!(options.vacuity.as_str(), "warn" | "error" | "ignore") {
                    return Err("--vacuity must be warn, error, or ignore".to_owned());
                }
            }
            "--property" => options.property = Some(required_option_value(args, "--property")?),
            "--exclude-property" => options
                .exclude_properties
                .push(required_option_value(args, "--exclude-property")?),
            "--instances" => {
                let raw = required_option_value(args, "--instances")?;
                let (name, value) = parse_instance_override(&raw)?;
                options.scope.instances.insert(name, value);
            }
            "--values" => {
                let raw = required_option_value(args, "--values")?;
                let (name, value) = parse_sweep_range(&raw, "--values")?;
                options.scope.values.insert(name, value);
            }
            "--strict-tags" => options.strict_tags = true,
            "--requirements" => {
                options.requirements = Some(PathBuf::from(required_option_value(
                    args,
                    "--requirements",
                )?));
            }
            "--no-cache" => options.use_cache = false,
            "--lemma" => options.lemmas.push(required_option_value(args, "--lemma")?),
            "--from-state" => {
                options.from_state =
                    Some(PathBuf::from(required_option_value(args, "--from-state")?));
            }
            "--edition" => options.edition = parse_edition(args)?,
            _ => return Err(format!("unknown verify option '{option}'")),
        }
    }
    Ok(options)
}

fn parse_edition(args: &mut impl Iterator<Item = String>) -> Result<String, String> {
    let edition = required_option_value(args, "--edition")?;
    if matches!(edition.as_str(), "current" | "next") {
        Ok(edition)
    } else {
        Err("--edition must be current or next".to_owned())
    }
}

fn parse_specialized_verify_options(
    args: &mut impl Iterator<Item = String>,
    allow_edition: bool,
) -> Result<(usize, String, String, String), String> {
    let mut depth = 8_usize;
    let mut deadlock = "warn".to_owned();
    let mut engine = "bmc".to_owned();
    let mut edition = "current".to_owned();
    while let Some(option) = args.next() {
        match option.as_str() {
            "--depth" => {
                depth = args
                    .next()
                    .ok_or_else(|| "--depth requires a value".to_owned())?
                    .parse()
                    .map_err(|_| "--depth must be a non-negative integer".to_owned())?;
            }
            "--deadlock" => {
                deadlock = args
                    .next()
                    .ok_or_else(|| "--deadlock requires a value".to_owned())?;
            }
            "--engine" => {
                engine = args
                    .next()
                    .ok_or_else(|| "--engine requires a value".to_owned())?;
                if !matches!(engine.as_str(), "bmc" | "induction") {
                    return Err("--engine must be bmc or induction".to_owned());
                }
            }
            "--edition" if allow_edition => edition = parse_edition(args)?,
            _ => return Err(format!("unknown specialized check option '{option}'")),
        }
    }
    Ok((depth, deadlock, engine, edition))
}

fn parse_optional_output(
    args: &mut impl Iterator<Item = String>,
) -> Result<Option<PathBuf>, String> {
    match args.next() {
        None => Ok(None),
        Some(option) if matches!(option.as_str(), "-o" | "--output") => {
            let output = PathBuf::from(
                args.next()
                    .ok_or_else(|| "--output requires a path".to_owned())?,
            );
            if args.next().is_some() {
                return Err("unexpected argument after --output".to_owned());
            }
            Ok(Some(output))
        }
        Some(option) => Err(format!("unknown output option '{option}'")),
    }
}

fn main() {
    if print_cli_metadata() {
        return;
    }
    if let Some((output, status)) = fmt_command() {
        match output {
            FmtCliOutput::Source(source) => print!("{source}"),
            FmtCliOutput::Json(value) => println!(
                "{}",
                serde_json::to_string_pretty(&value).expect("serialize fmt output")
            ),
        }
        if status != 0 {
            std::process::exit(status);
        }
        return;
    }
    let (output, reported_status) = match command() {
        Ok(result) => result,
        Err(error) => (error_output("usage", &error), 2),
    };
    let status = normalized_exit_status(&output, reported_status);
    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("serialize CLI output")
    );
    if status != 0 {
        std::process::exit(status);
    }
}

enum FmtCliOutput {
    Source(String),
    Json(Value),
}

fn fmt_command() -> Option<(FmtCliOutput, i32)> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("fmt") {
        return None;
    }
    Some(match parse_fmt_options(args) {
        Ok(options) => run_fmt(&options),
        Err(error) => (FmtCliOutput::Json(error_output("usage", &error)), 2),
    })
}

struct FmtOptions {
    paths: Vec<PathBuf>,
    check: bool,
    edition: fsl_syntax::FormatEdition,
}

fn parse_fmt_options(args: impl Iterator<Item = String>) -> Result<FmtOptions, String> {
    let mut args = args.peekable();
    let mut paths = Vec::new();
    let mut check = false;
    let mut edition = fsl_syntax::FormatEdition::Current;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--check" => check = true,
            "--edition" => {
                edition = fsl_syntax::FormatEdition::parse(&required_option_value(
                    &mut args,
                    "--edition",
                )?)?;
            }
            option if option.starts_with('-') && option != "-" => {
                return Err(format!("unknown fmt option '{option}'"));
            }
            path => paths.push(PathBuf::from(path)),
        }
    }
    if paths.is_empty() {
        return Err("usage: fslc fmt FILE [--check] [--edition current|next]".to_owned());
    }
    if !check && paths.len() != 1 {
        return Err("fmt accepts multiple paths only with --check".to_owned());
    }
    if paths.iter().filter(|path| path.as_os_str() == "-").count() > 1
        || paths.len() > 1 && paths.iter().any(|path| path.as_os_str() == "-")
    {
        return Err("stdin '-' cannot be repeated or mixed with file paths".to_owned());
    }
    Ok(FmtOptions {
        paths,
        check,
        edition,
    })
}

fn run_fmt(options: &FmtOptions) -> (FmtCliOutput, i32) {
    let mut files = Vec::new();
    let mut any_changed = false;
    for path in &options.paths {
        let source = match read_fmt_source(path) {
            Ok(source) => source,
            Err(error) => return (FmtCliOutput::Json(error_output("io", &error)), 2),
        };
        let formatted = match fsl_syntax::format_source(&source, options.edition) {
            Ok(formatted) => formatted,
            Err(fsl_syntax::FormatError::Parse(error)) => {
                return (FmtCliOutput::Json(surface_parse_error_output(&error)), 2);
            }
            Err(error) => return (FmtCliOutput::Json(format_error_output(&error)), 2),
        };
        if let Err(error) = validate_fmt_semantics(&source, path) {
            return (FmtCliOutput::Json(semantic_error_output(&error)), 2);
        }
        if let Err(error) = validate_fmt_semantics(&formatted, path) {
            return (FmtCliOutput::Json(semantic_error_output(&error)), 2);
        }
        let changed = source != formatted;
        any_changed |= changed;
        if options.check {
            files.push(json!({"path":path.to_string_lossy(),"changed":changed}));
        } else {
            return (FmtCliOutput::Source(formatted), 0);
        }
    }
    (
        FmtCliOutput::Json(json!({
            "fsl":"1.0",
            "result":"format_check",
            "edition":options.edition.as_str(),
            "changed":any_changed,
            "files":files
        })),
        i32::from(any_changed),
    )
}

fn read_fmt_source(path: &Path) -> Result<String, String> {
    if path.as_os_str() == "-" {
        let mut source = String::new();
        std::io::stdin()
            .read_to_string(&mut source)
            .map_err(|error| error.to_string())?;
        Ok(source)
    } else {
        std::fs::read_to_string(path).map_err(|error| error.to_string())
    }
}

struct MigrationOptions {
    paths: Vec<PathBuf>,
    edition: fslc_rust::migration::Edition,
    write: bool,
    project: Option<PathBuf>,
}

fn parse_migration_options(
    args: impl Iterator<Item = String>,
    migrate: bool,
) -> Result<MigrationOptions, String> {
    let mut args = args.peekable();
    let mut paths = Vec::new();
    let mut edition = if migrate {
        fslc_rust::migration::Edition::Next
    } else {
        fslc_rust::migration::Edition::Current
    };
    let mut write = false;
    let mut project = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--edition" => {
                edition = fslc_rust::migration::Edition::parse(&required_option_value(
                    &mut args,
                    "--edition",
                )?)?;
            }
            "--write" if migrate => write = true,
            "--project" if !migrate => {
                project = Some(PathBuf::from(required_option_value(
                    &mut args,
                    "--project",
                )?));
            }
            option if option.starts_with('-') => {
                return Err(format!(
                    "unknown {} option '{option}'",
                    if migrate { "migrate" } else { "lint" }
                ));
            }
            path => paths.push(PathBuf::from(path)),
        }
    }
    if paths.is_empty() {
        return Err(if migrate {
            "usage: fslc migrate FILE... [--edition current|next] [--write]".to_owned()
        } else {
            "usage: fslc lint FILE... [--edition current|next] [--project fsl-project.toml]"
                .to_owned()
        });
    }
    if migrate {
        let mut unique = std::collections::BTreeSet::new();
        if paths.iter().any(|path| !unique.insert(path.clone())) {
            return Err("the same migration path cannot be supplied twice".to_owned());
        }
    }
    Ok(MigrationOptions {
        paths,
        edition,
        write,
        project,
    })
}

fn run_lint(options: &MigrationOptions) -> (Value, i32) {
    let (id_policy, policy_source) = match load_id_policy(options.project.as_deref()) {
        Ok(policy) => policy,
        Err(error) => return (error_output("config", &error), 2),
    };
    let paths = match expand_lint_paths(&options.paths) {
        Ok(paths) => paths,
        Err(error) => return (error_output("io", &error), 2),
    };
    let mut files = Vec::new();
    let mut finding_count = 0_usize;
    for path in &paths {
        let (source, mut plan) = match load_migration_plan(path, options.edition) {
            Ok(loaded) => loaded,
            Err(error) => return (error, 2),
        };
        plan.diagnostics
            .extend(fslc_rust::migration::id_policy_diagnostics(
                &source, &id_policy,
            ));
        plan.diagnostics
            .sort_by_key(|diagnostic| diagnostic.span.start.offset);
        finding_count += plan.diagnostics.len();
        files.push(json!({
            "path": path,
            "findings": plan.diagnostics.iter().map(|finding| finding.json(&path.to_string_lossy(), options.edition)).collect::<Vec<_>>(),
        }));
    }
    (
        json!({
            "fsl":"1.0",
            "result":"lint",
            "edition":options.edition.as_str(),
            "finding_count":finding_count,
            "id_policy":{
                "source":policy_source,
                "patterns":id_policy.json(),
            },
            "files":files,
        }),
        i32::from(finding_count > 0),
    )
}

fn expand_lint_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut files = std::collections::BTreeSet::new();
    for path in paths {
        if path.is_dir() {
            collect_lint_directory(path, &mut files)?;
        } else {
            files.insert(path.clone());
        }
    }
    let mut unique_files = std::collections::BTreeMap::new();
    for path in files {
        let identity =
            std::fs::canonicalize(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        match unique_files.entry(identity) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(path);
            }
            std::collections::btree_map::Entry::Occupied(mut entry)
                if path.components().count() < entry.get().components().count() =>
            {
                entry.insert(path);
            }
            std::collections::btree_map::Entry::Occupied(_) => {}
        }
    }
    let mut files = unique_files.into_values().collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn collect_lint_directory(
    directory: &Path,
    files: &mut std::collections::BTreeSet<PathBuf>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", directory.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_lint_directory(&path, files)?;
        } else if file_type.is_file()
            && path.extension().and_then(std::ffi::OsStr::to_str) == Some("fsl")
        {
            files.insert(path);
        }
    }
    Ok(())
}

fn load_id_policy(
    project: Option<&Path>,
) -> Result<(fslc_rust::migration::IdPolicy, String), String> {
    let mut policy = fslc_rust::migration::IdPolicy::default();
    let Some(project) = project else {
        return Ok((policy, "builtin".to_owned()));
    };
    let source = std::fs::read_to_string(project)
        .map_err(|error| format!("{}: {error}", project.display()))?;
    let sections = parse_project_manifest(&source)?;
    if let Some(patterns) = sections.get("id_policy.patterns") {
        for (kind, raw_patterns) in &patterns.values {
            let kind = fslc_rust::migration::IdKind::parse(kind)?;
            if raw_patterns.starts_with('\'') || raw_patterns.ends_with('\'') {
                return Err(format!(
                    "invalid [id_policy.patterns].{} in {}: use double-quoted JSON-compatible strings",
                    kind.as_str(),
                    project.display()
                ));
            }
            let values = if raw_patterns.trim_start().starts_with('[') {
                serde_json::from_str::<Vec<String>>(raw_patterns).map_err(|error| {
                    format!(
                        "invalid [id_policy.patterns].{} in {}: {error}",
                        kind.as_str(),
                        project.display()
                    )
                })?
            } else {
                vec![raw_patterns.clone()]
            };
            policy.set_patterns(kind, values)?;
        }
    }
    policy.validate()?;
    Ok((policy, project.display().to_string()))
}

fn run_migrate(options: &MigrationOptions) -> (Value, i32) {
    let mut planned = Vec::new();
    let mut files = Vec::new();
    let mut refused = false;
    for path in &options.paths {
        let (source, plan) = match load_migration_plan(path, options.edition) {
            Ok(loaded) => loaded,
            Err(error) => return (error, 2),
        };
        refused |= plan.refused();
        let migrated = plan.migrated_source.as_ref();
        if let Some(migrated) = migrated
            && let Err(error) = validate_migration_semantics(&source, migrated, path)
        {
            return (semantic_error_output(&error), 2);
        }
        files.push(json!({
            "path":path,
            "changed":migrated.is_some(),
            "findings":plan.diagnostics.iter().map(|finding| finding.json(&path.to_string_lossy(), options.edition)).collect::<Vec<_>>(),
            "edits":migrated.map(|replacement| vec![json!({"start":0,"end":source.len(),"replacement":replacement})]).unwrap_or_default(),
        }));
        planned.push((path.clone(), migrated.cloned()));
    }
    if refused {
        return (
            json!({
                "fsl":"1.0",
                "result":"migration_refused",
                "edition":options.edition.as_str(),
                "written":false,
                "files":files,
            }),
            2,
        );
    }
    if options.write {
        let writes = planned
            .iter()
            .filter_map(|(path, source)| {
                source
                    .as_ref()
                    .map(|source| (path.as_path(), source.as_str()))
            })
            .collect::<Vec<_>>();
        if let Err(error) = atomic_write_migrations(&writes) {
            return (error_output("io", &error), 2);
        }
    }
    let changed = planned
        .iter()
        .filter(|(_, source)| source.is_some())
        .count();
    (
        json!({
            "fsl":"1.0",
            "result":"migrated",
            "edition":options.edition.as_str(),
            "changed":changed,
            "written":options.write,
            "files":files,
        }),
        0,
    )
}

fn load_migration_plan(
    path: &Path,
    edition: fslc_rust::migration::Edition,
) -> Result<(String, fslc_rust::migration::MigrationPlan), Value> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| error_output("io", &format!("{}: {error}", path.display())))?;
    let plan = fslc_rust::migration::plan_migration(&source, &path.to_string_lossy(), edition)
        .map_err(|error| migration_plan_error_output(&source, path, &error))?;
    Ok((source, plan))
}

fn validate_migration_semantics(before: &str, after: &str, path: &Path) -> Result<(), String> {
    let before = migration_kernel_contract(before, path)?;
    let after = migration_kernel_contract(after, path)?;
    if before == after {
        Ok(())
    } else {
        Err(format!(
            "migration changed the checked Public Kernel for '{}'",
            path.display()
        ))
    }
}

fn migration_kernel_contract(source: &str, path: &Path) -> Result<Value, String> {
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let resolver = fsl_core::FsResolver::new(base);
    let kernel = fsl_core::parse_kernel_source_with_file(source, &resolver, path.to_string_lossy())
        .map_err(|error| error.to_string())?;
    let model = fsl_core::build_model(kernel.clone()).map_err(|error| error.to_string())?;
    if matches!(
        fsl_syntax::parse_surface_document(source),
        Ok(fsl_syntax::SurfaceDocument::Requirements(_))
    ) {
        fsl_core::requirements_implements(source, &resolver, &model)
            .map_err(|error| error.to_string())?;
    }
    let mut contract = fsl_core::public_kernel_contract(
        &kernel,
        &model,
        &path.to_string_lossy(),
        source_dialect(source),
    )
    .map_err(|error| error.to_string())?;
    remove_migration_locations(&mut contract);
    remove_legacy_metadata_projection(&mut contract);
    Ok(json!({
        "kernel": contract,
        "annotations": migration_annotation_contract(&model),
    }))
}

fn remove_migration_locations(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for key in ["span", "source_node_id", "source_node_ids", "reverse_index"] {
                object.remove(key);
            }
            object.values_mut().for_each(remove_migration_locations);
        }
        Value::Array(values) => values.iter_mut().for_each(remove_migration_locations),
        _ => {}
    }
}

fn remove_legacy_metadata_projection(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("requirement");
            if let Some(Value::Object(origin)) = object.get_mut("origin") {
                origin.remove("declaration");
            }
            object
                .values_mut()
                .for_each(remove_legacy_metadata_projection);
        }
        Value::Array(values) => values
            .iter_mut()
            .for_each(remove_legacy_metadata_projection),
        _ => {}
    }
}

fn migration_annotation_contract(model: &fsl_core::KernelModel) -> Value {
    let mut targets = vec!["spec".to_owned(), "init".to_owned()];
    targets.extend(
        model
            .actions
            .iter()
            .map(|action| fsl_core::action_target(&action.name)),
    );
    for (kind, properties) in [
        ("invariant", &model.invariants),
        ("trans", &model.transitions),
        ("reachable", &model.reachables),
    ] {
        targets.extend(
            properties
                .iter()
                .map(|property| fsl_core::property_target(kind, &property.name)),
        );
    }
    targets.extend(
        model
            .leadstos
            .iter()
            .map(|property| fsl_core::property_target("leadsTo", &property.name)),
    );
    targets.sort();
    targets.dedup();
    Value::Object(
        targets
            .into_iter()
            .filter_map(|target| {
                let mut annotations = model
                    .annotations_for(&target)
                    .source_order()
                    .iter()
                    .map(fsl_syntax::Annotation::render_source)
                    .collect::<Vec<_>>();
                annotations.sort();
                (!annotations.is_empty()).then(|| (target, json!(annotations)))
            })
            .collect(),
    )
}

fn migration_plan_error_output(source: &str, path: &Path, message: &str) -> Value {
    let mut output = match fsl_syntax::parse_surface_document(source) {
        Err(error) => surface_parse_error_output(&error),
        Ok(_) => semantic_error_output(message),
    };
    let object = output.as_object_mut().expect("error envelope");
    object.insert("file".to_owned(), json!(path));
    if let Some(Value::Object(location)) = object.get_mut("loc") {
        location.insert("file".to_owned(), json!(path));
    }
    output
}

fn atomic_write_migrations(writes: &[(&Path, &str)]) -> Result<(), String> {
    if writes.is_empty() {
        return Ok(());
    }
    let nonce = format!("{}", std::process::id());
    let mut prepared = Vec::new();
    for (index, (path, source)) in writes.iter().enumerate() {
        let metadata = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        if !metadata.file_type().is_file() {
            return Err(format!(
                "migration write target '{}' is not a regular file",
                path.display()
            ));
        }
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let temp = parent.join(format!(".fslc-migrate-{nonce}-{index}.tmp"));
        let backup = parent.join(format!(".fslc-migrate-{nonce}-{index}.bak"));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| error.to_string())?;
        if let Err(error) = file
            .write_all(source.as_bytes())
            .and_then(|()| file.sync_all())
            .and_then(|()| std::fs::set_permissions(&temp, metadata.permissions()))
        {
            let _ = std::fs::remove_file(&temp);
            for (_, temp, _) in &prepared {
                let _ = std::fs::remove_file(temp);
            }
            return Err(error.to_string());
        }
        prepared.push((path.to_path_buf(), temp, backup));
    }
    for (path, _, backup) in &prepared {
        if let Err(error) = std::fs::hard_link(path, backup) {
            for (_, temp, backup) in &prepared {
                let _ = std::fs::remove_file(temp);
                let _ = std::fs::remove_file(backup);
            }
            return Err(error.to_string());
        }
    }
    for (committed, (path, temp, _)) in prepared.iter().enumerate() {
        if let Err(error) = std::fs::rename(temp, path) {
            for (path, _, backup) in prepared.iter().take(committed) {
                let _ = std::fs::rename(backup, path);
            }
            for (_, temp, backup) in &prepared {
                let _ = std::fs::remove_file(temp);
                let _ = std::fs::remove_file(backup);
            }
            return Err(error.to_string());
        }
    }
    for (_, _, backup) in &prepared {
        let _ = std::fs::remove_file(backup);
    }
    Ok(())
}

fn validate_fmt_semantics(source: &str, path: &Path) -> Result<(), String> {
    let dialect = fsl_syntax::dialect_keyword(source).map_err(|error| error.to_string())?;
    if path.as_os_str() == "-" || matches!(dialect, "refinement" | "agent") {
        return Ok(());
    }
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let resolver = fsl_core::FsResolver::new(base);
    let source_file = path.to_string_lossy();
    let kernel = fsl_core::parse_kernel_source_with_file(source, &resolver, source_file)
        .map_err(|error| error.to_string())?;
    fsl_core::build_model(kernel)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn print_cli_metadata() -> bool {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments
        .first()
        .is_some_and(|argument| argument == "--cli-contract")
    {
        print!("{CLI_CONTRACT}");
        return true;
    }
    let Some(help_index) = arguments
        .iter()
        .position(|argument| matches!(argument.as_str(), "-h" | "--help"))
    else {
        return false;
    };
    let contract: Value = serde_json::from_str(CLI_CONTRACT).expect("valid embedded CLI contract");
    let mut node = &contract["root"];
    for segment in &arguments[..help_index] {
        let Some(next) = node["commands"].as_array().and_then(|commands| {
            commands.iter().find(|candidate| {
                candidate["path"]
                    .as_array()
                    .and_then(|path| path.last())
                    .and_then(Value::as_str)
                    == Some(segment)
            })
        }) else {
            if node["commands"]
                .as_array()
                .is_some_and(|commands| !commands.is_empty())
            {
                return false;
            }
            break;
        };
        node = next;
    }
    print!("{}", node["help"].as_str().expect("CLI help is a string"));
    true
}

#[allow(clippy::too_many_lines, clippy::while_let_on_iterator)]
fn command() -> Result<(Value, i32), String> {
    let mut args = std::env::args().skip(1);
    let command = args
        .next()
        .ok_or_else(|| "usage: fslc <check|verify> SPEC [options]".to_owned())?;
    match command.as_str() {
        "version" | "-V" | "--version" => {
            println!("fslc {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        "check" => {
            let display_path = PathBuf::from(
                args.next()
                    .ok_or_else(|| "usage: fslc check SPEC [options]".to_owned())?,
            );
            let literate_guard = materialize_literate(&display_path)?;
            let path = literate_guard
                .as_ref()
                .map_or(&display_path, |state| &state.path);
            let mut strict_tags = false;
            let mut requirements = None;
            let mut edition = "current".to_owned();
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--strict-tags" => strict_tags = true,
                    "--requirements" => {
                        requirements = Some(PathBuf::from(required_option_value(
                            &mut args,
                            "--requirements",
                        )?));
                    }
                    "--edition" => edition = parse_edition(&mut args)?,
                    _ => return Err(format!("unknown check option '{option}'")),
                }
            }
            Ok(with_version_metadata(run_check_with_tags(
                path,
                &display_path,
                strict_tags,
                requirements.as_deref(),
                &edition,
            )))
        }
        "lint" => {
            let options = parse_migration_options(args, false)?;
            Ok(run_lint(&options))
        }
        "migrate" => {
            let options = parse_migration_options(args, true)?;
            Ok(run_migrate(&options))
        }
        "kernel" => {
            let mut path = None;
            let mut version = fsl_core::PublicKernelVersion::V1;
            while let Some(argument) = args.next() {
                match argument.as_str() {
                    "--kernel-version" => {
                        let value = required_option_value(&mut args, "--kernel-version")?;
                        version = match fsl_core::PublicKernelVersion::parse(&value) {
                            Ok(version) => version,
                            Err(error) => {
                                return Ok((semantic_error_output(&error.to_string()), 2));
                            }
                        };
                    }
                    option if option.starts_with('-') => {
                        return Err(format!("unknown kernel option '{option}'"));
                    }
                    value if path.is_none() => path = Some(PathBuf::from(value)),
                    value => return Err(format!("unexpected kernel argument '{value}'")),
                }
            }
            let path =
                path.ok_or_else(|| "usage: fslc kernel SPEC [--kernel-version MAJOR]".to_owned())?;
            Ok(run_kernel_contract(&path, version))
        }
        "conformance" => {
            let mut path = None;
            let mut depth = 4_usize;
            let mut version = fsl_core::PublicKernelVersion::V1;
            while let Some(argument) = args.next() {
                match argument.as_str() {
                    "--depth" => {
                        depth = required_option_value(&mut args, "--depth")?
                            .parse()
                            .map_err(|_| "--depth must be a non-negative integer".to_owned())?;
                    }
                    "--kernel-version" => {
                        let value = required_option_value(&mut args, "--kernel-version")?;
                        version = match fsl_core::PublicKernelVersion::parse(&value) {
                            Ok(version) => version,
                            Err(error) => {
                                return Ok((semantic_error_output(&error.to_string()), 2));
                            }
                        };
                    }
                    option if option.starts_with('-') => {
                        return Err(format!("unknown conformance option '{option}'"));
                    }
                    value if path.is_none() => path = Some(PathBuf::from(value)),
                    value => return Err(format!("unexpected conformance argument '{value}'")),
                }
            }
            let path = path.ok_or_else(|| {
                "usage: fslc conformance SPEC [--depth N] [--kernel-version MAJOR]".to_owned()
            })?;
            Ok(run_conformance(&path, depth, version))
        }
        "approval" => approval_command(args),
        "document" => document_command(args),
        "db" => db_command(args),
        "compat" => {
            if args.next().as_deref() != Some("check") {
                return Err("usage: fslc compat check DBSYSTEM [--include-ai]".to_owned());
            }
            let path = PathBuf::from(
                args.next()
                    .ok_or_else(|| "compat check requires a dbsystem".to_owned())?,
            );
            let include_ai = match args.next() {
                None => false,
                Some(option) if option == "--include-ai" => true,
                Some(option) => return Err(format!("unknown compat check option '{option}'")),
            };
            let (mut result, status) = run_db_check(&path, 8, "warn", "bmc");
            if include_ai && let Value::Object(result) = &mut result {
                result.insert(
                    "compat".to_owned(),
                    json!({"include_ai":true,"source":"dbsystem artifact capability model"}),
                );
            }
            Ok((result, status))
        }
        "ai" => ai_command(args),
        "domain" => domain_command(args),
        "explain" | "mutate" | "typestate" => {
            let path = PathBuf::from(
                args.next()
                    .ok_or_else(|| format!("fslc {command} requires a spec"))?,
            );
            let mut depth = 8_usize;
            let mut readable = false;
            let mut max_mutants = 100_usize;
            let mut by_requirement = false;
            let mut typescript_only = false;
            let mut external_mutants = None;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--depth" => {
                        depth = args
                            .next()
                            .ok_or_else(|| "--depth requires a value".to_owned())?
                            .parse()
                            .map_err(|_| "--depth must be a non-negative integer".to_owned())?;
                    }
                    "--readable" => readable = true,
                    "--max-mutants" => {
                        max_mutants = args
                            .next()
                            .ok_or_else(|| "--max-mutants requires a value".to_owned())?
                            .parse()
                            .map_err(|_| "--max-mutants must be an integer".to_owned())?;
                    }
                    "--by-requirement" => by_requirement = true,
                    "--from" => {
                        external_mutants = Some(PathBuf::from(
                            args.next()
                                .ok_or_else(|| "--from requires a JSONL path".to_owned())?,
                        ));
                    }
                    "--ts" => typescript_only = true,
                    _ => return Err(format!("unknown {command} option '{option}'")),
                }
            }
            let result = match command.as_str() {
                "explain" => run_explain(&path, depth, readable),
                "mutate" => run_mutate(
                    &path,
                    depth,
                    max_mutants,
                    by_requirement,
                    external_mutants.as_deref(),
                ),
                "typestate" => run_typestate(&path),
                _ => unreachable!(),
            };
            if readable && let Some(text) = result.0.get("readable").and_then(Value::as_str) {
                println!("{text}");
                std::process::exit(result.1);
            }
            if typescript_only
                && let Some(entities) = result.0.get("entities").and_then(Value::as_array)
            {
                for entity in entities {
                    if let Some(source) = entity.get("typescript").and_then(Value::as_str) {
                        println!("{source}");
                    }
                }
                std::process::exit(result.1);
            }
            Ok(result)
        }
        "testgen" | "html" | "ledger" => {
            let path = PathBuf::from(
                args.next()
                    .ok_or_else(|| format!("fslc {command} requires a spec"))?,
            );
            let mut depth = 8_usize;
            let mut output = None;
            let mut target = "pytest".to_owned();
            let mut engine = "bmc".to_owned();
            let mut impl_log = None;
            let mut evidence = Vec::new();
            let mut approval_records = Vec::new();
            let mut trust_keys = Vec::new();
            let mut deadlock = if command == "ledger" {
                "ignore".to_owned()
            } else {
                "warn".to_owned()
            };
            let mut strict = false;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--depth" => {
                        depth = args
                            .next()
                            .ok_or_else(|| "--depth requires a value".to_owned())?
                            .parse()
                            .map_err(|_| "--depth must be a non-negative integer".to_owned())?;
                    }
                    "-o" | "--output" => {
                        output = Some(PathBuf::from(
                            args.next()
                                .ok_or_else(|| "--output requires a path".to_owned())?,
                        ));
                    }
                    "--target" => {
                        target = args
                            .next()
                            .ok_or_else(|| "--target requires a value".to_owned())?;
                    }
                    "--engine" => {
                        engine = args
                            .next()
                            .ok_or_else(|| "--engine requires a value".to_owned())?;
                        if !matches!(engine.as_str(), "bmc" | "induction") {
                            return Err("--engine must be bmc or induction".to_owned());
                        }
                    }
                    "--deadlock" => {
                        deadlock = args
                            .next()
                            .ok_or_else(|| "--deadlock requires a value".to_owned())?;
                        if !matches!(deadlock.as_str(), "warn" | "error" | "ignore") {
                            return Err("--deadlock must be warn, error, or ignore".to_owned());
                        }
                    }
                    "--impl-log" if command == "ledger" => {
                        impl_log = Some(PathBuf::from(required_option_value(
                            &mut args,
                            "--impl-log",
                        )?));
                    }
                    "--evidence" if command == "ledger" => evidence.push(PathBuf::from(
                        required_option_value(&mut args, "--evidence")?,
                    )),
                    "--approval" if command == "ledger" => approval_records.push(PathBuf::from(
                        required_option_value(&mut args, "--approval")?,
                    )),
                    "--trust-key" if command == "ledger" => trust_keys.push(PathBuf::from(
                        required_option_value(&mut args, "--trust-key")?,
                    )),
                    "--strict" => strict = true,
                    _ => return Err(format!("unknown {command} option '{option}'")),
                }
            }
            let result = match command.as_str() {
                "testgen" => {
                    run_testgen(&path, depth, &target, &deadlock, strict, output.as_deref())
                }
                "html" => run_html_report(&path, depth, &deadlock, &engine, output.as_deref()),
                "ledger" => run_ledger_report(
                    &LedgerReportRequest {
                        path: &path,
                        depth,
                        deadlock_mode: &deadlock,
                        engine: &engine,
                        impl_log: impl_log.as_deref(),
                        evidence_paths: &evidence,
                        output_path: output.as_deref(),
                    },
                    &approval_records,
                    &trust_keys,
                ),
                _ => unreachable!(),
            };
            if output.is_none()
                && result.0.get("result").and_then(Value::as_str) == Some("generated")
                && let Some(content) = result.0.get("content").and_then(Value::as_str)
            {
                print!("{content}");
                std::process::exit(result.1);
            }
            Ok(result)
        }
        "analyze" => {
            let first_path = PathBuf::from(
                args.next()
                    .ok_or_else(|| "fslc analyze requires a spec".to_owned())?,
            );
            let mut paths = vec![first_path];
            let mut projection = "tsg".to_owned();
            let mut focus = None;
            let mut output_format = "json".to_owned();
            let mut profile = None;
            let mut export_kind = None;
            let mut code = None;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--projection" => {
                        projection = args
                            .next()
                            .ok_or_else(|| "--projection requires a value".to_owned())?;
                    }
                    "--focus" => focus = args.next(),
                    "--format" => {
                        output_format = args
                            .next()
                            .ok_or_else(|| "--format requires a value".to_owned())?;
                    }
                    "--profile" => profile = args.next(),
                    "--export" => export_kind = args.next(),
                    "--code" => {
                        code = Some(PathBuf::from(required_option_value(&mut args, "--code")?));
                    }
                    _ if !option.starts_with('-') => paths.push(PathBuf::from(option)),
                    _ => return Err(format!("unknown analyze option '{option}'")),
                }
            }
            let result = if paths.len() != 1 || paths[0].is_dir() {
                run_analyze_batch(
                    &paths,
                    &projection,
                    focus.as_deref(),
                    &output_format,
                    profile.as_deref(),
                    export_kind.as_deref(),
                    code.as_deref(),
                )
            } else {
                run_analyze(
                    &paths[0],
                    &projection,
                    focus.as_deref(),
                    &output_format,
                    profile.as_deref(),
                    export_kind.as_deref(),
                    code.as_deref(),
                )
            };
            if output_format != "json"
                && result.0.get("result").and_then(Value::as_str) == Some("analyzed")
                && let Some(content) = result.0.get("content").and_then(Value::as_str)
            {
                print!("{content}");
                std::process::exit(result.1);
            }
            Ok(result)
        }
        "diff" => {
            let mut depth = 8_usize;
            let mut git_range = None;
            let mut mapping = None;
            let mut forbid = Vec::new();
            let mut paths = Vec::new();
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--depth" => {
                        depth = args
                            .next()
                            .ok_or_else(|| "--depth requires a value".to_owned())?
                            .parse()
                            .map_err(|_| "--depth must be an integer".to_owned())?;
                    }
                    "--git" => {
                        git_range = Some(required_option_value(&mut args, "--git")?);
                    }
                    "--mapping" => {
                        mapping = Some(PathBuf::from(required_option_value(
                            &mut args,
                            "--mapping",
                        )?));
                    }
                    "--forbid" => {
                        forbid.extend(
                            required_option_value(&mut args, "--forbid")?
                                .split(',')
                                .map(str::trim)
                                .filter(|item| !item.is_empty())
                                .map(str::to_owned),
                        );
                    }
                    _ if !option.starts_with('-') => paths.push(PathBuf::from(option)),
                    _ => return Err(format!("unknown diff option '{option}'")),
                }
            }
            if let Some(range) = git_range {
                if paths.len() > 1 {
                    return Ok((
                        error_output("usage", "--git accepts at most one spec path"),
                        2,
                    ));
                }
                Ok(run_diff_git(
                    &range,
                    paths.first().map(PathBuf::as_path),
                    depth,
                    mapping.as_deref(),
                    &forbid,
                ))
            } else if paths.len() == 2 {
                Ok(run_diff(
                    &paths[0],
                    &paths[1],
                    depth,
                    mapping.as_deref(),
                    &forbid,
                ))
            } else {
                Ok((
                    error_output("usage", "diff requires OLD NEW or --git BASE..HEAD [SPEC]"),
                    2,
                ))
            }
        }
        "chain" => {
            let mut keep_going = false;
            let mut path = PathBuf::from("fsl-project.toml");
            if let Some(first) = args.next() {
                if first == "--keep-going" {
                    keep_going = true;
                } else if first.starts_with('-') {
                    return Err(format!("unknown chain option '{first}'"));
                } else {
                    path = PathBuf::from(first);
                }
            }
            for option in args.by_ref() {
                match option.as_str() {
                    "--keep-going" => keep_going = true,
                    _ => return Err(format!("unknown chain option '{option}'")),
                }
            }
            let result = run_project_chain(&path, keep_going);
            eprintln!("{}", format_chain_table(&result.0));
            Ok(result)
        }
        "refine" => {
            let path = PathBuf::from(
                args.next()
                    .ok_or_else(|| "refine requires IMPL ABS MAPPING".to_owned())?,
            );
            let abstraction = PathBuf::from(
                args.next()
                    .ok_or_else(|| "refine requires IMPL ABS MAPPING".to_owned())?,
            );
            let mapping = PathBuf::from(
                args.next()
                    .ok_or_else(|| "refine requires IMPL ABS MAPPING".to_owned())?,
            );
            let mut depth = 8_usize;
            let mut rest = Vec::new();
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--depth" => {
                        depth = args
                            .next()
                            .ok_or_else(|| "--depth requires a value".to_owned())?
                            .parse()
                            .map_err(|_| "--depth must be a non-negative integer".to_owned())?;
                    }
                    _ if option.starts_with('-') => {
                        return Err(format!("unknown refine option '{option}'"));
                    }
                    _ => rest.push(PathBuf::from(option)),
                }
            }
            if rest.is_empty() {
                Ok(run_refine(&path, &abstraction, &mapping, depth))
            } else if rest.len() % 2 != 0 {
                Ok((
                    error_output(
                        "io",
                        "refine chain must list (abs map) pairs after the first mapping",
                    ),
                    2,
                ))
            } else {
                Ok(run_refine_chain(
                    &path,
                    &abstraction,
                    &mapping,
                    &rest,
                    depth,
                ))
            }
        }
        "replay" => {
            let path = PathBuf::from(
                args.next()
                    .ok_or_else(|| "usage: fslc replay SPEC --trace TRACE.json".to_owned())?,
            );
            let mut trace = None;
            let mut from_log = None;
            let mut mapping = None;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--trace" => {
                        trace = Some(PathBuf::from(required_option_value(&mut args, "--trace")?));
                    }
                    "--from-log" => {
                        from_log = Some(PathBuf::from(required_option_value(
                            &mut args,
                            "--from-log",
                        )?));
                    }
                    "--mapping" => {
                        mapping = Some(PathBuf::from(required_option_value(
                            &mut args,
                            "--mapping",
                        )?));
                    }
                    _ => return Err(format!("unknown replay option '{option}'")),
                }
            }
            if trace.is_some() && from_log.is_some() {
                return Ok((
                    error_output("usage", "--trace and --from-log are mutually exclusive"),
                    2,
                ));
            }
            if let Some(log) = from_log {
                let Some(mapping) = mapping else {
                    return Ok((
                        error_output("usage", "--mapping is required with --from-log"),
                        2,
                    ));
                };
                Ok(run_log_replay(&path, &log, &mapping))
            } else if let Some(trace) = trace {
                if mapping.is_some() {
                    return Ok((
                        error_output("usage", "--mapping is only valid with --from-log"),
                        2,
                    ));
                }
                Ok(run_replay(&path, &trace))
            } else {
                Ok((
                    error_output("usage", "one of --trace or --from-log is required"),
                    2,
                ))
            }
        }
        "sweep" => {
            let path = PathBuf::from(
                args.next()
                    .ok_or_else(|| "usage: fslc sweep SPEC [options]".to_owned())?,
            );
            let mut depth = "0..8".to_owned();
            let mut deadlock = "warn".to_owned();
            let mut engine = "bmc".to_owned();
            let mut k_ind = 1_usize;
            let mut vacuity = "warn".to_owned();
            let mut property = None;
            let mut strict_tags = false;
            let mut requirements = None;
            let mut instances = Vec::new();
            let mut values = Vec::new();
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--depth" => {
                        depth = args
                            .next()
                            .ok_or_else(|| "--depth requires a value".to_owned())?;
                    }
                    "--deadlock" => {
                        deadlock = args
                            .next()
                            .ok_or_else(|| "--deadlock requires a value".to_owned())?;
                        if !matches!(deadlock.as_str(), "warn" | "error" | "ignore") {
                            return Err("--deadlock must be warn, error, or ignore".to_owned());
                        }
                    }
                    "--engine" => {
                        engine = args
                            .next()
                            .ok_or_else(|| "--engine requires a value".to_owned())?;
                        if !matches!(engine.as_str(), "bmc" | "induction") {
                            return Err("--engine must be bmc or induction".to_owned());
                        }
                    }
                    "--k" => {
                        k_ind = args
                            .next()
                            .ok_or_else(|| "--k requires a value".to_owned())?
                            .parse()
                            .map_err(|_| "--k must be a positive integer".to_owned())?;
                        if k_ind == 0 {
                            return Err("--k must be a positive integer".to_owned());
                        }
                    }
                    "--instances" => instances.push(
                        args.next()
                            .ok_or_else(|| "--instances requires a value".to_owned())?,
                    ),
                    "--values" => values.push(
                        args.next()
                            .ok_or_else(|| "--values requires a value".to_owned())?,
                    ),
                    "--property" => {
                        property = Some(required_option_value(&mut args, "--property")?);
                    }
                    "--vacuity" => {
                        vacuity = required_option_value(&mut args, "--vacuity")?;
                        if !matches!(vacuity.as_str(), "warn" | "error" | "ignore") {
                            return Err("--vacuity must be warn, error, or ignore".to_owned());
                        }
                    }
                    "--strict-tags" => strict_tags = true,
                    "--requirements" => {
                        requirements = Some(PathBuf::from(required_option_value(
                            &mut args,
                            "--requirements",
                        )?));
                    }
                    _ => return Err(format!("unknown sweep option '{option}'")),
                }
            }
            Ok(run_sweep(
                &path,
                &depth,
                &deadlock,
                &engine,
                k_ind,
                &vacuity,
                property.as_deref(),
                strict_tags,
                requirements.as_deref(),
                &instances,
                &values,
            ))
        }
        "verify" | "scenarios" => {
            let display_path = PathBuf::from(
                args.next()
                    .ok_or_else(|| format!("usage: fslc {command} SPEC [options]"))?,
            );
            let literate_guard = materialize_literate(&display_path)?;
            let path = literate_guard
                .as_ref()
                .map_or(&display_path, |state| &state.path);
            let options = parse_verify_options(&mut args)?;
            Ok(if command == "verify" {
                let result = run_verify_cli(path, &display_path, &options);
                with_version_metadata(apply_domain_edition(
                    result,
                    path,
                    &display_path,
                    &options.edition,
                ))
            } else {
                if options.engine != "bmc"
                    || options.explicit_budget != DEFAULT_EXPLICIT_BUDGET
                    || options.k_ind != 1
                    || options.vacuity != "warn"
                    || options.property.is_some()
                    || !options.exclude_properties.is_empty()
                    || !options.scope.instances.is_empty()
                    || !options.scope.values.is_empty()
                    || options.strict_tags
                    || options.requirements.is_some()
                    || !options.use_cache
                    || !options.lemmas.is_empty()
                    || options.from_state.is_some()
                    || options.edition != "current"
                {
                    return Err("scenarios accepts only --depth and --deadlock".to_owned());
                }
                run_scenarios(path, options.depth, &options.deadlock)
            })
        }
        _ => Err(format!("unknown command '{command}'")),
    }
}

#[allow(clippy::too_many_lines)]
fn approval_command(mut args: impl Iterator<Item = String>) -> Result<(Value, i32), String> {
    let subcommand = args
        .next()
        .ok_or_else(|| "usage: fslc approval <create|check|diff> SPEC [options]".to_owned())?;
    let path = PathBuf::from(
        args.next()
            .ok_or_else(|| format!("fslc approval {subcommand} requires a spec"))?,
    );
    match subcommand.as_str() {
        "create" => {
            let mut kind = None;
            let mut artifact = None;
            let mut approver = None;
            let mut requirements = Vec::new();
            let mut depth = None;
            let mut deadlock = None;
            let mut engine = None;
            let mut glossary = None;
            let mut evidence_paths = Vec::new();
            let mut output = None;
            let mut signing_key = None;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--kind" => kind = Some(required_option_value(&mut args, "--kind")?),
                    "--artifact" => {
                        artifact = Some(PathBuf::from(required_option_value(
                            &mut args,
                            "--artifact",
                        )?));
                    }
                    "--approver" => {
                        approver = Some(required_option_value(&mut args, "--approver")?);
                    }
                    "--requirement" => {
                        requirements.push(required_option_value(&mut args, "--requirement")?);
                    }
                    "--signing-key" => {
                        signing_key = Some(PathBuf::from(required_option_value(
                            &mut args,
                            "--signing-key",
                        )?));
                    }
                    "--depth" => {
                        depth = Some(
                            required_option_value(&mut args, "--depth")?
                                .parse::<usize>()
                                .map_err(|_| "--depth must be a non-negative integer".to_owned())?,
                        );
                    }
                    "--deadlock" => {
                        let value = required_option_value(&mut args, "--deadlock")?;
                        if !matches!(value.as_str(), "warn" | "error" | "ignore") {
                            return Err("--deadlock must be warn, error, or ignore".to_owned());
                        }
                        deadlock = Some(value);
                    }
                    "--engine" => {
                        let value = required_option_value(&mut args, "--engine")?;
                        if !matches!(value.as_str(), "bmc" | "induction") {
                            return Err("--engine must be bmc or induction".to_owned());
                        }
                        engine = Some(value);
                    }
                    "--glossary" => {
                        glossary = Some(PathBuf::from(required_option_value(
                            &mut args,
                            "--glossary",
                        )?));
                    }
                    "--evidence" => {
                        evidence_paths.push(PathBuf::from(required_option_value(
                            &mut args,
                            "--evidence",
                        )?));
                    }
                    "-o" | "--output" => {
                        output = Some(PathBuf::from(required_option_value(&mut args, "--output")?));
                    }
                    _ => return Err(format!("unknown approval create option '{option}'")),
                }
            }
            let kind = kind.ok_or_else(|| "approval create requires --kind".to_owned())?;
            let artifact =
                artifact.ok_or_else(|| "approval create requires --artifact".to_owned())?;
            let approver =
                approver.ok_or_else(|| "approval create requires --approver".to_owned())?;
            let inputs = if kind == "requirements_document" {
                if depth.is_some() || deadlock.is_some() || engine.is_some() {
                    return Err(
                        "--depth/--deadlock/--engine are not valid with --kind requirements_document"
                            .to_owned(),
                    );
                }
                document_generation_inputs(&artifact, glossary.as_deref(), &evidence_paths)?
            } else {
                if glossary.is_some() || !evidence_paths.is_empty() {
                    return Err(
                        "--glossary/--evidence are only valid with --kind requirements_document"
                            .to_owned(),
                    );
                }
                approval::GenerationInputs::Solver(approval::SolverGenerationInputs {
                    depth: depth.unwrap_or(8),
                    deadlock: deadlock.unwrap_or_else(|| "ignore".to_owned()),
                    engine: engine.unwrap_or_else(|| "bmc".to_owned()),
                })
            };
            Ok(run_approval_create(
                &path,
                &kind,
                &artifact,
                &approver,
                &requirements,
                &inputs,
                output.as_deref(),
                signing_key.as_deref(),
            ))
        }
        "check" => {
            let mut record = None;
            let mut trust_keys = Vec::new();
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--record" => {
                        record = Some(PathBuf::from(required_option_value(&mut args, "--record")?));
                    }
                    "--trust-key" => trust_keys.push(PathBuf::from(required_option_value(
                        &mut args,
                        "--trust-key",
                    )?)),
                    _ => return Err(format!("unknown approval check option '{option}'")),
                }
            }
            Ok(run_approval_check(
                &path,
                &record.ok_or_else(|| "approval check requires --record".to_owned())?,
                &trust_keys,
            ))
        }
        "diff" => {
            let mut record = None;
            let mut depth = 8_usize;
            let mut trust_keys = Vec::new();
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--record" => {
                        record = Some(PathBuf::from(required_option_value(&mut args, "--record")?));
                    }
                    "--depth" => {
                        depth = required_option_value(&mut args, "--depth")?
                            .parse()
                            .map_err(|_| "--depth must be a non-negative integer".to_owned())?;
                    }
                    "--trust-key" => trust_keys.push(PathBuf::from(required_option_value(
                        &mut args,
                        "--trust-key",
                    )?)),
                    _ => return Err(format!("unknown approval diff option '{option}'")),
                }
            }
            Ok(run_approval_diff(
                &path,
                &record.ok_or_else(|| "approval diff requires --record".to_owned())?,
                depth,
                &trust_keys,
            ))
        }
        _ => Err(format!("unknown approval subcommand '{subcommand}'")),
    }
}

#[allow(clippy::too_many_lines)]
fn document_command(mut args: impl Iterator<Item = String>) -> Result<(Value, i32), String> {
    let subcommand = args
        .next()
        .ok_or_else(|| "usage: fslc document <generate|claims> SPEC [options]".to_owned())?;
    let path = PathBuf::from(
        args.next()
            .ok_or_else(|| format!("fslc document {subcommand} requires a spec"))?,
    );
    match subcommand.as_str() {
        "generate" => {
            let mut locale = fsl_tools::Locale::Ja;
            let mut strict = false;
            let mut strict_rendering = false;
            let mut glossary = None;
            let mut evidence_paths = Vec::new();
            let mut approval_paths = Vec::new();
            let mut trust_keys = Vec::new();
            let mut output = None;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--view" => {
                        let value = required_option_value(&mut args, "--view")?;
                        if value != "requirements" {
                            return Err(
                                "--view must be requirements ('business'/'design' are reserved \
                                 until docs/DESIGN-document-dialect-adapters.md's activation \
                                 contract is met, issue #334)"
                                    .to_owned(),
                            );
                        }
                    }
                    "--lang" => {
                        let value = required_option_value(&mut args, "--lang")?;
                        locale = match value.as_str() {
                            "ja" => fsl_tools::Locale::Ja,
                            "en" => fsl_tools::Locale::En,
                            _ => return Err("--lang must be ja or en".to_owned()),
                        };
                    }
                    "--strict" => strict = true,
                    "--strict-rendering" => strict_rendering = true,
                    "--glossary" => {
                        glossary = Some(PathBuf::from(required_option_value(
                            &mut args,
                            "--glossary",
                        )?));
                    }
                    "--evidence" => {
                        evidence_paths.push(PathBuf::from(required_option_value(
                            &mut args,
                            "--evidence",
                        )?));
                    }
                    "--approval" => {
                        approval_paths.push(PathBuf::from(required_option_value(
                            &mut args,
                            "--approval",
                        )?));
                    }
                    "--trust-key" => trust_keys.push(PathBuf::from(required_option_value(
                        &mut args,
                        "--trust-key",
                    )?)),
                    "-o" | "--output" => {
                        output = Some(PathBuf::from(required_option_value(&mut args, "--output")?));
                    }
                    _ => return Err(format!("unknown document generate option '{option}'")),
                }
            }
            let result = run_document_generate(
                &path,
                locale,
                strict,
                strict_rendering,
                glossary.as_deref(),
                &evidence_paths,
                &approval_paths,
                &trust_keys,
                output.as_deref(),
            );
            if output.is_none()
                && result.0.get("result").and_then(Value::as_str) == Some("generated")
                && let Some(content) = result.0.get("content").and_then(Value::as_str)
            {
                // The envelope (and any `warnings`, e.g. FSL-DOC-LABEL-UNKNOWN)
                // never reaches stdout in this no-`-o` bypass, since stdout must
                // stay the raw document; surface warnings on stderr instead of
                // silently dropping them.
                if let Some(warnings) = result.0.get("warnings").and_then(Value::as_array) {
                    for warning in warnings {
                        eprintln!("warning: {warning}");
                    }
                }
                print!("{content}");
                std::process::exit(result.1);
            }
            Ok(result)
        }
        "claims" => {
            let mut output = None;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--view" => {
                        let value = required_option_value(&mut args, "--view")?;
                        if value != "requirements" {
                            return Err(
                                "--view must be requirements ('business'/'design' are reserved \
                                 until docs/DESIGN-document-dialect-adapters.md's activation \
                                 contract is met, issue #334)"
                                    .to_owned(),
                            );
                        }
                    }
                    "-o" | "--output" => {
                        output = Some(PathBuf::from(required_option_value(&mut args, "--output")?));
                    }
                    _ => return Err(format!("unknown document claims option '{option}'")),
                }
            }
            let result = run_document_claims(&path, output.as_deref());
            if output.is_none()
                && result.0.get("result").and_then(Value::as_str) == Some("generated")
                && let Some(content) = result.0.get("content").and_then(Value::as_str)
            {
                print!("{content}");
                std::process::exit(result.1);
            }
            Ok(result)
        }
        "check" => {
            let artifact = PathBuf::from(
                args.next()
                    .ok_or_else(|| "fslc document check requires an artifact path".to_owned())?,
            );
            let mut glossary = None;
            let mut evidence_paths = Vec::new();
            let mut approval_paths = Vec::new();
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--glossary" => {
                        glossary = Some(PathBuf::from(required_option_value(
                            &mut args,
                            "--glossary",
                        )?));
                    }
                    "--evidence" => {
                        evidence_paths.push(PathBuf::from(required_option_value(
                            &mut args,
                            "--evidence",
                        )?));
                    }
                    "--approval" => {
                        approval_paths.push(PathBuf::from(required_option_value(
                            &mut args,
                            "--approval",
                        )?));
                    }
                    _ => return Err(format!("unknown document check option '{option}'")),
                }
            }
            Ok(run_document_check(
                &path,
                &artifact,
                glossary.as_deref(),
                &evidence_paths,
                &approval_paths,
            ))
        }
        _ => Err(format!("unknown document subcommand '{subcommand}'")),
    }
}

/// An unsupported source dialect is a scope boundary (issue #334,
/// `docs/DESIGN-document-dialect-adapters.md`), not a defect in the spec: it
/// gets its own coded `document` envelope so a caller can programmatically
/// distinguish "RCIR has no adapter for this dialect yet" from a genuine
/// parse/semantic error in a supported dialect.
fn document_projection_error_output(error: &fsl_tools::DocumentProjectionError) -> Value {
    match error {
        fsl_tools::DocumentProjectionError::UnsupportedDialect { dialect } => {
            let mut output = error_output("document", &error.to_string());
            output
                .as_object_mut()
                .expect("document error envelope")
                .extend([
                    ("code".to_owned(), json!("FSL-DOC-DIALECT-UNSUPPORTED")),
                    ("dialect".to_owned(), json!(dialect)),
                    (
                        "supported_dialects".to_owned(),
                        json!(fsl_tools::RCIR_SUPPORTED_DIALECTS),
                    ),
                ]);
            output
        }
        fsl_tools::DocumentProjectionError::Other(message) => semantic_error_output(message),
    }
}

fn load_document_claims(path: &Path) -> Result<(String, fsl_tools::RequirementClaimSet), Value> {
    load_document_claims_with_label(path, &path.to_string_lossy())
}

/// Like [`load_document_claims`], but projects under an explicit label
/// rather than `path`'s own spelling. `fslc document check` (issue #329)
/// needs this: every rendered `出典`/`Source:` line carries the label
/// verbatim, so re-projecting under a *different* spelling of the same file
/// (e.g. a relative-path difference from running `check` in another
/// directory than `generate`) would report false drift. `check` re-projects
/// under the label the artifact's own frontmatter recorded instead.
fn load_document_claims_with_label(
    path: &Path,
    label: &str,
) -> Result<(String, fsl_tools::RequirementClaimSet), Value> {
    let source =
        std::fs::read_to_string(path).map_err(|error| error_output("io", &error.to_string()))?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    fsl_tools::project_requirement_claims_from_source(&source, Some(label), base)
        .map(|claims| (source, claims))
        .map_err(|error| document_projection_error_output(&error))
}

fn document_diagnostics_error(
    claims: &fsl_tools::RequirementClaimSet,
    strict: bool,
) -> Option<Value> {
    let (code, message) = if claims.requirements.is_empty() {
        (
            "FSL-DOC-NO-REQUIREMENTS",
            "the specification declares no requirement IDs".to_owned(),
        )
    } else if strict && claims.coverage.counts.unattributed > 0 {
        (
            "FSL-DOC-UNTAGGED-TARGET",
            format!(
                "{} authored target(s) are not linked to a requirement ID",
                claims.coverage.counts.unattributed
            ),
        )
    } else if strict && claims.coverage.counts.unsupported > 0 {
        (
            "FSL-DOC-UNSUPPORTED-TARGET",
            format!(
                "{} authored target(s) use a semantic target RCIR v1 does not project",
                claims.coverage.counts.unsupported
            ),
        )
    } else {
        return None;
    };
    let mut output = error_output("document", &message);
    output
        .as_object_mut()
        .expect("document error envelope")
        .insert("code".to_owned(), json!(code));
    Some(output)
}

fn default_document_output(path: &Path, suffix: &str) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("document");
    PathBuf::from(format!("{stem}{suffix}"))
}

/// Diagnostic code for a glossary file that fails to load, parse, or self-
/// validate (issue #330) — everything except a duplicate label target,
/// which the issue's own diagnostics table gives its own code.
fn document_glossary_error(message: &str) -> Value {
    let mut output = error_output("document", message);
    output
        .as_object_mut()
        .expect("document error envelope")
        .insert("code".to_owned(), json!("FSL-DOC-GLOSSARY-INVALID"));
    output
}

fn document_glossary_issue_error(issues: &[fsl_tools::GlossaryIssue]) -> Value {
    let message = issues
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    if issues
        .iter()
        .any(|issue| matches!(issue, fsl_tools::GlossaryIssue::DuplicateTarget(_)))
    {
        let mut output = error_output("document", &message);
        output
            .as_object_mut()
            .expect("document error envelope")
            .insert("code".to_owned(), json!("FSL-DOC-LABEL-CONFLICT"));
        output
    } else {
        document_glossary_error(&message)
    }
}

/// Load, parse, and validate a `--glossary` sidecar (issue #330) against the
/// locale it will be rendered under. `None` when no `--glossary` was given.
/// Shared by `generate` and `check`: both re-render with whatever glossary
/// (or lack of one) applies, so both need the identical loading rule.
fn load_glossary(
    path: Option<&Path>,
    expected_locale: fsl_tools::Locale,
) -> Result<Option<(fsl_tools::Glossary, String)>, Value> {
    let Some(path) = path else {
        return Ok(None);
    };
    let bytes = std::fs::read(path).map_err(|error| error_output("io", &error.to_string()))?;
    let digest = approval::sha256_bytes(&bytes);
    let text = String::from_utf8(bytes)
        .map_err(|error| error_output("io", &format!("{}: {error}", path.display())))?;
    let glossary = fsl_tools::parse_glossary(&text)
        .map_err(|issues| document_glossary_issue_error(&issues))?;
    if glossary.locale != expected_locale {
        return Err(document_glossary_error(&format!(
            "glossary locale '{}' does not match the document locale '{}'",
            glossary.locale.as_str(),
            expected_locale.as_str()
        )));
    }
    Ok(Some((glossary, digest)))
}

fn document_evidence_error(message: &str) -> Value {
    let mut output = error_output("document", message);
    output
        .as_object_mut()
        .expect("document error envelope")
        .insert("code".to_owned(), json!("FSL-DOC-EVIDENCE-INVALID"));
    output
}

type EvidenceFiles = Vec<(String, Value)>;

/// Load and parse every `--evidence` file (issue #332) — each must be a JSON
/// object envelope in the same shape `fslc ledger --evidence` already
/// accepts; this module classifies nothing itself (`fsl_tools::ledger`'s
/// `assurance_token`/`assurance_label` remain the sole classifier). Returns
/// `None` when no `--evidence` flags were given. The combined digest sorts
/// each file's own digest before hashing them together, so the same file
/// *set* given in a different `--evidence` order still yields the same
/// `evidence_digest` frontmatter value.
fn load_evidence(paths: &[PathBuf]) -> Result<Option<(EvidenceFiles, String)>, Value> {
    if paths.is_empty() {
        return Ok(None);
    }
    let mut files = Vec::new();
    let mut digests = Vec::new();
    for path in paths {
        let bytes = std::fs::read(path).map_err(|error| error_output("io", &error.to_string()))?;
        digests.push(approval::sha256_bytes(&bytes));
        let text = String::from_utf8(bytes)
            .map_err(|error| error_output("io", &format!("{}: {error}", path.display())))?;
        let value = serde_json::from_str::<Value>(&text).map_err(|error| {
            document_evidence_error(&format!("{}: invalid JSON: {error}", path.display()))
        })?;
        if !value.is_object() {
            return Err(document_evidence_error(&format!(
                "{}: evidence JSON must contain an object envelope",
                path.display()
            )));
        }
        files.push((path.display().to_string(), value));
    }
    digests.sort();
    let combined = approval::sha256_bytes(digests.join("\n").as_bytes());
    Ok(Some((files, combined)))
}

fn document_approval_error(message: &str) -> Value {
    let mut output = error_output("document", message);
    output
        .as_object_mut()
        .expect("document error envelope")
        .insert("code".to_owned(), json!("FSL-DOC-APPROVAL-INVALID"));
    output
}

fn document_approval_drifted_error(reasons: &[&'static str]) -> Value {
    let mut output = error_output(
        "document",
        "a supplied --approval record does not match the current rendering",
    );
    output
        .as_object_mut()
        .expect("document error envelope")
        .extend([
            ("code".to_owned(), json!("FSL-DOC-APPROVAL-DRIFTED")),
            ("reasons".to_owned(), json!(reasons)),
        ]);
    output
}

/// A `--approval` record loaded and (for a signed schema) verified for
/// `fslc document generate`'s overlay (issue #333), retaining the full
/// parsed record so the caller can compare its `spec`/`target` digests
/// against the current rendering before ever displaying it.
struct LoadedApproval {
    record: approval::ApprovalRecord,
    record_path: String,
    signature_key_id: Option<String>,
    current_reviewed_digest: Option<String>,
}

/// Load and verify every `--approval` record. Each must target
/// `requirements_document`; a signed (v2/v4) record must verify against
/// `--trust-key` or this fails closed — a stakeholder document must never
/// display an unverifiable signed approval. Returns `None` when no
/// `--approval` flags were given. The combined digest sorts each record
/// file's own digest before hashing them together, the same order-
/// independent scheme `load_evidence`/`load_glossary` already use.
fn load_approvals(
    paths: &[PathBuf],
    trust_keys: &[PathBuf],
) -> Result<Option<(Vec<LoadedApproval>, String)>, Value> {
    if paths.is_empty() {
        return Ok(None);
    }
    let trust =
        approval::TrustStore::load(trust_keys).map_err(|error| error_output("io", &error))?;
    let mut loaded = Vec::new();
    let mut digests = Vec::new();
    for path in paths {
        let bytes = std::fs::read(path).map_err(|error| error_output("io", &error.to_string()))?;
        digests.push(approval::sha256_bytes(&bytes));
        let versioned = approval::read_versioned_record(path)
            .map_err(|error| document_approval_error(&format!("{}: {error}", path.display())))?;
        let (record, signature_key_id) = match &versioned {
            approval::VersionedApprovalRecord::V1(record) => (record.clone(), None),
            approval::VersionedApprovalRecord::V2(record) => {
                let verified = trust
                    .verify(record)
                    .map_err(|error| error_output("io", &error))?;
                if !verified {
                    return Err(document_approval_error(&format!(
                        "{}: signature is invalid or untrusted",
                        path.display()
                    )));
                }
                (versioned.binding(), Some(record.signature.key_id.clone()))
            }
        };
        if record.target.kind != "requirements_document" {
            return Err(document_approval_error(&format!(
                "{}: approval record targets '{}', not requirements_document",
                path.display(),
                record.target.kind
            )));
        }
        let current_reviewed_digest = approval::reviewed_artifact_digest(&record)
            .map_err(|error| error_output("io", &error))?;
        loaded.push(LoadedApproval {
            record,
            record_path: path.display().to_string(),
            signature_key_id,
            current_reviewed_digest,
        });
    }
    digests.sort();
    let combined = approval::sha256_bytes(digests.join("\n").as_bytes());
    Ok(Some((loaded, combined)))
}

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn run_document_generate(
    path: &Path,
    locale: fsl_tools::Locale,
    strict: bool,
    strict_rendering: bool,
    glossary_path: Option<&Path>,
    evidence_paths: &[PathBuf],
    approval_paths: &[PathBuf],
    trust_keys: &[PathBuf],
    output_path: Option<&Path>,
) -> (Value, i32) {
    let (source, claims) = match load_document_claims(path) {
        Ok(parts) => parts,
        Err(output) => return (output, 2),
    };
    if let Some(error) = document_diagnostics_error(&claims, strict) {
        return (error, 2);
    }
    let loaded_glossary = match load_glossary(glossary_path, locale) {
        Ok(loaded) => loaded,
        Err(output) => return (output, 2),
    };
    let loaded_evidence = match load_evidence(evidence_paths) {
        Ok(loaded) => loaded,
        Err(output) => return (output, 2),
    };
    let mut evidence_warnings = Vec::new();
    if let Some((files, _)) = &loaded_evidence {
        let requirement_ids: std::collections::BTreeSet<&str> = claims
            .requirements
            .iter()
            .map(|requirement| requirement.id.as_str())
            .collect();
        let unmatched = fsl_tools::unmatched_evidence_paths(&requirement_ids, files);
        if !unmatched.is_empty() {
            if strict {
                let message = format!(
                    "{} evidence file(s) do not match any requirement ID in this specification: {}",
                    unmatched.len(),
                    unmatched.join(", ")
                );
                let mut output = error_output("document", &message);
                output
                    .as_object_mut()
                    .expect("document error envelope")
                    .insert("code".to_owned(), json!("FSL-DOC-EVIDENCE-UNMATCHED"));
                return (output, 2);
            }
            evidence_warnings = unmatched;
        }
    }
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let resolver = fsl_core::FsResolver::new(base);
    let kernel = match fsl_core::parse_kernel_source(&source, &resolver) {
        Ok(kernel) => kernel,
        Err(error) => return (semantic_error_output(&error.to_string()), 2),
    };
    let model = match fsl_core::build_model(kernel.clone()) {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error.to_string()), 2),
    };
    let mut label_warnings = Vec::new();
    if let Some((glossary, _)) = &loaded_glossary {
        let unknown = fsl_tools::unknown_targets(glossary, &model);
        if !unknown.is_empty() {
            if strict {
                let message = format!(
                    "{} glossary target(s) do not exist: {}",
                    unknown.len(),
                    unknown
                        .iter()
                        .map(|target| target.target.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                let mut output = error_output("document", &message);
                output
                    .as_object_mut()
                    .expect("document error envelope")
                    .insert("code".to_owned(), json!("FSL-DOC-LABEL-UNKNOWN"));
                return (output, 2);
            }
            label_warnings = unknown;
        }
    }
    let applied_glossary = loaded_glossary
        .as_ref()
        .map(|(glossary, digest)| fsl_tools::AppliedGlossary { glossary, digest });
    let applied_evidence = loaded_evidence
        .as_ref()
        .map(|(files, digest)| fsl_tools::AppliedEvidence { files, digest });
    let rendered = match fsl_tools::render_requirements_document(
        &claims,
        &kernel,
        &model,
        &source,
        locale,
        applied_glossary.as_ref(),
        applied_evidence.as_ref(),
        None,
    ) {
        Ok(rendered) => rendered,
        Err(error) => return (error_output("document", &error), 2),
    };
    if strict_rendering && rendered.formula_fallback_count > 0 {
        let mut output = error_output(
            "document",
            &format!(
                "{} expression(s) fell back to canonical FSL text under --strict-rendering",
                rendered.formula_fallback_count
            ),
        );
        output
            .as_object_mut()
            .expect("document error envelope")
            .insert("code".to_owned(), json!("FSL-DOC-FORMULA-FALLBACK"));
        return (output, 2);
    }
    let pre_approval_digest = approval::sha256_bytes(rendered.markdown.as_bytes());

    let loaded_approvals = match load_approvals(approval_paths, trust_keys) {
        Ok(loaded) => loaded,
        Err(output) => return (output, 2),
    };
    let mut applied_approvals_vec = Vec::new();
    if let Some((loaded, _)) = &loaded_approvals {
        let mut mismatched: Vec<&'static str> = Vec::new();
        for approval in loaded {
            if approval.record.spec.digest != claims.spec.spec_digest {
                mismatched.push("spec_changed");
            }
            if approval.record.target.digest != pre_approval_digest {
                mismatched.push("rendering_changed");
            }
            if approval.record.target.claim_set_digest.as_deref()
                != Some(claims.spec.claim_set_digest.as_str())
            {
                mismatched.push("claim_set_changed");
            }
            if approval.record.target.reviewed_digest.as_deref()
                != approval.current_reviewed_digest.as_deref()
            {
                mismatched.push("artifact_changed");
            }
        }
        mismatched.sort_unstable();
        mismatched.dedup();
        if !mismatched.is_empty() {
            return (document_approval_drifted_error(&mismatched), 2);
        }
        applied_approvals_vec = loaded
            .iter()
            .map(|approval| fsl_tools::AppliedApproval {
                record_path: approval.record_path.clone(),
                approver: approval.record.approval.approver.clone(),
                approved_at: approval.record.approval.approved_at.clone(),
                requirements: approval.record.approval.requirements.clone(),
                artifact_digest: approval
                    .record
                    .target
                    .reviewed_digest
                    .clone()
                    .expect("validated requirements_document approval"),
                signature_key_id: approval.signature_key_id.clone(),
            })
            .collect();
    }
    let rendered = if let Some((_, combined_digest)) = &loaded_approvals {
        let applied_approvals = fsl_tools::AppliedApprovals {
            records: &applied_approvals_vec,
            digest: combined_digest,
        };
        match fsl_tools::render_requirements_document(
            &claims,
            &kernel,
            &model,
            &source,
            locale,
            applied_glossary.as_ref(),
            applied_evidence.as_ref(),
            Some(&applied_approvals),
        ) {
            Ok(rendered) => rendered,
            Err(error) => return (error_output("document", &error), 2),
        }
    } else {
        rendered
    };
    let artifact_digest = approval::sha256_bytes(rendered.markdown.as_bytes());
    if let Some(path) = output_path
        && let Err(error) = std::fs::write(path, &rendered.markdown)
    {
        return (error_output("io", &error.to_string()), 2);
    }
    let mut output = envelope();
    output.insert("result".to_owned(), json!("generated"));
    output.insert("kind".to_owned(), json!("requirements_document"));
    output.insert(
        "output".to_owned(),
        json!(output_path.map_or_else(
            || default_document_output(path, "_requirements.md"),
            Path::to_path_buf
        )),
    );
    output.insert("spec_digest".to_owned(), json!(claims.spec.spec_digest));
    output.insert(
        "claim_set_digest".to_owned(),
        json!(claims.spec.claim_set_digest),
    );
    output.insert("artifact_digest".to_owned(), json!(artifact_digest));
    output.insert(
        "coverage".to_owned(),
        json!({
            "authored_targets": claims.coverage.counts.authored,
            "rendered_targets": claims.coverage.counts.rendered,
            "unattributed_targets": claims.coverage.counts.unattributed,
            "unsupported_targets": claims.coverage.counts.unsupported,
            "formula_fallbacks": rendered.formula_fallback_count,
        }),
    );
    output.insert(
        "provenance".to_owned(),
        json!({"completeness": claims.provenance.completeness}),
    );
    if let Some((_, digest)) = &loaded_glossary {
        output.insert(
            "glossary".to_owned(),
            json!({
                "digest": digest,
                "labels": loaded_glossary.as_ref().map_or(0, |(glossary, _)| glossary.labels.len()),
            }),
        );
    }
    if let Some((files, digest)) = &loaded_evidence {
        output.insert(
            "evidence".to_owned(),
            json!({
                "digest": digest,
                "files": files.len(),
            }),
        );
    }
    if let Some((_, digest)) = &loaded_approvals {
        output.insert(
            "approvals".to_owned(),
            json!({
                "digest": digest,
                "records": applied_approvals_vec.len(),
            }),
        );
    }
    let mut warnings = Vec::new();
    warnings.extend(label_warnings.iter().map(|target| {
        json!({
            "kind": "label_unknown",
            "code": "FSL-DOC-LABEL-UNKNOWN",
            "target": target.target,
            "detail": target.detail,
        })
    }));
    warnings.extend(evidence_warnings.iter().map(|path| {
        json!({
            "kind": "evidence_unmatched",
            "code": "FSL-DOC-EVIDENCE-UNMATCHED",
            "target": path,
        })
    }));
    if !warnings.is_empty() {
        output.insert("warnings".to_owned(), json!(warnings));
    }
    if output_path.is_none() {
        output.insert("content".to_owned(), json!(rendered.markdown));
    }
    (Value::Object(output), 0)
}

fn run_document_claims(path: &Path, output_path: Option<&Path>) -> (Value, i32) {
    let (_, claims) = match load_document_claims(path) {
        Ok(parts) => parts,
        Err(output) => return (output, 2),
    };
    let content = match serde_json::to_string_pretty(&claims) {
        Ok(content) => content,
        Err(error) => return (error_output("internal", &error.to_string()), 2),
    };
    if let Some(path) = output_path
        && let Err(error) = std::fs::write(path, &content)
    {
        return (error_output("io", &error.to_string()), 2);
    }
    let mut output = envelope();
    output.insert("result".to_owned(), json!("generated"));
    output.insert("kind".to_owned(), json!("requirement_claims"));
    output.insert(
        "output".to_owned(),
        json!(output_path.map_or_else(
            || default_document_output(path, ".claims.json"),
            Path::to_path_buf
        )),
    );
    output.insert("spec_digest".to_owned(), json!(claims.spec.spec_digest));
    output.insert(
        "claim_set_digest".to_owned(),
        json!(claims.spec.claim_set_digest),
    );
    if output_path.is_none() {
        output.insert("content".to_owned(), json!(content));
    }
    (Value::Object(output), 0)
}

fn document_schema_error(message: &str) -> Value {
    let mut output = error_output("document", message);
    output
        .as_object_mut()
        .expect("document error envelope")
        .insert("code".to_owned(), json!("FSL-DOC-SCHEMA-UNSUPPORTED"));
    output
}

/// Load every `--approval` record for `fslc document check` (issue #333),
/// which reproduces the "Approval records" section's *text* for structural
/// comparison only — unlike `document generate`, it never verifies a
/// signature (admission was `generate`'s job); it only needs the plaintext
/// display fields to render byte-identically.
fn load_approvals_for_check(
    paths: &[PathBuf],
) -> Result<Option<(Vec<fsl_tools::AppliedApproval>, String)>, Value> {
    if paths.is_empty() {
        return Ok(None);
    }
    let mut applied = Vec::new();
    let mut digests = Vec::new();
    for path in paths {
        let bytes = std::fs::read(path).map_err(|error| error_output("io", &error.to_string()))?;
        digests.push(approval::sha256_bytes(&bytes));
        let versioned = approval::read_versioned_record(path)
            .map_err(|error| document_approval_error(&format!("{}: {error}", path.display())))?;
        let (record, signature_key_id) = match &versioned {
            approval::VersionedApprovalRecord::V1(record) => (record.clone(), None),
            approval::VersionedApprovalRecord::V2(record) => {
                (versioned.binding(), Some(record.signature.key_id.clone()))
            }
        };
        if record.target.kind != "requirements_document" {
            return Err(document_approval_error(&format!(
                "{}: approval record targets '{}', not requirements_document",
                path.display(),
                record.target.kind
            )));
        }
        applied.push(fsl_tools::AppliedApproval {
            record_path: path.display().to_string(),
            approver: record.approval.approver,
            approved_at: record.approval.approved_at,
            requirements: record.approval.requirements,
            artifact_digest: record.target.digest,
            signature_key_id,
        });
    }
    digests.sort();
    let combined = approval::sha256_bytes(digests.join("\n").as_bytes());
    Ok(Some((applied, combined)))
}

/// `fslc document check` (issue #329): a purely structural drift check
/// between `artifact` (a possibly hand-edited `fslc document generate`
/// output) and a fresh re-projection + re-render of `spec_path`, under the
/// locale and source label the artifact's own frontmatter recorded.
#[allow(clippy::too_many_lines)]
fn run_document_check(
    spec_path: &Path,
    artifact: &Path,
    glossary_path: Option<&Path>,
    evidence_paths: &[PathBuf],
    approval_paths: &[PathBuf],
) -> (Value, i32) {
    let artifact_text = match std::fs::read_to_string(artifact) {
        Ok(text) => text,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    let frontmatter = match fsl_tools::parse_frontmatter_only(&artifact_text) {
        Ok(frontmatter) => frontmatter,
        Err(issue) => return (document_schema_error(&issue.to_string()), 2),
    };
    if frontmatter.schema != fsl_tools::DOCUMENT_SCHEMA {
        return (
            document_schema_error(&format!(
                "unsupported fsl_document_schema '{}' (expected '{}')",
                frontmatter.schema,
                fsl_tools::DOCUMENT_SCHEMA
            )),
            2,
        );
    }
    if frontmatter.view != "requirements" {
        return (
            document_schema_error(&format!("unsupported document view '{}'", frontmatter.view)),
            2,
        );
    }
    let Some(locale) = fsl_tools::Locale::parse(&frontmatter.lang) else {
        return (
            document_schema_error(&format!("unsupported document lang '{}'", frontmatter.lang)),
            2,
        );
    };

    let spec_label = frontmatter
        .source
        .clone()
        .unwrap_or_else(|| spec_path.to_string_lossy().into_owned());
    let (source, claims) = match load_document_claims_with_label(spec_path, &spec_label) {
        Ok(parts) => parts,
        Err(output) => return (output, 2),
    };
    let base = spec_path.parent().unwrap_or_else(|| Path::new("."));
    let resolver = fsl_core::FsResolver::new(base);
    let kernel = match fsl_core::parse_kernel_source(&source, &resolver) {
        Ok(kernel) => kernel,
        Err(error) => return (semantic_error_output(&error.to_string()), 2),
    };
    let model = match fsl_core::build_model(kernel.clone()) {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error.to_string()), 2),
    };
    let loaded_glossary = match load_glossary(glossary_path, locale) {
        Ok(loaded) => loaded,
        Err(output) => return (output, 2),
    };
    let loaded_evidence = match load_evidence(evidence_paths) {
        Ok(loaded) => loaded,
        Err(output) => return (output, 2),
    };
    let loaded_approvals = match load_approvals_for_check(approval_paths) {
        Ok(loaded) => loaded,
        Err(output) => return (output, 2),
    };
    let applied_glossary = loaded_glossary
        .as_ref()
        .map(|(glossary, digest)| fsl_tools::AppliedGlossary { glossary, digest });
    let applied_evidence = loaded_evidence
        .as_ref()
        .map(|(files, digest)| fsl_tools::AppliedEvidence { files, digest });
    let applied_approvals = loaded_approvals
        .as_ref()
        .map(|(records, digest)| fsl_tools::AppliedApprovals { records, digest });
    let rendered = match fsl_tools::render_requirements_document(
        &claims,
        &kernel,
        &model,
        &source,
        locale,
        applied_glossary.as_ref(),
        applied_evidence.as_ref(),
        applied_approvals.as_ref(),
    ) {
        Ok(rendered) => rendered,
        Err(error) => return (error_output("document", &error), 2),
    };

    let report =
        match fsl_tools::check_requirements_document(&artifact_text, &claims, &rendered.markdown) {
            Ok(report) => report,
            Err(error) => return (document_schema_error(&error.to_string()), 2),
        };

    let mut output = envelope();
    output.insert("artifact".to_owned(), json!(artifact));
    output.insert("spec_digest".to_owned(), json!(claims.spec.spec_digest));
    output.insert(
        "claim_set_digest".to_owned(),
        json!(claims.spec.claim_set_digest),
    );
    if report.is_conformant() {
        output.insert("result".to_owned(), json!("document_conformant"));
        (Value::Object(output), 0)
    } else {
        output.insert("result".to_owned(), json!("document_drifted"));
        output.insert(
            "reasons".to_owned(),
            serde_json::to_value(&report.reasons).expect("serialize drift reasons"),
        );
        (Value::Object(output), 1)
    }
}

fn db_command(mut args: impl Iterator<Item = String>) -> Result<(Value, i32), String> {
    let subcommand = args
        .next()
        .ok_or_else(|| "usage: fslc db <check|observe|import> ...".to_owned())?;
    match subcommand.as_str() {
        "check" => {
            let path = PathBuf::from(args.next().ok_or_else(|| {
                "usage: fslc db check SPEC [--depth N] [--engine ENGINE]".to_owned()
            })?);
            let mut depth = 8_usize;
            let mut deadlock = "warn".to_owned();
            let mut engine = "bmc".to_owned();
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--depth" => {
                        depth = args
                            .next()
                            .ok_or_else(|| "--depth requires a value".to_owned())?
                            .parse()
                            .map_err(|_| "--depth must be a non-negative integer".to_owned())?;
                    }
                    "--deadlock" => {
                        deadlock = args
                            .next()
                            .ok_or_else(|| "--deadlock requires a value".to_owned())?;
                    }
                    "--engine" => {
                        engine = args
                            .next()
                            .ok_or_else(|| "--engine requires a value".to_owned())?;
                        if !matches!(engine.as_str(), "bmc" | "induction") {
                            return Err("--engine must be bmc or induction".to_owned());
                        }
                    }
                    _ => return Err(format!("unknown db check option '{option}'")),
                }
            }
            Ok(run_db_check(&path, depth, &deadlock, &engine))
        }
        "observe" => {
            let path = PathBuf::from(
                args.next()
                    .ok_or_else(|| "usage: fslc db observe SPEC --trace EVENTS.json".to_owned())?,
            );
            if args.next().as_deref() != Some("--trace") {
                return Err("db observe requires --trace EVENTS.json".to_owned());
            }
            let trace = PathBuf::from(
                args.next()
                    .ok_or_else(|| "--trace requires a path".to_owned())?,
            );
            if args.next().is_some() {
                return Err("unexpected db observe argument".to_owned());
            }
            Ok(run_db_observe(&path, &trace))
        }
        "import" => {
            let path = PathBuf::from(args.next().ok_or_else(|| {
                "usage: fslc db import SOURCE [--name NAME] [--source FORMAT] [-o PATH]".to_owned()
            })?);
            let mut name = "ImportedDb".to_owned();
            let mut format = "auto".to_owned();
            let mut output = None;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--name" => {
                        name = args
                            .next()
                            .ok_or_else(|| "--name requires a value".to_owned())?;
                    }
                    "--source" => {
                        format = args
                            .next()
                            .ok_or_else(|| "--source requires a value".to_owned())?;
                    }
                    "-o" | "--output" => {
                        output = Some(PathBuf::from(
                            args.next()
                                .ok_or_else(|| "--output requires a path".to_owned())?,
                        ));
                    }
                    _ => return Err(format!("unknown db import option '{option}'")),
                }
            }
            let result = run_db_import(&path, &name, &format, output.as_deref());
            if output.is_none()
                && result.0.get("result").and_then(Value::as_str) == Some("imported")
                && let Some(source) = result.0.get("dbsystem_source").and_then(Value::as_str)
            {
                print!("{source}");
                std::process::exit(0);
            }
            Ok(result)
        }
        _ => Err(format!("unknown db subcommand '{subcommand}'")),
    }
}

#[allow(clippy::too_many_lines)]
fn ai_command(mut args: impl Iterator<Item = String>) -> Result<(Value, i32), String> {
    let subcommand = args.next().ok_or_else(|| {
        "usage: fslc ai <check|replay|eval|regress|compare|drift|compat> ...".to_owned()
    })?;
    if subcommand == "compare" {
        let mut before = None;
        let mut after = None;
        let mut dataset = None;
        let mut from_label = None;
        let mut to_label = None;
        while let Some(option) = args.next() {
            let value = args
                .next()
                .ok_or_else(|| format!("{option} requires a value"))?;
            match option.as_str() {
                "--from" => before = Some(PathBuf::from(value)),
                "--to" => after = Some(PathBuf::from(value)),
                "--dataset" => dataset = Some(value),
                "--from-label" => from_label = Some(value),
                "--to-label" => to_label = Some(value),
                _ => return Err(format!("unknown ai compare option '{option}'")),
            }
        }
        return Ok(run_ai_compare(
            &before.ok_or_else(|| "ai compare requires --from".to_owned())?,
            &after.ok_or_else(|| "ai compare requires --to".to_owned())?,
            dataset.as_deref(),
            from_label.as_deref(),
            to_label.as_deref(),
        ));
    }
    let path = PathBuf::from(
        args.next()
            .ok_or_else(|| format!("fslc ai {subcommand} requires a file"))?,
    );
    match subcommand.as_str() {
        "check" => {
            let (depth, deadlock, engine, _) = parse_specialized_verify_options(&mut args, false)?;
            Ok(run_ai_check(&path, depth, &deadlock, &engine))
        }
        "replay" => {
            let mut logs = None;
            let mut component = None;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--logs" => {
                        logs = Some(PathBuf::from(required_option_value(&mut args, "--logs")?));
                    }
                    "--component" => {
                        component = Some(required_option_value(&mut args, "--component")?);
                    }
                    _ => return Err(format!("unknown ai replay option '{option}'")),
                }
            }
            Ok(run_ai_replay(
                &path,
                &logs.ok_or_else(|| "ai replay requires --logs EVENTS.jsonl".to_owned())?,
                component.as_deref(),
            ))
        }
        "eval" => {
            let mut records = None;
            let mut dataset = None;
            let mut property = None;
            let mut slice = None;
            while let Some(option) = args.next() {
                let value = args
                    .next()
                    .ok_or_else(|| format!("{option} requires a value"))?;
                match option.as_str() {
                    "--records" => records = Some(PathBuf::from(value)),
                    "--dataset" => dataset = Some(value),
                    "--property" => property = Some(value),
                    "--slice" => slice = Some(value),
                    _ => return Err(format!("unknown ai eval option '{option}'")),
                }
            }
            Ok(run_ai_eval(
                &path,
                records.as_deref(),
                dataset.as_deref(),
                property.as_deref(),
                slice.as_deref(),
            ))
        }
        "regress" => {
            let mut before = None;
            let mut after = None;
            let mut dataset = None;
            let mut migration = None;
            while let Some(option) = args.next() {
                let value = args
                    .next()
                    .ok_or_else(|| format!("{option} requires a value"))?;
                match option.as_str() {
                    "--before-records" => before = Some(PathBuf::from(value)),
                    "--after-records" => after = Some(PathBuf::from(value)),
                    "--dataset" => dataset = Some(value),
                    "--migration" => migration = Some(value),
                    _ => return Err(format!("unknown ai regress option '{option}'")),
                }
            }
            Ok(run_ai_regress(
                &path,
                &before.ok_or_else(|| "ai regress requires --before-records".to_owned())?,
                &after.ok_or_else(|| "ai regress requires --after-records".to_owned())?,
                dataset.as_deref(),
                migration.as_deref(),
            ))
        }
        "drift" => {
            let mut logs = None;
            let mut baseline = None;
            let mut property = None;
            let mut window = None;
            let mut baseline_label = None;
            while let Some(option) = args.next() {
                let value = args
                    .next()
                    .ok_or_else(|| format!("{option} requires a value"))?;
                match option.as_str() {
                    "--logs" => logs = Some(PathBuf::from(value)),
                    "--baseline-logs" => baseline = Some(PathBuf::from(value)),
                    "--property" => property = Some(value),
                    "--window" => window = Some(value),
                    "--baseline" => baseline_label = Some(value),
                    _ => return Err(format!("unknown ai drift option '{option}'")),
                }
            }
            Ok(run_ai_drift(
                &path,
                &logs.ok_or_else(|| "ai drift requires --logs".to_owned())?,
                baseline.as_deref(),
                property.as_deref(),
                window.as_deref(),
                baseline_label.as_deref(),
            ))
        }
        "compat" => {
            let environment = match args.next() {
                None => None,
                Some(option) if option == "--environment" => Some(
                    args.next()
                        .ok_or_else(|| "--environment requires a value".to_owned())?,
                ),
                Some(option) => return Err(format!("unknown ai compat option '{option}'")),
            };
            Ok(run_ai_compat(&path, environment.as_deref()))
        }
        _ => Err(format!("unknown ai subcommand '{subcommand}'")),
    }
}

#[allow(clippy::too_many_lines)]
fn domain_command(mut args: impl Iterator<Item = String>) -> Result<(Value, i32), String> {
    let subcommand = args
        .next()
        .ok_or_else(|| "usage: fslc domain <check|analyze|expand|generate> ...".to_owned())?;
    let path = PathBuf::from(
        args.next()
            .ok_or_else(|| format!("fslc domain {subcommand} requires a file"))?,
    );
    match subcommand.as_str() {
        "check" => {
            let (depth, deadlock, engine, edition) =
                parse_specialized_verify_options(&mut args, true)?;
            Ok(run_domain_check(&path, depth, &deadlock, &engine, &edition))
        }
        "analyze" => Ok(run_domain_analyze(&path)),
        "expand" => {
            let output = parse_optional_output(&mut args)?;
            let result = run_domain_expand(&path, output.as_deref());
            if output.is_none()
                && let Some(source) = result.0.get("kernel_source").and_then(Value::as_str)
            {
                print!("{source}");
                std::process::exit(0);
            }
            Ok(result)
        }
        "generate" => {
            let mut target = "typescript".to_owned();
            let mut profile = "functional-ddd".to_owned();
            let mut output = None;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--profile" => {
                        profile = args
                            .next()
                            .ok_or_else(|| "--profile requires a value".to_owned())?;
                        if profile != "functional-ddd" {
                            return Err("--profile must be functional-ddd".to_owned());
                        }
                    }
                    "--target" => {
                        target = args
                            .next()
                            .ok_or_else(|| "--target requires a value".to_owned())?;
                    }
                    "-o" | "--output" => {
                        output = Some(PathBuf::from(
                            args.next()
                                .ok_or_else(|| "--output requires a path".to_owned())?,
                        ));
                    }
                    _ => return Err(format!("unknown domain generate option '{option}'")),
                }
            }
            Ok(run_domain_generate(
                &path,
                &profile,
                &target,
                output.as_deref(),
            ))
        }
        "replay" => {
            if args.next().as_deref() != Some("--logs") {
                return Err("domain replay requires --logs EVENTS.jsonl".to_owned());
            }
            let logs = PathBuf::from(
                args.next()
                    .ok_or_else(|| "--logs requires a path".to_owned())?,
            );
            Ok(run_domain_replay(&path, &logs))
        }
        "testgen" => {
            let mut depth = 8_usize;
            let mut target = "vitest".to_owned();
            let mut deadlock = "warn".to_owned();
            let mut strict = false;
            let mut output = None;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--depth" => {
                        depth = args
                            .next()
                            .ok_or_else(|| "--depth requires a value".to_owned())?
                            .parse()
                            .map_err(|_| "--depth must be an integer".to_owned())?;
                    }
                    "--target" => {
                        target = args
                            .next()
                            .ok_or_else(|| "--target requires a value".to_owned())?;
                    }
                    "--deadlock" => {
                        deadlock = args
                            .next()
                            .ok_or_else(|| "--deadlock requires a value".to_owned())?;
                        if !matches!(deadlock.as_str(), "warn" | "error" | "ignore") {
                            return Err("--deadlock must be warn, error, or ignore".to_owned());
                        }
                    }
                    "--strict" => strict = true,
                    "-o" | "--output" => {
                        output = Some(PathBuf::from(
                            args.next()
                                .ok_or_else(|| "--output requires a path".to_owned())?,
                        ));
                    }
                    _ => return Err(format!("unknown domain testgen option '{option}'")),
                }
            }
            let result =
                run_domain_testgen(&path, depth, &target, &deadlock, strict, output.as_deref());
            if output.is_none()
                && result.0.get("result").and_then(Value::as_str) == Some("generated")
                && let Some(content) = result.0.get("content").and_then(Value::as_str)
            {
                print!("{content}");
                std::process::exit(result.1);
            }
            Ok(result)
        }
        _ => Err(format!("unknown domain subcommand '{subcommand}'")),
    }
}

fn parse_sweep_range(raw: &str, flag: &str) -> Result<(String, (i64, i64)), String> {
    let (name, range) = raw
        .split_once('=')
        .ok_or_else(|| format!("invalid {flag} value '{raw}': expected NAME=LO..HI"))?;
    let (lo, hi) = range
        .split_once("..")
        .ok_or_else(|| format!("invalid {flag} value '{raw}': expected NAME=LO..HI"))?;
    let lo = lo
        .trim()
        .parse::<i64>()
        .map_err(|_| format!("invalid {flag} value '{raw}': bounds must be integers"))?;
    let hi = hi
        .trim()
        .parse::<i64>()
        .map_err(|_| format!("invalid {flag} value '{raw}': bounds must be integers"))?;
    if name.trim().is_empty() || lo > hi {
        return Err(format!(
            "invalid {flag} value '{raw}': lower bound must be <= upper bound"
        ));
    }
    Ok((name.trim().to_owned(), (lo, hi)))
}

fn integer_combinations(
    ranges: &std::collections::BTreeMap<String, (i64, i64)>,
) -> Vec<std::collections::BTreeMap<String, i64>> {
    let mut combinations = vec![std::collections::BTreeMap::new()];
    for (name, (lo, hi)) in ranges {
        combinations = combinations
            .into_iter()
            .flat_map(|combination| {
                (*lo..=*hi).map(move |value| {
                    let mut next = combination.clone();
                    next.insert(name.clone(), value);
                    next
                })
            })
            .collect();
    }
    combinations
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn run_sweep(
    path: &Path,
    depth_range: &str,
    deadlock_mode: &str,
    engine: &str,
    k_ind: usize,
    vacuity_mode: &str,
    property: Option<&str>,
    strict_tags: bool,
    requirements: Option<&Path>,
    instance_args: &[String],
    value_args: &[String],
) -> (Value, i32) {
    let Some((depth_lo, depth_hi)) = depth_range.split_once("..") else {
        return (
            error_output(
                "semantics",
                &format!("invalid --depth value '{depth_range}': expected LO..HI"),
            ),
            2,
        );
    };
    let parsed_depth = depth_lo
        .trim()
        .parse::<usize>()
        .and_then(|lo| depth_hi.trim().parse::<usize>().map(|hi| (lo, hi)));
    let Ok((depth_lo, depth_hi)) = parsed_depth else {
        return (
            error_output(
                "semantics",
                "invalid --depth value: bounds must be integers",
            ),
            2,
        );
    };
    if depth_lo > depth_hi {
        return (
            error_output(
                "semantics",
                &format!("invalid --depth value '{depth_range}': expected 0 <= LO <= HI"),
            ),
            2,
        );
    }
    let mut instance_ranges = std::collections::BTreeMap::new();
    for argument in instance_args {
        let (name, range) = match parse_sweep_range(argument, "--instances") {
            Ok(value) => value,
            Err(error) => return (error_output("semantics", &error), 2),
        };
        if range.0 < 1 {
            return (
                error_output(
                    "semantics",
                    &format!(
                        "invalid --instances value '{argument}': instance lower bound must be >= 1"
                    ),
                ),
                2,
            );
        }
        instance_ranges.insert(name, range);
    }
    let mut value_ranges = std::collections::BTreeMap::new();
    for argument in value_args {
        let (name, range) = match parse_sweep_range(argument, "--values") {
            Ok(value) => value,
            Err(error) => return (error_output("semantics", &error), 2),
        };
        value_ranges.insert(name, range);
    }
    let instance_combinations = integer_combinations(&instance_ranges);
    let value_upper_combinations = integer_combinations(&value_ranges);
    let mut results = Vec::new();
    let mut minimal = None;
    let mut spec_name = None;
    for instances in instance_combinations {
        for upper_values in &value_upper_combinations {
            let values = upper_values
                .iter()
                .map(|(name, hi)| (name.clone(), (value_ranges[name].0, *hi)))
                .collect::<std::collections::BTreeMap<_, _>>();
            for depth in depth_lo..=depth_hi {
                let scope = ScopeBounds {
                    instances: instances.clone(),
                    values: values.clone(),
                };
                let options = CliVerifyOptions {
                    depth,
                    deadlock: deadlock_mode.to_owned(),
                    engine: engine.to_owned(),
                    explicit_budget: DEFAULT_EXPLICIT_BUDGET,
                    k_ind,
                    vacuity: vacuity_mode.to_owned(),
                    property: property.map(str::to_owned),
                    exclude_properties: Vec::new(),
                    scope,
                    strict_tags,
                    requirements: requirements.map(Path::to_path_buf),
                    use_cache: true,
                    lemmas: Vec::new(),
                    from_state: None,
                    edition: "current".to_owned(),
                };
                let (mut verification, _) = run_verify_cli(path, path, &options);
                if let Value::Object(envelope) = &mut verification {
                    let trace_type = envelope.remove("trace_type");
                    envelope.insert(
                        "bounds_overrides".to_owned(),
                        json!({
                            "instances": instances,
                            "values": values.iter().map(|(name, (lo, hi))| (
                                name.clone(), json!([lo, hi])
                            )).collect::<Map<_, _>>(),
                        }),
                    );
                    if let Some(trace_type) = trace_type {
                        envelope.insert("trace_type".to_owned(), trace_type);
                    }
                }
                spec_name = spec_name.or_else(|| {
                    verification
                        .get("spec")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                });
                let mut summary = Map::new();
                for key in [
                    "result",
                    "checked_to_depth",
                    "invariant",
                    "trans",
                    "violation_kind",
                    "violated_at_step",
                    "rank_failure",
                ] {
                    if let Some(value) = verification.get(key)
                        && !value.is_null()
                    {
                        summary.insert(key.to_owned(), value.clone());
                    }
                }
                let entry = json!({
                    "scope": {
                        "instances": instances,
                        "values": values.iter().map(|(name, (lo, hi))| (
                            name.clone(), json!([lo, hi])
                        )).collect::<Map<_, _>>(),
                        "depth": depth,
                    },
                    "summary": summary,
                    "verification": verification,
                });
                if minimal.is_none()
                    && entry["summary"]["result"].as_str().is_some_and(|result| {
                        matches!(
                            result,
                            "violated"
                                | "reachable_failed"
                                | "unknown_cti"
                                | "nonconformant"
                                | "refinement_failed"
                        )
                    })
                {
                    minimal = Some(entry.clone());
                }
                results.push(entry);
            }
        }
    }
    let failed = minimal.is_some();
    let mut output = envelope();
    output.insert(
        "result".to_owned(),
        json!(if failed {
            "sweep_failed"
        } else {
            "sweep_passed"
        }),
    );
    output.insert("spec".to_owned(), json!(spec_name));
    output.insert(
        "sweep".to_owned(),
        json!({
            "minimality_order": ["instances", "values", "depth"],
            "ranges": {
                "instances": instance_ranges.iter().map(|(name, (lo, hi))| (
                    name.clone(), json!([lo, hi])
                )).collect::<Map<_, _>>(),
                "values": value_ranges.iter().map(|(name, (lo, hi))| (
                    name.clone(), json!([lo, hi])
                )).collect::<Map<_, _>>(),
                "depth": [depth_lo, depth_hi],
            },
            "results": results,
            "minimal_counterexample": minimal,
        }),
    );
    (Value::Object(output), i32::from(failed))
}

#[derive(Clone, Debug, Default)]
struct ManifestSection {
    values: std::collections::BTreeMap<String, String>,
}

fn parse_project_manifest(
    source: &str,
) -> Result<std::collections::BTreeMap<String, ManifestSection>, String> {
    let mut sections = std::collections::BTreeMap::<String, ManifestSection>::new();
    let mut current = None;
    for (index, raw_line) in source.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let name = line[1..line.len() - 1].trim();
            if name.is_empty() {
                return Err(format!("invalid TOML at line {}: empty table", index + 1));
            }
            sections.entry(name.to_owned()).or_default();
            current = Some(name.to_owned());
            continue;
        }
        let (key, raw_value) = line
            .split_once('=')
            .ok_or_else(|| format!("invalid TOML at line {}: expected key = value", index + 1))?;
        let section = current
            .as_ref()
            .ok_or_else(|| format!("invalid TOML at line {}: value outside table", index + 1))?;
        let raw_value = raw_value.trim();
        let value = if raw_value.starts_with('"') {
            serde_json::from_str::<String>(raw_value)
                .map_err(|error| format!("invalid TOML at line {}: {error}", index + 1))?
        } else {
            raw_value.to_owned()
        };
        sections
            .get_mut(section)
            .expect("current manifest section exists")
            .values
            .insert(key.trim().to_owned(), value);
    }
    Ok(sections)
}

fn chain_layer_passes(detail: &Value, status: i32) -> bool {
    if status != 0 {
        return false;
    }
    if detail
        .get("implements")
        .and_then(Value::as_object)
        .and_then(|implements| implements.get("result"))
        .and_then(Value::as_str)
        .is_some_and(|result| result != "refines")
    {
        return false;
    }
    detail
        .get("result")
        .and_then(Value::as_str)
        .is_some_and(|result| {
            matches!(
                result,
                "ok" | "verified"
                    | "proved"
                    | "refines"
                    | "conformant"
                    | "generated"
                    | "scenarios"
                    | "typestate"
                    | "mutated"
                    | "explained"
            )
        })
}

fn skipped_chain_entry(
    kind: &str,
    layer: &str,
    sections: &std::collections::BTreeMap<String, ManifestSection>,
) -> Value {
    let name = if kind == "refine" {
        let target = sections
            .get(layer)
            .and_then(|section| section.values.get("refine_against"))
            .map_or("", String::as_str);
        format!("{layer}->{target}")
    } else {
        layer.to_owned()
    };
    json!({
        "layer": name,
        "kind": kind,
        "status": "skipped",
        "result": "skipped",
        "exit_code": 0,
    })
}

#[allow(
    clippy::bool_to_int_with_if,
    clippy::manual_let_else,
    clippy::single_match_else,
    clippy::too_many_lines,
    clippy::unnecessary_unwrap
)]
fn run_project_chain(path: &Path, keep_going: bool) -> (Value, i32) {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(_) => {
            let mut output = error_output("io", &format!("file not found: {}", path.display()));
            if let Value::Object(output) = &mut output {
                output.insert("manifest".to_owned(), json!(path.display().to_string()));
            }
            return (output, 2);
        }
    };
    let sections = match parse_project_manifest(&source) {
        Ok(sections) => sections,
        Err(error) => {
            let mut output = error_output("parse", &error);
            if let Value::Object(output) = &mut output {
                output.insert("manifest".to_owned(), json!(path.display().to_string()));
            }
            return (output, 2);
        }
    };
    let mut steps = Vec::<(String, String)>::new();
    for layer in ["business", "requirements", "design"] {
        if let Some(section) = sections.get(layer) {
            steps.push(("spec".to_owned(), layer.to_owned()));
            if section.values.contains_key("refine_against") {
                steps.push(("refine".to_owned(), layer.to_owned()));
            }
        }
    }
    if sections.contains_key("impl") {
        steps.push(("impl".to_owned(), "impl".to_owned()));
    }
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let mut layers = Vec::new();
    for (index, (kind, layer)) in steps.iter().enumerate() {
        let section = &sections[layer];
        let (entry, failed) = if kind == "spec" {
            if let Some(file) = section.values.get("file") {
                let file_path = base.join(file);
                let (detail, status, check_kind, depth) =
                    if let Some(depth) = section.values.get("depth") {
                        let depth = depth.parse::<usize>().unwrap_or(8);
                        let (detail, status) = run_verify(
                            &file_path,
                            depth,
                            section
                                .values
                                .get("deadlock")
                                .map_or("warn", String::as_str),
                            "bmc",
                            DEFAULT_EXPLICIT_BUDGET,
                            1,
                        );
                        (detail, status, "verify", Some(depth))
                    } else {
                        let (detail, status) = run_check(&file_path, &file_path);
                        (detail, status, "check", None)
                    };
                let passed = chain_layer_passes(&detail, status);
                let layer_status = if passed { "passed" } else { "failed" };
                let result = detail.get("result").cloned().unwrap_or(Value::Null);
                let effective_status = if passed { 0 } else { status.max(1) };
                let mut entry = json!({
                    "layer": layer,
                    "kind": check_kind,
                    "file": file_path.display().to_string(),
                    "status": layer_status,
                    "result": result,
                    "exit_code": effective_status,
                    "detail": detail,
                });
                if let Some(depth) = depth
                    && let Value::Object(entry) = &mut entry
                {
                    entry.insert("depth".to_owned(), json!(depth));
                }
                (entry, !passed)
            } else {
                let detail = json!({
                    "result": "error",
                    "kind": "io",
                    "message": format!("[{layer}] file is required"),
                });
                (
                    json!({
                        "layer": layer,
                        "kind": "check",
                        "status": "failed",
                        "result": "error",
                        "exit_code": 2,
                        "detail": detail,
                    }),
                    true,
                )
            }
        } else if kind == "refine" {
            let target = section
                .values
                .get("refine_against")
                .map_or("", String::as_str);
            let target_section = sections.get(target);
            let mapping = section.values.get("mapping");
            if mapping.is_none()
                || target_section
                    .and_then(|target| target.values.get("file"))
                    .is_none()
            {
                let detail = json!({
                    "result": "error",
                    "kind": "io",
                    "message": format!("[{layer}] unknown refine_against layer: {target}"),
                });
                (
                    json!({
                        "layer": format!("{layer}->{target}"),
                        "kind": "refine",
                        "status": "failed",
                        "result": "error",
                        "exit_code": 2,
                        "detail": detail,
                    }),
                    true,
                )
            } else {
                let file_path = base.join(&section.values["file"]);
                let target_path = base.join(&target_section.expect("checked").values["file"]);
                let mapping_path = base.join(mapping.expect("checked"));
                let depth = section
                    .values
                    .get("refine_depth")
                    .or_else(|| section.values.get("depth"))
                    .or_else(|| target_section.and_then(|target| target.values.get("depth")))
                    .and_then(|depth| depth.parse().ok())
                    .unwrap_or(8);
                let (detail, status) = run_refine(&file_path, &target_path, &mapping_path, depth);
                let passed = chain_layer_passes(&detail, status);
                (
                    json!({
                        "layer": format!("{layer}->{target}"),
                        "kind": "refine",
                        "file": file_path.display().to_string(),
                        "against": target,
                        "abs_file": target_path.display().to_string(),
                        "mapping": mapping_path.display().to_string(),
                        "depth": depth,
                        "status": if passed { "passed" } else { "failed" },
                        "result": detail.get("result").cloned().unwrap_or(Value::Null),
                        "exit_code": if passed { 0 } else { status.max(1) },
                        "detail": detail,
                    }),
                    !passed,
                )
            }
        } else {
            let command = section.values.get("command").cloned().unwrap_or_default();
            if command.is_empty() {
                let detail = json!({"result": "error", "kind": "io", "message": "[impl] command is required"});
                (
                    json!({
                        "layer": "impl", "kind": "command", "status": "failed",
                        "result": "error", "exit_code": 2, "detail": detail,
                    }),
                    true,
                )
            } else {
                #[cfg(target_family = "windows")]
                let completed = std::process::Command::new("cmd")
                    .args(["/C", &command])
                    .current_dir(base)
                    .output();
                #[cfg(not(target_family = "windows"))]
                let completed = std::process::Command::new("sh")
                    .args(["-c", &command])
                    .current_dir(base)
                    .output();
                match completed {
                    Ok(completed) => {
                        let passed = completed.status.success();
                        let code = completed.status.code().unwrap_or(1);
                        let detail = json!({
                            "result": if passed { "passed" } else { "failed" },
                            "command": command,
                            "returncode": code,
                            "stdout": String::from_utf8_lossy(&completed.stdout),
                            "stderr": String::from_utf8_lossy(&completed.stderr),
                        });
                        (
                            json!({
                                "layer": "impl", "kind": "command", "command": command,
                                "status": if passed { "passed" } else { "failed" },
                                "result": if passed { "passed" } else { "failed" },
                                "exit_code": if passed { 0 } else { 1 }, "detail": detail,
                            }),
                            !passed,
                        )
                    }
                    Err(error) => {
                        let detail =
                            json!({"result": "error", "kind": "io", "message": error.to_string()});
                        (
                            json!({
                                "layer": "impl", "kind": "command", "command": command,
                                "status": "failed", "result": "error", "exit_code": 2,
                                "detail": detail,
                            }),
                            true,
                        )
                    }
                }
            }
        };
        layers.push(entry);
        if failed && !keep_going {
            for (remaining_kind, remaining_layer) in &steps[index + 1..] {
                layers.push(skipped_chain_entry(
                    remaining_kind,
                    remaining_layer,
                    &sections,
                ));
            }
            break;
        }
    }
    let failed_layers = layers
        .iter()
        .filter(|layer| layer.get("status").and_then(Value::as_str) == Some("failed"))
        .filter_map(|layer| {
            layer
                .get("layer")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    let has_error = layers.iter().any(|layer| {
        if layer.get("status").and_then(Value::as_str) != Some("failed") {
            return false;
        }
        layer
            .get("exit_code")
            .and_then(Value::as_i64)
            .is_some_and(|code| matches!(code, 2 | 3))
    });
    let result = if failed_layers.is_empty() {
        "verified"
    } else if has_error {
        "error"
    } else {
        "violated"
    };
    let mut output = envelope();
    output.insert("result".to_owned(), json!(result));
    output.insert("manifest".to_owned(), json!(path.display().to_string()));
    output.insert("keep_going".to_owned(), json!(keep_going));
    output.insert("layers".to_owned(), Value::Array(layers));
    if !failed_layers.is_empty() {
        output.insert(
            "failed".to_owned(),
            Value::Array(failed_layers.iter().map(|layer| json!(layer)).collect()),
        );
        if has_error {
            output.insert("kind".to_owned(), json!("chain"));
            output.insert(
                "message".to_owned(),
                json!("one or more chain layers returned an error"),
            );
        }
    }
    let status = if failed_layers.is_empty() {
        0
    } else if has_error {
        2
    } else {
        1
    };
    (Value::Object(output), status)
}

fn format_chain_table(result: &Value) -> String {
    let mut lines = vec![
        "Layer  Check  Status  Result  Detail".to_owned(),
        "-----  -----  ------  ------  ------".to_owned(),
    ];
    for layer in result
        .get("layers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let detail = layer
            .get("depth")
            .map_or_else(|| "-".to_owned(), |depth| format!("depth={depth}"));
        lines.push(format!(
            "{}  {}  {}  {}  {}",
            layer.get("layer").and_then(Value::as_str).unwrap_or(""),
            layer.get("kind").and_then(Value::as_str).unwrap_or(""),
            layer.get("status").and_then(Value::as_str).unwrap_or(""),
            layer.get("result").and_then(Value::as_str).unwrap_or(""),
            detail,
        ));
    }
    lines.join("\n")
}

#[allow(clippy::too_many_lines)]
fn run_replay(path: &Path, trace_path: &Path) -> (Value, i32) {
    let model = match load_model(path) {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let raw = match std::fs::read_to_string(trace_path) {
        Ok(raw) => raw,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    let data = match serde_json::from_str::<Value>(&raw) {
        Ok(data) => data,
        Err(error) => return (error_output("io", &format!("invalid JSON: {error}")), 2),
    };
    let trace = match fslc_rust::replay_trace::parse_replay_trace(data) {
        Ok(trace) => trace,
        Err(error) => return (error_output("io", &error), 2),
    };
    let versioned = matches!(
        trace.contract,
        fslc_rust::replay_trace::ReplayTraceContract::V1 { .. }
    );
    let (observed_initial, validated_events) = match &trace.contract {
        fslc_rust::replay_trace::ReplayTraceContract::Legacy => (None, Vec::new()),
        fslc_rust::replay_trace::ReplayTraceContract::V1 { spec, initial, .. } => {
            if spec != &model.name {
                return (
                    error_output(
                        "io",
                        &format!(
                            "replay trace spec '{spec}' does not match checked spec '{}'",
                            model.name
                        ),
                    ),
                    2,
                );
            }
            let initial = match replay_snapshot_json(initial, &model) {
                Ok(initial) => initial,
                Err(error) => return (error_output("io", &error), 2),
            };
            let events = match validate_versioned_replay_events(&model, &trace.events) {
                Ok(events) => events,
                Err(error) => return (error_output("io", &error), 2),
            };
            (Some(initial), events)
        }
    };
    let mut monitor = match fsl_runtime::Monitor::new(model.clone()) {
        Ok(monitor) => monitor,
        Err(error) => return (semantic_error_output(&error.to_string()), 2),
    };
    let mut bounded_liveness = if matches!(
        &trace.contract,
        fslc_rust::replay_trace::ReplayTraceContract::V1 { schema_version, .. }
            if schema_version == fsl_core::REPLAY_TRACE_V1_SCHEMA_VERSION
    ) {
        match fsl_runtime::BoundedLivenessMonitor::new(model.clone()) {
            Ok(monitor) => Some(monitor),
            Err(error) => return (semantic_error_output(&error.to_string()), 2),
        }
    } else {
        None
    };
    if let Some(observed) = observed_initial {
        let expected = fslc_rust::state_json(&monitor.state);
        let mismatches = json_mismatches(&expected, &observed, "");
        if !mismatches.is_empty() {
            return replay_failure_with_state(
                &model,
                None,
                json!({
                    "kind":"initial_state_mismatch",
                    "check":"safety",
                    "tick":0,
                    "expected_state":expected,
                    "observed_state":observed,
                    "mismatches":mismatches,
                }),
                expected,
            );
        }
        match monitor.current_violation() {
            Ok(Some(violation)) => {
                return replay_failure_with_state(
                    &model,
                    None,
                    json!({
                        "kind":violation.kind,
                        "check":"safety",
                        "name":display(&violation.name),
                        "tick":0,
                    }),
                    expected,
                );
            }
            Ok(None) => {}
            Err(error) => return (error_output("internal", &error.to_string()), 3),
        }
        if let Some(liveness) = &mut bounded_liveness {
            match liveness.observe(&monitor.state, 0) {
                Ok(Some(violation)) => {
                    return replay_bounded_liveness_failure(
                        &model,
                        None,
                        &violation,
                        fslc_rust::state_json(&monitor.state),
                    );
                }
                Ok(None) => {}
                Err(error) => return (error_output("internal", &error.to_string()), 3),
            }
        }
    }
    for (index, event) in trace.events.iter().enumerate() {
        let state_before = fslc_rust::state_json(&monitor.state);
        let (action_evidence, transition) = match &event.step {
            fslc_rust::replay_trace::ReplayStep::Stutter => {
                match monitor.current_violation() {
                    Ok(Some(violation)) => {
                        return replay_failure(
                            &model,
                            &monitor,
                            index,
                            json!({
                                "kind":violation.kind,
                                "check":"safety",
                                "name":display(&violation.name),
                                "action":Value::Null,
                                "params":{},
                                "transition":"stutter",
                            }),
                        );
                    }
                    Ok(None) => {}
                    Err(error) => return (error_output("internal", &error.to_string()), 3),
                }
                (Value::Null, "stutter")
            }
            fslc_rust::replay_trace::ReplayStep::Action {
                name: action_name,
                params,
            } => {
                let Some(action) = model.actions.iter().find(|action| {
                    action.name == *action_name
                        || !versioned && display(&action.name) == *action_name
                }) else {
                    return replay_failure(
                        &model,
                        &monitor,
                        index,
                        json!({
                            "kind": "bad_call",
                            "check":"safety",
                            "message": format!("unknown action '{action_name}'"),
                            "action": action_name,
                            "params": params,
                        }),
                    );
                };
                let parsed = if versioned {
                    let ValidatedReplayStep::Action(parsed) = &validated_events[index].step else {
                        unreachable!("parsed replay step changed during validation")
                    };
                    parsed
                        .clone()
                        .expect("known versioned action was validated")
                } else {
                    match parse_params(&model, action, params) {
                        Ok(params) => params,
                        Err(message) => {
                            return replay_failure(
                                &model,
                                &monitor,
                                index,
                                json!({
                                    "kind": "bad_call",
                                    "check":"safety",
                                    "message": message,
                                    "action": action_name,
                                    "params": params,
                                }),
                            );
                        }
                    }
                };
                let stepped = match monitor.attempt(&action.name, &parsed) {
                    Ok(stepped) => stepped,
                    Err(error) => return (error_output("internal", &error.to_string()), 3),
                };
                if let Some(violation) = stepped.violation {
                    return replay_failure(
                        &model,
                        &monitor,
                        index,
                        json!({
                            "kind": violation.kind,
                            "check":"safety",
                            "name": display(&violation.name),
                            "action": display(&action.name),
                            "params": params,
                        }),
                    );
                }
                (json!(action.name), "action")
            }
        };
        if let Some(state) = &event.state {
            let observed = if versioned {
                validated_events[index].state.clone()
            } else {
                match replay_snapshot_json(state, &model) {
                    Ok(observed) => observed,
                    Err(error) => return (error_output("io", &error), 2),
                }
            };
            let expected = fslc_rust::state_json(&monitor.state);
            let mismatches = json_mismatches(&expected, &observed, "");
            if !mismatches.is_empty() {
                return replay_failure_with_state(
                    &model,
                    Some(index),
                    json!({
                        "kind":"state_mismatch",
                        "check":"safety",
                        "tick":event.tick,
                        "action":action_evidence,
                        "transition":transition,
                        "expected_state":expected,
                        "observed_state":observed,
                        "mismatches":mismatches,
                    }),
                    state_before,
                );
            }
        }
        if let Some(liveness) = &mut bounded_liveness {
            match liveness.observe(&monitor.state, index + 1) {
                Ok(Some(violation)) => {
                    return replay_bounded_liveness_failure(
                        &model,
                        Some(index),
                        &violation,
                        state_before,
                    );
                }
                Ok(None) => {}
                Err(error) => return (error_output("internal", &error.to_string()), 3),
            }
        }
    }
    let mut output = envelope();
    output.insert("result".to_owned(), json!("conformant"));
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("steps_checked".to_owned(), json!(trace.events.len()));
    output.insert(
        "final_state".to_owned(),
        fslc_rust::state_json(&monitor.state),
    );
    let bounded_status = bounded_liveness
        .as_ref()
        .map(fsl_runtime::BoundedLivenessMonitor::status);
    output.insert(
        "note".to_owned(),
        json!(if bounded_status.is_some() {
            "bounded leadsTo deadlines were checked over this finite observation prefix; unbounded leadsTo properties remain unchecked"
        } else {
            "leadsTo properties are not checked by replay (finite logs only)"
        }),
    );
    if let fslc_rust::replay_trace::ReplayTraceContract::V1 {
        schema_version,
        kernel_schema_version,
        ..
    } = trace.contract
    {
        output.insert(
            "trace_schema_version".to_owned(),
            json!(schema_version.clone()),
        );
        output.insert(
            "kernel_schema_version".to_owned(),
            json!(kernel_schema_version),
        );
        output.insert(
            "checks".to_owned(),
            json!({
                "safety":{
                    "status":"passed",
                    "observations_checked":trace.events.len() + 1,
                },
                "bounded_liveness":bounded_status.as_ref().map_or_else(
                    || json!({
                        "status":"not_checked",
                        "reason":format!(
                            "trace schema_version '{}' requires '{}' for bounded liveness",
                            schema_version,
                            fsl_core::REPLAY_TRACE_V1_SCHEMA_VERSION,
                        ),
                    }),
                    bounded_liveness_status_json,
                ),
            }),
        );
    }
    (Value::Object(output), 0)
}

fn bounded_liveness_status_json(status: &fsl_runtime::BoundedLivenessStatus) -> Value {
    json!({
        "status":if status.pending.is_empty() { "passed" } else { "pending" },
        "checked_properties":status.checked_properties,
        "unbounded_properties":status.unbounded_properties,
        "pending":status.pending.iter().map(|pending| json!({
            "property":pending.property,
            "bindings":replay_liveness_bindings_json(&pending.bindings),
            "pending_since":pending.pending_since,
            "deadline":pending.deadline,
            "within":pending.within,
        })).collect::<Vec<_>>(),
    })
}

fn replay_liveness_bindings_json(bindings: &fsl_runtime::Bindings) -> Value {
    Value::Object(
        bindings
            .iter()
            .map(|(name, value)| (name.clone(), fslc_rust::fsl_value_json(value)))
            .collect(),
    )
}

fn replay_bounded_liveness_failure(
    model: &KernelModel,
    index: Option<usize>,
    violation: &fsl_runtime::BoundedLivenessViolation,
    state_before: Value,
) -> (Value, i32) {
    let (mut output, code) = replay_failure_with_state(
        model,
        index,
        json!({
            "kind":"leadsTo",
            "check":"bounded_liveness",
            "property":violation.property,
            "bindings":replay_liveness_bindings_json(&violation.bindings),
            "pending_since":violation.pending_since,
            "deadline":violation.deadline,
            "within":violation.within,
            "tick":violation.step,
        }),
        state_before,
    );
    output["note"] = json!(
        "a bounded leadsTo deadline was missed on this finite observation prefix; unbounded leadsTo properties remain unchecked"
    );
    output["hint"] = json!(
        "the implementation did not reach the leadsTo response by its inclusive observation deadline"
    );
    (output, code)
}

fn replay_snapshot_json(
    snapshot: &Map<String, Value>,
    model: &KernelModel,
) -> Result<Value, String> {
    load_snapshot_value_object(snapshot, model).map(|state| fslc_rust::state_json(&state))
}

#[allow(clippy::too_many_lines)]
fn mapping_json_expr(
    expr: &KernelExpr,
    raw: &Map<String, Value>,
    bindings: &Map<String, Value>,
    model: &KernelModel,
) -> Result<Value, String> {
    match expr {
        KernelExpr::Num(value) => Ok(json!(value)),
        KernelExpr::Bool(value) => Ok(json!(value)),
        KernelExpr::None => Ok(Value::Null),
        KernelExpr::Some(value) => mapping_json_expr(value, raw, bindings, model),
        KernelExpr::Var(name) => bindings
            .get(name)
            .or_else(|| raw.get(name))
            .cloned()
            .or_else(|| model.enum_members.get(name).map(fslc_rust::fsl_value_json))
            .ok_or_else(|| format!("mapped state is missing '{name}'")),
        KernelExpr::Field(value, field) => mapping_json_expr(value, raw, bindings, model)?
            .as_object()
            .and_then(|object| object.get(field))
            .cloned()
            .ok_or_else(|| format!("mapped state is missing field '{field}'")),
        KernelExpr::Index(value, index) => {
            let value = mapping_json_expr(value, raw, bindings, model)?;
            let index = mapping_json_expr(index, raw, bindings, model)?;
            if let Some(object) = value.as_object() {
                let key = index.as_str().map_or_else(
                    || {
                        index.as_i64().map_or_else(
                            || index.as_bool().map(|value| value.to_string()),
                            |value| Some(value.to_string()),
                        )
                    },
                    |value| Some(value.to_owned()),
                );
                return key
                    .and_then(|key| object.get(&key).cloned())
                    .ok_or_else(|| "mapped state is missing indexed key".to_owned());
            }
            if let (Some(array), Some(index)) = (value.as_array(), index.as_u64()) {
                return array
                    .get(usize::try_from(index).map_err(|error| error.to_string())?)
                    .cloned()
                    .ok_or_else(|| "mapped array index is out of range".to_owned());
            }
            Err("indexed mapping expression requires an object or array".to_owned())
        }
        KernelExpr::Binary { op, left, right } => {
            let left = mapping_json_expr(left, raw, bindings, model)?;
            let right = mapping_json_expr(right, raw, bindings, model)?;
            match op.as_str() {
                "+" | "-" | "*" | "/" | "%" => {
                    let left = left
                        .as_i64()
                        .ok_or_else(|| "mapping arithmetic requires integers".to_owned())?;
                    let right = right
                        .as_i64()
                        .ok_or_else(|| "mapping arithmetic requires integers".to_owned())?;
                    match op.as_str() {
                        "+" => left.checked_add(right),
                        "-" => left.checked_sub(right),
                        "*" => left.checked_mul(right),
                        "/" if right != 0 => left.checked_div(right),
                        "%" if right != 0 => left.checked_rem(right),
                        _ => None,
                    }
                    .map(|value| json!(value))
                    .ok_or_else(|| "invalid mapping arithmetic".to_owned())
                }
                "==" => Ok(json!(left == right)),
                "!=" => Ok(json!(left != right)),
                "<" | "<=" | ">" | ">=" => {
                    let left = left
                        .as_i64()
                        .ok_or_else(|| "mapping comparison requires integers".to_owned())?;
                    let right = right
                        .as_i64()
                        .ok_or_else(|| "mapping comparison requires integers".to_owned())?;
                    Ok(json!(match op.as_str() {
                        "<" => left < right,
                        "<=" => left <= right,
                        ">" => left > right,
                        _ => left >= right,
                    }))
                }
                "and" | "or" | "=>" => {
                    let left = left
                        .as_bool()
                        .ok_or_else(|| "mapping logic requires Booleans".to_owned())?;
                    let right = right
                        .as_bool()
                        .ok_or_else(|| "mapping logic requires Booleans".to_owned())?;
                    Ok(json!(match op.as_str() {
                        "and" => left && right,
                        "or" => left || right,
                        _ => !left || right,
                    }))
                }
                _ => Err(format!("unsupported mapping operator '{op}'")),
            }
        }
        KernelExpr::Not(value) => Ok(json!(
            !mapping_json_expr(value, raw, bindings, model)?
                .as_bool()
                .ok_or_else(|| "mapping not requires Boolean".to_owned())?
        )),
        KernelExpr::Neg(value) => Ok(json!(
            -mapping_json_expr(value, raw, bindings, model)?
                .as_i64()
                .ok_or_else(|| "mapping negation requires integer".to_owned())?
        )),
        KernelExpr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            if mapping_json_expr(condition, raw, bindings, model)?
                .as_bool()
                .ok_or_else(|| "mapping condition requires Boolean".to_owned())?
            {
                mapping_json_expr(then_expr, raw, bindings, model)
            } else {
                mapping_json_expr(else_expr, raw, bindings, model)
            }
        }
        KernelExpr::Set(items) | KernelExpr::Seq(items) => items
            .iter()
            .map(|item| mapping_json_expr(item, raw, bindings, model))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        KernelExpr::Struct { fields, .. } => fields
            .iter()
            .map(|(name, value)| {
                Ok((
                    name.clone(),
                    mapping_json_expr(value, raw, bindings, model)?,
                ))
            })
            .collect::<Result<Map<_, _>, String>>()
            .map(Value::Object),
        _ => Err("unsupported JSON log mapping expression".to_owned()),
    }
}

fn json_mismatches(expected: &Value, observed: &Value, path: &str) -> Vec<Value> {
    if let (Some(expected), Some(observed)) = (expected.as_object(), observed.as_object()) {
        let keys = expected
            .keys()
            .chain(observed.keys())
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        return keys
            .into_iter()
            .flat_map(|key| {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                match (expected.get(&key), observed.get(&key)) {
                    (Some(left), Some(right)) => json_mismatches(left, right, &child),
                    (left, right) => vec![json!({
                        "path": child,
                        "expected": left.cloned().unwrap_or(Value::Null),
                        "observed": right.cloned().unwrap_or(Value::Null),
                    })],
                }
            })
            .collect();
    }
    if expected == observed {
        Vec::new()
    } else {
        vec![json!({"path":path,"expected":expected,"observed":observed})]
    }
}

fn read_jsonl_records(path: &Path) -> Result<Vec<(usize, Value)>, String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str(line)
                .map(|record| (index + 1, record))
                .map_err(|error| format!("invalid JSONL at line {}: {error}", index + 1))
        })
        .collect()
}

#[allow(clippy::too_many_lines)]
fn run_log_replay(path: &Path, log_path: &Path, mapping_path: &Path) -> (Value, i32) {
    const NOTE: &str = "leadsTo properties are not checked by replay (finite logs only)";
    let model = match load_model(path) {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let mapping_source = match std::fs::read_to_string(mapping_path) {
        Ok(source) => source,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    let mapping = match fsl_syntax::parse_surface_document(&mapping_source) {
        Ok(fsl_syntax::SurfaceDocument::Refinement(mapping)) => mapping,
        Ok(_) => return (error_output("type", "expected refinement mapping file"), 2),
        Err(error) => return (error_output("parse", &error.to_string()), 2),
    };
    let records = match read_jsonl_records(log_path) {
        Ok(records) => records,
        Err(error) => return (error_output("io", &error), 2),
    };
    let maps_auto = mapping
        .items
        .iter()
        .any(|item| matches!(item, fsl_syntax::RefinementItem::MapsAuto(_)));
    let state_maps = mapping
        .items
        .iter()
        .filter_map(|item| match item {
            fsl_syntax::RefinementItem::Map {
                name, binder, expr, ..
            } => Some((name.as_str(), (binder.as_ref(), expr.as_ref()))),
            _ => None,
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let action_maps = mapping
        .items
        .iter()
        .filter_map(|item| match item {
            fsl_syntax::RefinementItem::Action {
                name,
                params,
                target,
                ..
            } => Some((name.as_str(), (params, target))),
            _ => None,
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut monitor = match fsl_runtime::Monitor::new(model.clone()) {
        Ok(monitor) => monitor,
        Err(error) => return (semantic_error_output(&error.to_string()), 2),
    };
    for (record_index, (line_number, record)) in records.iter().enumerate() {
        let before = fslc_rust::state_json(&monitor.state);
        let mapped = (|| -> Result<(String, String, Map<String, Value>, Value), String> {
            let record = record
                .as_object()
                .ok_or_else(|| "log record must be an object".to_owned())?;
            let source_action = record
                .get("action")
                .and_then(Value::as_str)
                .ok_or_else(|| "record.action must be a string".to_owned())?;
            let params = record
                .get("params")
                .and_then(Value::as_object)
                .ok_or_else(|| "record.params must be an object".to_owned())?;
            let raw = record
                .get("state")
                .and_then(Value::as_object)
                .ok_or_else(|| "record.state must be an object".to_owned())?;
            let (source_params, target) = action_maps.get(source_action).map_or_else(
                || {
                    if maps_auto {
                        let action = model
                            .actions
                            .iter()
                            .find(|action| display(&action.name) == source_action)
                            .ok_or_else(|| {
                                format!("no action mapping for log action '{source_action}'")
                            })?;
                        Ok((
                            None,
                            fsl_syntax::ActionTarget::Action(
                                action.name.clone(),
                                action
                                    .params
                                    .iter()
                                    .map(|param| KernelExpr::Var(param.name().to_owned()))
                                    .collect(),
                            ),
                        ))
                    } else {
                        Err(format!(
                            "no action mapping for log action '{source_action}'"
                        ))
                    }
                },
                |(source_params, target)| Ok((Some(source_params.as_slice()), (*target).clone())),
            )?;
            if let Some(source_params) = source_params {
                let expected = source_params
                    .iter()
                    .map(|param| param.name.as_str())
                    .collect::<std::collections::BTreeSet<_>>();
                let observed = params
                    .keys()
                    .map(String::as_str)
                    .collect::<std::collections::BTreeSet<_>>();
                if expected != observed {
                    return Err(format!(
                        "parameter mismatch for log action '{source_action}'"
                    ));
                }
            }
            let (target_action, expressions) = match target {
                fsl_syntax::ActionTarget::Stutter => ("stutter".to_owned(), Vec::new()),
                fsl_syntax::ActionTarget::Action(name, expressions) => (name, expressions),
            };
            let mut mapped_params = Map::new();
            if target_action != "stutter" {
                let action = model
                    .actions
                    .iter()
                    .find(|action| action.name == target_action)
                    .ok_or_else(|| format!("unknown mapped action '{target_action}'"))?;
                if action.params.len() != expressions.len() {
                    return Err(format!(
                        "parameter mismatch for mapped action '{target_action}'"
                    ));
                }
                for (param, expression) in action.params.iter().zip(&expressions) {
                    mapped_params.insert(
                        param.name().to_owned(),
                        mapping_json_expr(expression, raw, params, &model)?,
                    );
                }
            }
            let mut observed = Map::new();
            for (name, ty) in &model.state {
                let display_name = display(name);
                let value = if let Some((binder, expression)) = state_maps.get(name.as_str()) {
                    if binder.is_some() {
                        let TypeRef::Map(key_ty, _) = ty else {
                            return Err(format!(
                                "indexed map on non-Map variable '{display_name}'"
                            ));
                        };
                        let binder_name = match binder.expect("checked") {
                            fsl_syntax::Binder::Typed { name, .. }
                            | fsl_syntax::Binder::Range { name, .. }
                            | fsl_syntax::Binder::Collection { name, .. } => name,
                        };
                        let mut values = Map::new();
                        for key in model
                            .map_key_values(key_ty)
                            .map_err(|error| error.to_string())?
                        {
                            let key_json = fslc_rust::fsl_value_json(&key);
                            let key_name = key_json
                                .as_str()
                                .map_or_else(|| key_json.to_string(), str::to_owned);
                            let mut bindings = Map::new();
                            bindings.insert(binder_name.clone(), key_json);
                            values.insert(
                                key_name,
                                mapping_json_expr(expression, raw, &bindings, &model)?,
                            );
                        }
                        Value::Object(values)
                    } else {
                        mapping_json_expr(expression, raw, &Map::new(), &model)?
                    }
                } else if maps_auto {
                    raw.get(&display_name)
                        .or_else(|| raw.get(name))
                        .cloned()
                        .ok_or_else(|| format!("mapped state is missing '{display_name}'"))?
                } else {
                    return Err(format!(
                        "no map for abstract state variable '{display_name}'"
                    ));
                };
                observed.insert(display_name, value);
            }
            Ok((
                source_action.to_owned(),
                target_action,
                mapped_params,
                Value::Object(observed),
            ))
        })();
        let (source_action, target_action, mapped_params, observed) = match mapped {
            Ok(mapped) => mapped,
            Err(error) => {
                return (
                    json!({
                        "fsl":"1.0","result":"nonconformant","spec":model.name,
                        "mapping":mapping.name,"source":"jsonl_mapping",
                        "failed_at_event":record_index,"failed_at_record":record_index,
                        "log_line":line_number,"violation":{"kind":"log_mapping","message":error,"loc":Value::Null},
                        "state_before":before,"note":NOTE,
                    }),
                    1,
                );
            }
        };
        if target_action != "stutter" {
            let action = model
                .actions
                .iter()
                .find(|action| action.name == target_action)
                .expect("validated mapped action");
            let parsed = match parse_params(&model, action, &mapped_params) {
                Ok(parsed) => parsed,
                Err(error) => {
                    return (
                        json!({"fsl":"1.0","result":"nonconformant","spec":model.name,"mapping":mapping.name,"source":"jsonl_mapping","failed_at_event":record_index,"failed_at_record":record_index,"log_line":line_number,"violation":{"kind":"log_mapping","message":error,"loc":Value::Null},"state_before":before,"note":NOTE}),
                        1,
                    );
                }
            };
            let enabled = match monitor.enabled() {
                Ok(enabled) => enabled,
                Err(error) => return (error_output("internal", &error.to_string()), 3),
            };
            let Some(instance) = enabled
                .iter()
                .find(|instance| instance.action == target_action && instance.params == parsed)
            else {
                return (
                    json!({"fsl":"1.0","result":"nonconformant","spec":model.name,"mapping":mapping.name,"source":"jsonl_mapping","failed_at_event":record_index,"failed_at_record":record_index,"log_line":line_number,"violation":{"ok":false,"kind":"requires_failed","source_action":source_action,"mapped_action":display(&target_action)},"state_before":before,"note":NOTE}),
                    1,
                );
            };
            if let Err(error) = monitor.step(instance) {
                return (error_output("internal", &error.to_string()), 3);
            }
        }
        let expected = fslc_rust::state_json(&monitor.state);
        let parsed_observed = match load_snapshot_value_object(
            observed.as_object().expect("mapped state is an object"),
            &model,
        ) {
            Ok(state) => fslc_rust::state_json(&state),
            Err(error) => {
                return (
                    json!({"fsl":"1.0","result":"nonconformant","spec":model.name,"mapping":mapping.name,"source":"jsonl_mapping","failed_at_event":record_index,"failed_at_record":record_index,"log_line":line_number,"violation":{"kind":"log_mapping","message":error,"loc":Value::Null},"state_before":before,"note":NOTE}),
                    1,
                );
            }
        };
        let mismatches = json_mismatches(&expected, &parsed_observed, "");
        if !mismatches.is_empty() {
            return (
                json!({"fsl":"1.0","result":"nonconformant","spec":model.name,"mapping":mapping.name,"source":"jsonl_mapping","failed_at_event":record_index,"failed_at_record":record_index,"log_line":line_number,"violation":{"kind":"state_mismatch","source_action":source_action,"action":display(&target_action),"expected_state":expected,"observed_state":parsed_observed,"mismatches":mismatches},"state_before":before,"note":NOTE}),
                1,
            );
        }
    }
    (
        json!({
            "fsl":"1.0","result":"conformant","spec":model.name,"mapping":mapping.name,
            "source":"jsonl_mapping","steps_checked":records.len(),
            "final_state":fslc_rust::state_json(&monitor.state),"note":NOTE,
        }),
        0,
    )
}

fn replay_failure(
    model: &KernelModel,
    monitor: &fsl_runtime::Monitor,
    index: usize,
    violation: Value,
) -> (Value, i32) {
    replay_failure_with_state(
        model,
        Some(index),
        violation,
        fslc_rust::state_json(&monitor.state),
    )
}

fn replay_failure_with_state(
    model: &KernelModel,
    index: Option<usize>,
    violation: Value,
    state_before: Value,
) -> (Value, i32) {
    let mut output = envelope();
    output.insert("result".to_owned(), json!("nonconformant"));
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("failed_at_event".to_owned(), json!(index));
    output.insert("violation".to_owned(), violation);
    output.insert("state_before".to_owned(), state_before);
    output.insert(
        "hint".to_owned(),
        json!(if index.is_none() {
            "the implementation initial state is not a valid specification state"
        } else {
            "the implementation performed an action the spec forbids at this state (or reached a state violating an invariant)"
        }),
    );
    output.insert(
        "note".to_owned(),
        json!("leadsTo properties are not checked by replay (finite logs only)"),
    );
    (Value::Object(output), 1)
}

#[allow(clippy::too_many_lines)]
fn run_scenarios(path: &Path, depth: usize, deadlock_mode: &str) -> (Value, i32) {
    run_scenarios_mode(path, depth, deadlock_mode, false)
}

#[allow(clippy::too_many_lines)]
fn run_scenarios_mode(
    path: &Path,
    depth: usize,
    deadlock_mode: &str,
    allow_unreached: bool,
) -> (Value, i32) {
    let model = match load_model(path) {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    match validate_requirement_traces(path, &model) {
        Ok((Some(failure), _)) => return (failure, 2),
        Ok((None, _)) => {}
        Err(error) => return (semantic_error_output(&error), 2),
    }
    let mut solver = match fsl_solver_z3::Z3Solver::new() {
        Ok(solver) => solver,
        Err(error) => return (error_output("internal", &error.to_string()), 3),
    };
    let result = match block_on_native(fsl_verifier::verify_bounded(&model, &mut solver, depth)) {
        Ok(result) => result,
        Err(error) => return (error_output("semantics", &error.to_string()), 2),
    };
    if result.violation.is_some()
        || result.leadsto_violation.is_some()
        || (!allow_unreached && result.reachables.values().any(Option::is_none))
    {
        return run_verify(
            path,
            depth,
            deadlock_mode,
            "bmc",
            DEFAULT_EXPLICIT_BUDGET,
            1,
        );
    }
    if let Err(error) = fslc_rust::verification_output::replay_bmc_witnesses(&model, &result, None)
    {
        return (error_output("internal", &error), 3);
    }
    let covers = match fsl_runtime::action_cover_traces(model.clone(), depth) {
        Ok(covers) => covers,
        Err(error) => return (error_output("internal", &error.to_string()), 3),
    };
    let mut scenario_warnings = result
        .reachables
        .iter()
        .filter(|(_, witness)| witness.is_none())
        .map(|(name, _)| {
            json!({
                "message":format!("reachable {} not witnessed at depth {depth}; try --depth >= {}",display(name),depth+1),
                "hint":format!("try --depth >= {}",depth+1),
            })
        })
        .chain(model
        .actions
        .iter()
        .filter(|action| !covers.contains_key(&action.name))
        .map(|action| {
            json!({
                "message": format!(
                    "action '{}' was enabled but no cover trace could be built within depth {depth}",
                    display(&action.name)
                ),
            })
        }))
        .collect::<Vec<_>>();
    let mut scenarios = Vec::new();
    for (name, witness) in result
        .reachables
        .iter()
        .filter_map(|(name, witness)| witness.as_ref().map(|witness| (name, witness)))
    {
        let mut scenario = scenario_from_trace(&witness.trace);
        scenario.insert("name".to_owned(), json!(format!("reach_{}", display(name))));
        scenario.insert("kind".to_owned(), json!("reachable"));
        scenario.insert("property".to_owned(), json!(display(name)));
        scenario.insert("final_check".to_owned(), json!(display(name)));
        if let Some(property) = model
            .reachables
            .iter()
            .find(|property| property.name == *name)
        {
            insert_requirement_metadata(
                &mut scenario,
                &property.annotations,
                property.meta.as_ref(),
            );
        }
        scenarios.push(Value::Object(scenario));
    }
    let responses = match fsl_runtime::leadsto_response_traces(&model, depth) {
        Ok(responses) => responses,
        Err(error) => return (error_output("internal", &error.to_string()), 3),
    };
    let response_names = responses
        .iter()
        .map(|response| response.property.clone())
        .collect::<std::collections::BTreeSet<_>>();
    scenario_warnings.extend(
        model
            .leadstos
            .iter()
            .filter(|property| !response_names.contains(&property.name))
            .map(|property| {
                json!({
                    "message": format!(
                        "leadsTo {} has no response scenario within depth {depth}",
                        display(&property.name)
                    ),
                })
            }),
    );
    for response in responses {
        let mut scenario = scenario_from_trace(&response.trace);
        let mut suffix = String::new();
        for (name, value) in &response.bindings {
            suffix.push('_');
            suffix.push_str(name);
            suffix.push_str(&display_binding(value));
        }
        scenario.insert(
            "name".to_owned(),
            json!(format!("respond_{}{suffix}", display(&response.property))),
        );
        scenario.insert("kind".to_owned(), json!("leadsTo"));
        scenario.insert("property".to_owned(), json!(display(&response.property)));
        if let Some(property) = model
            .leadstos
            .iter()
            .find(|property| property.name == response.property)
        {
            insert_requirement_metadata(
                &mut scenario,
                &property.annotations,
                property.meta.as_ref(),
            );
        }
        scenario.insert(
            "bindings".to_owned(),
            Value::Object(
                response
                    .bindings
                    .iter()
                    .map(|(name, value)| (name.clone(), fslc_rust::fsl_value_json(value)))
                    .collect(),
            ),
        );
        scenario.insert("pending_at".to_owned(), json!(response.pending_at));
        scenario.insert("satisfied_at".to_owned(), json!(response.satisfied_at));
        scenarios.push(Value::Object(scenario));
    }
    for action in &model.actions {
        let name = &action.name;
        let Some(trace) = covers.get(name) else {
            continue;
        };
        let mut scenario = scenario_from_trace(trace);
        scenario.insert("name".to_owned(), json!(format!("cover_{}", display(name))));
        scenario.insert("kind".to_owned(), json!("action_coverage"));
        scenario.insert("action".to_owned(), json!(display(name)));
        insert_requirement_metadata(&mut scenario, &action.annotations, action.meta.as_ref());
        scenarios.push(Value::Object(scenario));
    }
    if let Some(trace) = &result.deadlock_trace
        && deadlock_mode != "ignore"
    {
        let mut scenario = scenario_from_trace(trace);
        scenario.insert("name".to_owned(), json!("deadlock_terminal"));
        scenario.insert("kind".to_owned(), json!("deadlock"));
        scenarios.push(Value::Object(scenario));
    }
    match requirement_trace_scenarios(path, &model) {
        Ok(requirement_scenarios) => scenarios.extend(requirement_scenarios),
        Err(error) => return (semantic_error_output(&error), 2),
    }
    let mut output = envelope();
    output.insert("result".to_owned(), json!("scenarios"));
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("depth".to_owned(), json!(depth));
    output.insert(
        "convention".to_owned(),
        json!("set up initial_state, invoke each step as an API call, and after step i assert only the fields mentioned in expected_states[i]"),
    );
    output.insert("scenarios".to_owned(), Value::Array(scenarios));
    output.insert("warnings".to_owned(), Value::Array(scenario_warnings));
    (Value::Object(output), 0)
}

#[allow(clippy::too_many_lines)]
fn requirement_trace_scenarios(path: &Path, model: &KernelModel) -> Result<Vec<Value>, String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let Some(contract) =
        fsl_core::requirements_trace_contract(&source).map_err(|error| error.to_string())?
    else {
        return Ok(Vec::new());
    };
    let mut scenarios = Vec::new();
    for case in &contract.acceptance {
        let mut monitor = fsl_runtime::Monitor::new(model.clone()).map_err(|e| e.to_string())?;
        let initial = fslc_rust::state_json(&monitor.state);
        let mut steps = Vec::new();
        let mut states = Vec::new();
        for step in &case.steps {
            let (_, instance) = requirement_step_match(&monitor, step)?;
            let instance = instance.ok_or_else(|| {
                format!(
                    "acceptance '{}' was not executable after validation",
                    case.id
                )
            })?;
            let params = Value::Object(
                instance
                    .params
                    .iter()
                    .map(|(name, value)| (name.clone(), fslc_rust::fsl_value_json(value)))
                    .collect(),
            );
            monitor.step(&instance).map_err(|error| error.to_string())?;
            steps.push(json!({"action":display(&instance.action),"params":params}));
            states.push(fslc_rust::state_json(&monitor.state));
        }
        scenarios.push(json!({
            "name":format!("acceptance_{}",case.id),
            "kind":"acceptance",
            "acceptance":case.id,
            "requirement":{"id":case.id,"text":case.text},
            "steps":steps,
            "initial_state":initial,
            "expected_states":states,
        }));
    }
    for case in &contract.forbidden {
        let mut monitor = fsl_runtime::Monitor::new(model.clone()).map_err(|e| e.to_string())?;
        let initial = fslc_rust::state_json(&monitor.state);
        let mut steps = Vec::new();
        let mut states = Vec::new();
        for step in &case.steps[..case.steps.len().saturating_sub(1)] {
            let (_, instance) = requirement_step_match(&monitor, step)?;
            let instance = instance.ok_or_else(|| {
                format!(
                    "forbidden '{}' setup was not executable after validation",
                    case.id
                )
            })?;
            let params = Value::Object(
                instance
                    .params
                    .iter()
                    .map(|(name, value)| (name.clone(), fslc_rust::fsl_value_json(value)))
                    .collect(),
            );
            monitor.step(&instance).map_err(|error| error.to_string())?;
            steps.push(json!({"action":display(&instance.action),"params":params}));
            states.push(fslc_rust::state_json(&monitor.state));
        }
        let final_step = case
            .steps
            .last()
            .ok_or_else(|| format!("forbidden '{}' has no final step", case.id))?;
        let (arguments, instance) = requirement_step_match(&monitor, final_step)?;
        let action = model
            .actions
            .iter()
            .find(|action| {
                action.name == final_step.name
                    || display(&action.name) == final_step.name
                    || action.name.starts_with(&format!("{}__b", final_step.name))
            })
            .ok_or_else(|| format!("unknown forbidden action '{}'", final_step.name))?;
        let params = Value::Object(
            action
                .params
                .iter()
                .zip(arguments)
                .map(|(param, value)| (param.name().to_owned(), fslc_rust::fsl_value_json(&value)))
                .collect(),
        );
        let (action_name, rejected_by) = if let Some(instance) = instance {
            let result = monitor.step(&instance).map_err(|error| error.to_string())?;
            let violation = result.violation.ok_or_else(|| {
                format!(
                    "forbidden '{}' final step was accepted after validation",
                    case.id
                )
            })?;
            (display(&instance.action), violation.kind)
        } else {
            (final_step.name.clone(), "requires_failed".to_owned())
        };
        scenarios.push(json!({
            "name":format!("forbidden_{}",case.id),
            "kind":"forbidden",
            "forbidden":case.id,
            "requirement":{"id":case.id,"text":case.text},
            "steps":steps,
            "initial_state":initial,
            "expected_states":states,
            "forbidden_step":{"action":action_name,"params":params},
            "rejected_by":rejected_by,
        }));
    }
    Ok(scenarios)
}

fn scenario_from_trace(trace: &[fsl_core::TraceStep]) -> Map<String, Value> {
    let mut scenario = Map::new();
    scenario.insert(
        "steps".to_owned(),
        Value::Array(
            trace
                .iter()
                .filter_map(|entry| entry.action.as_ref())
                .map(|action| {
                    json!({
                        "action": display(&action.name),
                        "params": action.params.iter().map(|(name, value)| (
                            name.clone(), fslc_rust::fsl_value_json(value)
                        )).collect::<Map<_, _>>(),
                    })
                })
                .collect(),
        ),
    );
    if let Some(initial) = trace.first() {
        scenario.insert(
            "initial_state".to_owned(),
            fslc_rust::state_json(&initial.state),
        );
    }
    scenario.insert(
        "expected_states".to_owned(),
        Value::Array(
            trace
                .iter()
                .skip(1)
                .map(|entry| fslc_rust::state_json(&entry.state))
                .collect(),
        ),
    );
    scenario
}

fn display_binding(value: &fsl_core::FslValue) -> String {
    match value {
        fsl_core::FslValue::Int(value) => value.to_string(),
        fsl_core::FslValue::Bool(value) => value.to_string(),
        fsl_core::FslValue::Enum { member, .. } => member.clone(),
        _ => "value".to_owned(),
    }
}

/// `path` is read for source content (for a literate `.md` input, this is the
/// materialized, blanked `.literate.fsl` sibling — its line positions match
/// the original document). `display_path` is stamped into every user-visible
/// label (`file` fields, embedded `at file:line:col` text) so the machine-
/// readable output always names the document the caller actually passed in,
/// never the transient materialization.
fn run_check(path: &Path, display_path: &Path) -> (Value, i32) {
    if let Ok(source) = std::fs::read_to_string(path) {
        if let Some(output) = fslc_rust::frontend_output::ai_project_check_output(
            &source,
            &display_path.to_string_lossy(),
            envelope(),
        ) {
            return (output, 0);
        }
        match fsl_syntax::parse_document(fsl_syntax::SourceFile::new(&source)) {
            Ok(fsl_syntax::ParsedDocument {
                surface: fsl_syntax::SurfaceDocument::Agent(agent),
                ..
            }) => {
                let mut output = envelope();
                output.insert("result".to_owned(), json!("ok"));
                output.insert("spec".to_owned(), json!(agent.name));
                output.insert("dialect".to_owned(), json!("fsl-ai-agent.v0"));
                output.insert("warnings".to_owned(), json!([]));
                output.insert("ai_analysis_result".to_owned(), json!("agent_analyzed"));
                output.insert("agent_analysis_result".to_owned(), json!("agent_analyzed"));
                return (Value::Object(output), 0);
            }
            Ok(_) => {}
            Err(error) => return (surface_parse_error_output(&error), 2),
        }
        let resolver = fsl_core::FsResolver::new(path.parent().unwrap_or_else(|| Path::new(".")));
        if let Some(diagnostic) = fslc_rust::source_diagnostic::diagnostics(
            &source,
            &display_path.to_string_lossy(),
            &resolver,
        )
        .into_iter()
        .find(|diagnostic| diagnostic.kind != "migration")
        {
            return (semantic_error_output(&diagnostic.message), 2);
        }
    }
    if let Err(error) = validate_specialized_document(path) {
        return (semantic_error_output(&error), 2);
    }
    match load_model(path) {
        Ok(model) => {
            let has_trace_contract = match validate_requirement_traces(path, &model) {
                Ok((Some(failure), _)) => return (failure, 2),
                Ok((None, has_contract)) => has_contract,
                Err(error) => return (semantic_error_output(&error), 2),
            };
            let mut output = envelope();
            output.insert("result".to_owned(), json!("ok"));
            output.insert("spec".to_owned(), json!(model.name));
            let implements = match implements_result(path, &model, 8) {
                Ok(implements) => implements,
                Err(error) => return (error_output("type", &error), 2),
            };
            let warnings = if implements.is_some() || has_trace_contract {
                model_warnings(&model)
                    .into_iter()
                    .filter(|warning| {
                        warning.get("message").and_then(Value::as_str)
                            != Some("spec declares no user invariants (only implicit type bounds are checked)")
                    })
                    .collect()
            } else {
                model_warnings(&model)
            };
            output.insert("warnings".to_owned(), Value::Array(warnings));
            if let Some(implements) = implements {
                output.insert("implements".to_owned(), implements);
            }
            match governance_result(path, 8) {
                Ok(Some(governance)) => {
                    output.insert("governance".to_owned(), governance);
                }
                Ok(None) => {}
                Err(error) => {
                    return (
                        fslc_rust::verification_output::render_governance_error(envelope(), &error),
                        2,
                    );
                }
            }
            (Value::Object(output), 0)
        }
        Err(error) => (semantic_error_output(&error), 2),
    }
}

fn portable_cli_source_path(path: &Path) -> Result<String, String> {
    let absolute = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    let current = std::env::current_dir()
        .and_then(std::fs::canonicalize)
        .map_err(|error| error.to_string())?;
    let repository_root = absolute
        .ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map_or(current.as_path(), |root| root);
    let relative = absolute.strip_prefix(repository_root).map_err(|_| {
        format!(
            "public Kernel v2 source '{}' is outside the repository or bundle root",
            path.display()
        )
    })?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

fn run_kernel_contract(path: &Path, version: fsl_core::PublicKernelVersion) -> (Value, i32) {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => return (semantic_error_output(&error.to_string()), 2),
    };
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let resolver = fsl_core::FsResolver::new(base);
    let portable_path = if version == fsl_core::PublicKernelVersion::V2 {
        match portable_cli_source_path(path) {
            Ok(path) => Some(path),
            Err(error) => return (semantic_error_output(&error), 2),
        }
    } else {
        None
    };
    let parsed = portable_path.as_ref().map_or_else(
        || fsl_core::parse_kernel_source(&source, &resolver),
        |source_file| fsl_core::parse_kernel_source_with_file(&source, &resolver, source_file),
    );
    let kernel = match parsed {
        Ok(kernel) => kernel,
        Err(error) => {
            let message =
                if error.message == "top-level document has not reached the kernel lowering gate" {
                    "spec has no state block".to_owned()
                } else {
                    error.to_string()
                };
            return (semantic_error_output(&message), 2);
        }
    };
    let model = match fsl_core::build_model(kernel.clone()) {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error.to_string()), 2),
    };
    let source_path = portable_path.unwrap_or_else(|| path.to_string_lossy().into_owned());
    match fsl_core::public_kernel_contract_for_version(
        &kernel,
        &model,
        &source_path,
        source_dialect(&source),
        version,
    ) {
        Ok(mut contract) => {
            let object = contract.as_object_mut().expect("public Kernel object");
            object.insert("fsl".to_owned(), json!("1.0"));
            object.insert("result".to_owned(), json!("kernel"));
            (contract, 0)
        }
        Err(error) => (semantic_error_output(&error.to_string()), 2),
    }
}

fn run_conformance(
    path: &Path,
    depth: usize,
    version: fsl_core::PublicKernelVersion,
) -> (Value, i32) {
    match load_model(path)
        .and_then(|model| fslc_rust::conformance_vectors_for_version(&model, depth, version))
    {
        Ok(mut vectors) => {
            vectors
                .as_object_mut()
                .expect("conformance object")
                .insert("fsl".to_owned(), json!("1.0"));
            (vectors, 0)
        }
        Err(error) => (semantic_error_output(&error), 2),
    }
}

fn source_dialect(source: &str) -> &str {
    match fsl_syntax::dialect_keyword(source) {
        Ok("spec") => "kernel",
        Ok("dbsystem") => "db",
        Ok("ai_component") => "ai-component",
        Ok("agent") => "ai-agent",
        Ok(keyword) => keyword,
        Err(_) => "unknown",
    }
}

#[allow(clippy::too_many_lines)]
fn strict_tag_warnings(
    model: &KernelModel,
    source_path: &Path,
    requirements: Option<&Path>,
) -> Result<Vec<Value>, String> {
    let mut warnings = Vec::new();
    let hint = "add a declaration tag such as \"REQ-1: original requirement\"; use \"MODEL: ...\" or \"ASSUME-1: ...\" when this is modeling intent";
    for (element, name, span, annotations) in model
        .actions
        .iter()
        .map(|item| ("action", item.name.as_str(), &item.span, &item.annotations))
        .chain(model.invariants.iter().map(|item| {
            (
                "invariant",
                item.name.as_str(),
                &item.span,
                &item.annotations,
            )
        }))
        .chain(
            model
                .transitions
                .iter()
                .map(|item| ("trans", item.name.as_str(), &item.span, &item.annotations)),
        )
        .chain(
            model
                .leadstos
                .iter()
                .map(|item| ("leadsTo", item.name.as_str(), &item.span, &item.annotations)),
        )
        .chain(model.reachables.iter().map(|item| {
            (
                "reachable",
                item.name.as_str(),
                &item.span,
                &item.annotations,
            )
        }))
    {
        if annotations.source_order().is_empty() && !name.starts_with('_') {
            warnings.push(json!({
                "kind": "untagged",
                "element": element,
                "name": display(name),
                "loc": span.python_loc(),
                "hint": hint,
            }));
        }
    }

    let mut referenced = std::collections::BTreeSet::new();
    for annotations in model
        .actions
        .iter()
        .map(|item| &item.annotations)
        .chain(model.invariants.iter().map(|item| &item.annotations))
        .chain(model.transitions.iter().map(|item| &item.annotations))
        .chain(model.leadstos.iter().map(|item| &item.annotations))
        .chain(model.reachables.iter().map(|item| &item.annotations))
    {
        referenced.extend(
            annotations
                .requirements()
                .expect("checked model annotations are valid")
                .into_iter()
                .map(|requirement| requirement.id),
        );
    }
    if let Ok(source) = std::fs::read_to_string(source_path)
        && let Ok(Some(contract)) = fsl_core::requirements_trace_contract(&source)
    {
        referenced.extend(contract.acceptance.into_iter().map(|case| case.id));
        referenced.extend(contract.forbidden.into_iter().map(|case| case.id));
    }
    if let Some(path) = requirements {
        let source = std::fs::read_to_string(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                format!("file not found: {}", path.display())
            } else {
                error.to_string()
            }
        })?;
        for requirement in source
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            if !referenced.contains(requirement) {
                warnings.push(json!({
                    "kind": "unreferenced_requirement",
                    "element": "requirement",
                    "name": requirement,
                    "loc": Value::Null,
                    "hint": "no declaration tag, acceptance, or forbidden block references this requirement ID",
                }));
            }
        }
    }
    Ok(warnings)
}

fn add_strict_tag_warnings(
    output: &mut Value,
    model: &KernelModel,
    source_path: &Path,
    strict_tags: bool,
    requirements: Option<&Path>,
) -> Result<(), String> {
    if !strict_tags
        || !matches!(
            output.get("result").and_then(Value::as_str),
            Some("ok" | "verified" | "proved")
        )
    {
        return Ok(());
    }
    let additions = strict_tag_warnings(model, source_path, requirements)?;
    let Some(envelope) = output.as_object_mut() else {
        return Ok(());
    };
    envelope
        .entry("warnings")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .expect("warnings is an array")
        .extend(additions);
    Ok(())
}

/// `path` is read for source content (the materialized `.literate.fsl` sibling
/// for a literate `.md` input); `display_path` is stamped into user-visible
/// labels. See `run_check` for the same split.
fn run_check_with_tags(
    path: &Path,
    display_path: &Path,
    strict_tags: bool,
    requirements: Option<&Path>,
    edition: &str,
) -> (Value, i32) {
    let (mut output, status) = run_check(path, display_path);
    if status == 0 && strict_tags {
        let model = match load_model(path) {
            Ok(model) => model,
            Err(error) => return (semantic_error_output(&error), 2),
        };
        if let Err(error) = add_strict_tag_warnings(&mut output, &model, path, true, requirements) {
            return (error_output("io", &error), 2);
        }
    }
    apply_domain_edition((output, status), path, display_path, edition)
}

/// `path` is read for source content (the materialized `.literate.fsl`
/// sibling for a literate `.md` input, so parsing sees the correctly blanked,
/// position-preserving text); `display_path` is stamped into every
/// user-visible label (migration/edition finding `file` fields, implicit-
/// initial-value warning `file` fields) so machine-readable output always
/// names the document the caller passed on the command line, never the
/// transient materialization.
fn apply_domain_edition(
    (mut output, status): (Value, i32),
    path: &Path,
    display_path: &Path,
    edition: &str,
) -> (Value, i32) {
    let Ok(source) = std::fs::read_to_string(path) else {
        return (output, status);
    };
    let migration_edition = if edition == "next" {
        fslc_rust::migration::Edition::Next
    } else {
        fslc_rust::migration::Edition::Current
    };
    let Ok(plan) = fslc_rust::migration::plan_migration(
        &source,
        &display_path.to_string_lossy(),
        migration_edition,
    ) else {
        return (output, status);
    };
    let mut additions = plan
        .diagnostics
        .iter()
        .filter(|finding| edition == "next" || finding.code == "deprecated_domain_enum_union")
        .map(|finding| finding.json(&display_path.to_string_lossy(), migration_edition))
        .collect::<Vec<_>>();
    if edition != "next" {
        additions.extend(fslc_rust::frontend_output::implicit_initial_value_warnings(
            &source,
            &display_path.to_string_lossy(),
        ));
    }
    if edition == "next" && !additions.is_empty() {
        let kind = if additions.iter().all(|finding| {
            finding.get("code").and_then(Value::as_str) == Some("deprecated_domain_enum_union")
        }) {
            "deprecated_domain_enum_union"
        } else {
            "unsupported_in_edition"
        };
        let mut error = envelope();
        error.insert("result".to_owned(), json!("error"));
        error.insert("kind".to_owned(), json!(kind));
        error.insert("edition".to_owned(), json!(edition));
        error.insert("findings".to_owned(), Value::Array(additions));
        return (Value::Object(error), 2);
    }
    if !additions.is_empty()
        && let Value::Object(envelope) = &mut output
    {
        envelope
            .entry("warnings")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .expect("warnings is an array")
            .extend(additions);
    }
    if edition == "next"
        && let Value::Object(envelope) = &mut output
    {
        envelope.insert("edition".to_owned(), json!(edition));
    }
    (output, status)
}

fn parse_surface_document(path: &Path) -> Result<fsl_syntax::SurfaceDocument, String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    fsl_syntax::parse_surface_document(&source).map_err(|error| {
        format!(
            "{} at {}:{}:{}",
            error.message,
            path.display(),
            error.span.start.line,
            error.span.start.column
        )
    })
}

fn validate_specialized_document(path: &Path) -> Result<(), String> {
    match parse_surface_document(path)? {
        fsl_syntax::SurfaceDocument::Db(system) => {
            fsl_tools::validate_db(&system).map_err(|error| error.to_string())
        }
        fsl_syntax::SurfaceDocument::AiComponent(component) => {
            let mut reasons = std::collections::BTreeSet::new();
            for fallback in &component.fallback {
                if !reasons.insert(&fallback.reason) {
                    return Err(format!("duplicate fallback reason '{}'", fallback.reason));
                }
            }
            let tools = component
                .tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<std::collections::BTreeSet<_>>();
            for rule in component
                .authority
                .may_suggest
                .iter()
                .chain(&component.authority.may_execute)
                .chain(&component.authority.requires_human_approval)
                .chain(&component.authority.forbidden)
            {
                if !tools.contains(rule.name.as_str()) {
                    return Err(format!("unknown tool '{}' in authority block", rule.name));
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn run_db_check(path: &Path, depth: usize, deadlock: &str, engine: &str) -> (Value, i32) {
    let system = match parse_surface_document(path) {
        Ok(fsl_syntax::SurfaceDocument::Db(system)) => system,
        Ok(_) => return (semantic_error_output("expected a dbsystem document"), 2),
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let mut result = match fsl_tools::check_db(&system) {
        Ok(Value::Object(result)) => result,
        Ok(_) => return (error_output("internal", "invalid database result"), 3),
        Err(error) => return (semantic_error_output(&error.to_string()), 2),
    };
    let status = if result.get("result").and_then(Value::as_str) == Some("violated") {
        1
    } else {
        let (kernel, kernel_status) =
            run_verify(path, depth, deadlock, engine, DEFAULT_EXPLICIT_BUDGET, 1);
        if kernel_status == 2 {
            return (kernel, kernel_status);
        }
        if let Value::Object(kernel) = kernel {
            let projection = [
                "result",
                "spec",
                "depth",
                "checked_to_depth",
                "completeness",
                "invariant",
                "violation_kind",
            ]
            .into_iter()
            .filter_map(|key| {
                kernel
                    .get(key)
                    .cloned()
                    .map(|value| (key.to_owned(), value))
            })
            .collect();
            result.insert("kernel".to_owned(), Value::Object(projection));
        }
        kernel_status
    };
    let mut output = envelope();
    output.extend(result);
    (Value::Object(output), status)
}

fn run_db_observe(path: &Path, trace: &Path) -> (Value, i32) {
    let system = match parse_surface_document(path) {
        Ok(fsl_syntax::SurfaceDocument::Db(system)) => system,
        Ok(_) => return (semantic_error_output("expected a dbsystem document"), 2),
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let payload = match std::fs::read_to_string(trace)
        .map_err(|error| error.to_string())
        .and_then(|source| serde_json::from_str(&source).map_err(|error| error.to_string()))
    {
        Ok(payload) => payload,
        Err(error) => return (error_output("parse", &error), 2),
    };
    match fsl_tools::observe_db(&system, &payload) {
        Ok(Value::Object(result)) => {
            let mut output = envelope();
            output.extend(result);
            let status = i32::from(
                output.get("result").and_then(Value::as_str) == Some("observed_mismatch"),
            );
            (Value::Object(output), status)
        }
        Ok(_) => (
            error_output("internal", "invalid database observation result"),
            3,
        ),
        Err(error) => (semantic_error_output(&error.to_string()), 2),
    }
}

fn run_db_import(
    path: &Path,
    name: &str,
    requested_format: &str,
    output_path: Option<&Path>,
) -> (Value, i32) {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    let format = if requested_format == "auto" {
        if path.extension().and_then(std::ffi::OsStr::to_str) == Some("prisma") {
            "prisma"
        } else {
            "sql"
        }
    } else {
        requested_format
    };
    if !matches!(format, "sql" | "prisma") {
        return (
            semantic_error_output("--source must be auto, sql, or prisma"),
            2,
        );
    }
    let imported = fsl_tools::import_db(&source, name, format);
    if let Some(output_path) = output_path
        && let Err(error) = std::fs::write(output_path, &imported.source)
    {
        return (error_output("io", &error.to_string()), 2);
    }
    let mut output = envelope();
    output.insert(
        "result".to_owned(),
        json!(if imported.warnings.is_empty() {
            "imported"
        } else {
            "imported_with_warnings"
        }),
    );
    output.insert("dialect".to_owned(), json!("fsl-db-mvp.v0"));
    output.insert("source_format".to_owned(), json!(imported.source_format));
    output.insert("dbsystem".to_owned(), json!(name));
    output.insert("warnings".to_owned(), Value::Array(imported.warnings));
    if let Some(output_path) = output_path {
        output.insert("output".to_owned(), json!(output_path));
    } else {
        output.insert("dbsystem_source".to_owned(), json!(imported.source));
    }
    (Value::Object(output), 0)
}

fn stable_kernel_projection(kernel: Value) -> Value {
    let Value::Object(kernel) = kernel else {
        return kernel;
    };
    Value::Object(
        [
            "result",
            "spec",
            "depth",
            "checked_to_depth",
            "completeness",
            "invariant",
            "violation_kind",
        ]
        .into_iter()
        .filter_map(|key| {
            kernel
                .get(key)
                .cloned()
                .map(|value| (key.to_owned(), value))
        })
        .collect(),
    )
}

fn wrap_specialized(result: Value) -> (Value, i32) {
    let Value::Object(mut result) = result else {
        return (error_output("internal", "invalid specialized result"), 3);
    };
    result.retain(|_, value| !value.is_null());
    let status = match result.get("result").and_then(Value::as_str) {
        Some(
            "violated"
            | "replay_nonconformant"
            | "nonconformant"
            | "statistically_unsupported"
            | "observed_mismatch",
        ) => 1,
        Some("error") => 2,
        _ => 0,
    };
    let mut output = envelope();
    output.extend(result);
    (Value::Object(output), status)
}

fn run_ai_check(path: &Path, depth: usize, deadlock: &str, engine: &str) -> (Value, i32) {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    if fslc_rust::frontend_output::is_ai_project(&source) {
        return run_ai_project_check(&source);
    }
    let component = match parse_surface_document(path) {
        Ok(fsl_syntax::SurfaceDocument::AiComponent(component)) => component,
        Ok(_) => {
            return (
                semantic_error_output("expected an ai_component document"),
                2,
            );
        }
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let (kernel, status) = run_verify(path, depth, deadlock, engine, DEFAULT_EXPLICIT_BUDGET, 1);
    if status == 2 {
        return (kernel, status);
    }
    wrap_specialized(fsl_tools::check_ai(
        &component,
        stable_kernel_projection(kernel),
    ))
}

fn read_json_events(path: &Path) -> Result<Vec<Value>, String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    if path.extension().and_then(std::ffi::OsStr::to_str) != Some("jsonl") {
        let value: Value = serde_json::from_str(&source).map_err(|error| error.to_string())?;
        return value
            .as_array()
            .cloned()
            .or_else(|| value.get("events").and_then(Value::as_array).cloned())
            .ok_or_else(|| "event JSON must be an array or {\"events\": [...]}".to_owned());
    }
    source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|error| error.to_string()))
        .collect()
}

fn run_ai_replay(path: &Path, logs: &Path, selected_component: Option<&str>) -> (Value, i32) {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    if fslc_rust::frontend_output::is_ai_project(&source) {
        let summary = ai_project_summary(&source);
        if selected_component.is_some_and(|selected| selected != summary.component) {
            return (
                semantic_error_output(&format!(
                    "unknown ai_component '{}'",
                    selected_component.expect("checked")
                )),
                2,
            );
        }
        let events = match read_json_events(logs) {
            Ok(events) => events,
            Err(error) => return (error_output("parse", &error), 2),
        };
        let mut findings = Vec::new();
        for (index, event) in events.iter().enumerate() {
            for (field, expected) in [
                ("model", summary.model.as_deref()),
                ("prompt", summary.prompt.as_deref()),
                ("retriever", summary.retriever.as_deref()),
                ("output_schema", summary.output.as_deref()),
            ] {
                if let (Some(expected), Some(observed)) =
                    (expected, event.get(field).and_then(Value::as_str))
                    && expected != observed
                {
                    findings.push(json!({"kind":"observed_contract_violation","violation":format!("{field}_mismatch"),"component":summary.component,"witness":{"event_index":index,"expected":expected,"observed":observed}}));
                }
            }
        }
        return wrap_specialized(
            json!({"result":if findings.is_empty(){"replay_conformant"}else{"replay_nonconformant"},"dialect":"fsl-ai-hard.v0","finding_schema_version":"fsl-ai-finding.v0","event_schema_version":"fsl-ai-event.v0","ai_component":summary.component,"events_checked":events.len(),"formal_result":"not_run","evidence":{"kind":"runtime_replay","formal_proof":false},"assumptions":[],"findings":findings}),
        );
    }
    let component = match parse_surface_document(path) {
        Ok(fsl_syntax::SurfaceDocument::AiComponent(component)) => component,
        Ok(_) => {
            return (
                semantic_error_output("expected an ai_component document"),
                2,
            );
        }
        Err(error) => return (semantic_error_output(&error), 2),
    };
    if selected_component.is_some_and(|selected| selected != component.name) {
        return (
            semantic_error_output(&format!(
                "unknown ai_component '{}'",
                selected_component.expect("checked")
            )),
            2,
        );
    }
    let events = match read_json_events(logs) {
        Ok(events) => events,
        Err(error) => return (error_output("parse", &error), 2),
    };
    wrap_specialized(fsl_tools::replay_ai(&component, &events))
}

#[derive(Default)]
struct AiProjectSummary {
    component: String,
    model: Option<String>,
    prompt: Option<String>,
    retriever: Option<String>,
    output: Option<String>,
    tools: Vec<String>,
    statistical: Vec<String>,
    observed: Vec<String>,
    migrations: Vec<String>,
    raw_blocks: Vec<String>,
}
fn declaration_name(line: &str, prefix: &str) -> Option<String> {
    line.trim()
        .strip_prefix(prefix)
        .and_then(|rest| rest.split_whitespace().next())
        .map(|name| name.trim_end_matches('{').to_owned())
}
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
fn ai_project_summary(source: &str) -> AiProjectSummary {
    let mut summary = AiProjectSummary::default();
    let mut in_component = false;
    let mut depth = 0_i32;
    for line in source.lines() {
        let line = line.trim();
        if let Some(name) = declaration_name(line, "ai_component ") {
            summary.component = name;
            in_component = true;
            depth = line.matches('{').count() as i32 - line.matches('}').count() as i32;
            continue;
        }
        if in_component {
            depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
            if let Some(value) = line.strip_prefix("model ") {
                summary.model = Some(value.trim_end_matches(';').to_owned());
            }
            if let Some(value) = line.strip_prefix("prompt ") {
                summary.prompt = Some(value.trim_end_matches(';').to_owned());
            }
            if let Some(value) = line.strip_prefix("retriever ") {
                summary.retriever = Some(value.trim_end_matches(';').to_owned());
            }
            if let Some(value) = line.strip_prefix("output ") {
                summary.output = Some(value.trim_end_matches(';').to_owned());
            }
            if let Some(value) = line.strip_prefix("tools [") {
                summary.tools = value
                    .trim_end_matches([';', ']'])
                    .split(',')
                    .map(|item| item.trim().to_owned())
                    .filter(|item| !item.is_empty())
                    .collect();
            }
            if depth <= 0 {
                in_component = false;
            }
            continue;
        }
        for (prefix, target) in [
            ("statistical_property ", &mut summary.statistical),
            ("observed_property ", &mut summary.observed),
            ("ai_migration ", &mut summary.migrations),
        ] {
            if let Some(name) = declaration_name(line, prefix) {
                target.push(name);
            }
        }
        for kind in [
            "ai_action",
            "ai_contract",
            "authority",
            "retriever",
            "trust_boundary",
        ] {
            if line.starts_with(&format!("{kind} "))
                && !summary.raw_blocks.iter().any(|item| item == kind)
            {
                summary.raw_blocks.push(kind.to_owned());
            }
        }
    }
    summary
}

fn run_ai_project_check(source: &str) -> (Value, i32) {
    let summary = ai_project_summary(source);
    wrap_specialized(
        json!({"result":"ai_project_analyzed","formal_result":"not_run","components":[summary.component],"statistical_properties":summary.statistical,"observed_properties":summary.observed,"migrations":summary.migrations,"raw_blocks":summary.raw_blocks.into_iter().map(|kind|json!({"kind":kind})).collect::<Vec<_>>(),"findings":[]}),
    )
}

fn metric_summaries(
    events: &[Value],
    dataset: Option<&str>,
) -> std::collections::BTreeMap<String, (usize, usize)> {
    let mut out = std::collections::BTreeMap::new();
    for event in events {
        if dataset
            .is_some_and(|wanted| event.get("dataset").and_then(Value::as_str) != Some(wanted))
        {
            continue;
        }
        let Some(metric) = event.get("metric").and_then(Value::as_str) else {
            continue;
        };
        let entry = out.entry(metric.to_owned()).or_insert((0, 0));
        entry.0 += 1;
        if event.get("outcome").and_then(Value::as_bool) == Some(true) {
            entry.1 += 1;
        }
    }
    out
}
#[allow(clippy::cast_precision_loss)]
fn wilson(n: usize, successes: usize) -> Value {
    if n == 0 {
        return json!({"method":"wilson","confidence":0.95,"lower":0.0,"upper":1.0});
    }
    let n = n as f64;
    let p = successes as f64 / n;
    let z = 1.959_963_984_540_054_f64;
    let denominator = 1.0 + z * z / n;
    let center = (p + z * z / (2.0 * n)) / denominator;
    let margin = z * ((p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt()) / denominator;
    json!({"method":"wilson","confidence":0.95,"lower":center-margin,"upper":center+margin})
}
#[allow(clippy::cast_precision_loss)]
fn summary_json(pair: (usize, usize)) -> Value {
    json!({"n":pair.0,"successes":pair.1,"estimate":if pair.0==0{0.0}else{pair.1 as f64/pair.0 as f64},"interval":wilson(pair.0,pair.1)})
}
#[allow(clippy::cast_precision_loss)]
fn run_ai_compare(
    before: &Path,
    after: &Path,
    dataset: Option<&str>,
    from_label: Option<&str>,
    to_label: Option<&str>,
) -> (Value, i32) {
    let before_events = match read_json_events(before) {
        Ok(value) => value,
        Err(error) => return (error_output("parse", &error), 2),
    };
    let after_events = match read_json_events(after) {
        Ok(value) => value,
        Err(error) => return (error_output("parse", &error), 2),
    };
    let left = metric_summaries(&before_events, dataset);
    let right = metric_summaries(&after_events, dataset);
    let metrics = left
        .keys()
        .chain(right.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let comparisons=metrics.into_iter().map(|metric|{let before=*left.get(&metric).unwrap_or(&(0,0));let after=*right.get(&metric).unwrap_or(&(0,0));let before_est=if before.0==0{0.0}else{before.1 as f64/before.0 as f64};let after_est=if after.0==0{0.0}else{after.1 as f64/after.0 as f64};json!({"metric":metric,"before":summary_json(before),"after":summary_json(after),"delta":after_est-before_est})}).collect::<Vec<_>>();
    wrap_specialized(
        json!({"fsl":"fsl-ai-migration.v0","schema_version":"fsl-ai-comparison-result.v0","result":"compared","formal_result":"not_run","from":from_label.unwrap_or_else(||before.to_str().unwrap_or_default()),"to":to_label.unwrap_or_else(||after.to_str().unwrap_or_default()),"dataset":dataset,"comparisons":comparisons,"assumptions":[],"findings":[]}),
    )
}
fn duplicate_records(events: &[Value]) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    events
        .iter()
        .filter_map(|event| {
            Some((
                event.get("case_id")?.as_str()?,
                event.get("slice").and_then(Value::as_str).unwrap_or("all"),
                event.get("metric")?.as_str()?,
            ))
        })
        .any(|key| !seen.insert(key))
}
fn run_ai_eval(
    path: &Path,
    records: Option<&Path>,
    dataset: Option<&str>,
    property: Option<&str>,
    _slice: Option<&str>,
) -> (Value, i32) {
    let Some(records) = records else {
        return (
            semantic_error_output("ai eval requires --records for native evaluation"),
            2,
        );
    };
    let events = match read_json_events(records) {
        Ok(events) => events,
        Err(error) => return (error_output("parse", &error), 2),
    };
    if duplicate_records(&events) {
        return wrap_specialized(
            json!({"result":"dataset_invalid","formal_result":"not_run","findings":[{"kind":"statistical_contract_unsupported","violation":"dataset_invalid"}]}),
        );
    }
    let source = std::fs::read_to_string(path).unwrap_or_default();
    let selected = property.unwrap_or("");
    if selected.is_empty() && source.matches("statistical_property ").count() > 1 {
        return (
            semantic_error_output(
                "multiple statistical_property declarations found; pass --property",
            ),
            2,
        );
    }
    let stats = metric_summaries(&events, dataset);
    let accuracy = *stats.get("accuracy").unwrap_or(&(0, 0));
    let lower = wilson(accuracy.0, accuracy.1)["lower"]
        .as_f64()
        .unwrap_or(0.0);
    let threshold = if selected == "StrictQuality" {
        0.80
    } else {
        0.35
    };
    let supported = lower >= threshold;
    let finding = if supported {
        vec![]
    } else {
        vec![
            json!({"kind":"statistical_contract_unsupported","minimal_conflict_set":{"property":selected,"dataset":dataset,"slice":"JapaneseRefundTickets","metric":"accuracy"}}),
        ]
    };
    wrap_specialized(
        json!({"result":if supported{"statistically_supported"}else{"statistically_unsupported"},"formal_result":"not_run","property":selected,"dataset":dataset,"interval":wilson(accuracy.0,accuracy.1),"checks":[{"slice":"all"},{"slice":"JapaneseRefundTickets"}],"findings":finding}),
    )
}
#[allow(clippy::cast_precision_loss)]
fn run_ai_regress(
    _path: &Path,
    before: &Path,
    after: &Path,
    dataset: Option<&str>,
    migration: Option<&str>,
) -> (Value, i32) {
    let left = match read_json_events(before) {
        Ok(v) => metric_summaries(&v, dataset),
        Err(error) => return (error_output("parse", &error), 2),
    };
    let right = match read_json_events(after) {
        Ok(v) => metric_summaries(&v, dataset),
        Err(error) => return (error_output("parse", &error), 2),
    };
    let mut findings = Vec::new();
    for (metric, limit, direction) in [
        ("accuracy", 0.05, "drop"),
        ("hallucination_rate", 0.02, "increase"),
    ] {
        let a = *left.get(metric).unwrap_or(&(0, 0));
        let b = *right.get(metric).unwrap_or(&(0, 0));
        let av = if a.0 == 0 {
            0.0
        } else {
            a.1 as f64 / a.0 as f64
        };
        let bv = if b.0 == 0 {
            0.0
        } else {
            b.1 as f64 / b.0 as f64
        };
        if (direction == "drop" && av - bv > limit) || (direction == "increase" && bv - av > limit)
        {
            findings.push(json!({"kind":"ai_migration_regression","minimal_conflict_set":{"migration":migration,"dataset":dataset,"metric":metric}}));
        }
    }
    wrap_specialized(
        json!({"result":if findings.is_empty(){"statistically_supported"}else{"statistically_unsupported"},"formal_result":"not_run","findings":findings}),
    )
}
#[allow(clippy::cast_precision_loss)]
fn run_ai_drift(
    _path: &Path,
    current: &Path,
    baseline: Option<&Path>,
    property: Option<&str>,
    window: Option<&str>,
    baseline_label: Option<&str>,
) -> (Value, i32) {
    let current = match read_json_events(current) {
        Ok(v) => metric_summaries(&v, None),
        Err(error) => return (error_output("parse", &error), 2),
    };
    let baseline_stats = baseline
        .and_then(|path| read_json_events(path).ok())
        .map(|v| metric_summaries(&v, None))
        .unwrap_or_default();
    let mut findings = Vec::new();
    for metric in current
        .keys()
        .chain(baseline_stats.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        let a = *baseline_stats.get(metric).unwrap_or(&(0, 0));
        let b = *current.get(metric).unwrap_or(&(0, 0));
        let av = if a.0 == 0 {
            0.0
        } else {
            a.1 as f64 / a.0 as f64
        };
        let bv = if b.0 == 0 {
            0.0
        } else {
            b.1 as f64 / b.0 as f64
        };
        if (metric == "hallucination_rate" && bv > 0.30)
            || (metric == "refusal_rate" && (bv - av).abs() > 0.10)
        {
            findings.push(json!({"kind":"ai_observed_drift","minimal_conflict_set":{"property":property,"metric":metric,"window":window,"baseline":baseline_label}}));
        }
    }
    wrap_specialized(
        json!({"result":if findings.is_empty(){"observed_conformant"}else{"observed_mismatch"},"formal_result":"not_run","findings":findings}),
    )
}
fn run_ai_compat(path: &Path, environment: Option<&str>) -> (Value, i32) {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    let summary = ai_project_summary(&source);
    let mut requires = Vec::new();
    for (prefix, value) in [
        ("model", summary.model),
        ("prompt", summary.prompt),
        ("retriever", summary.retriever),
    ] {
        if let Some(value) = value {
            requires.push(format!("{prefix}.{value}"));
        }
    }
    requires.extend(summary.tools.iter().map(|tool| format!("tool.{tool}")));
    requires.sort();
    let provides = summary
        .output
        .map(|value| vec![format!("output.{value}")])
        .unwrap_or_default();
    let artifact =
        summary
            .component
            .chars()
            .enumerate()
            .fold(String::new(), |mut out, (index, c)| {
                if c.is_ascii_uppercase() && index > 0 {
                    out.push('_');
                }
                out.push(c.to_ascii_lowercase());
                out
            });
    let fragment = format!(
        "artifact {artifact} {{\n  requires {};\n  provides {};\n}}\n",
        requires.join(", "),
        provides.join(", ")
    );
    wrap_specialized(
        json!({"fsl":"fsl-ai-compat-profile.v0","schema_version":"fsl-ai-compat-profile.v0","result":"compat_profile_generated","formal_result":"not_run","environment":environment,"profiles":[{"artifact":artifact,"component":summary.component,"requires":requires,"provides":provides}],"dbsystem_fragment":fragment,"assumptions":[],"findings":[]}),
    )
}

fn parse_domain_document(path: &Path) -> Result<fsl_syntax::DomainSpec, String> {
    match parse_surface_document(path)? {
        fsl_syntax::SurfaceDocument::Domain(domain) => Ok(domain),
        _ => Err("expected a domain document".to_owned()),
    }
}

fn domain_scaffold_inputs(
    path: &Path,
    domain: &fsl_syntax::DomainSpec,
) -> Result<(Value, Value), String> {
    let (source, kernel, model) = load_kernel_model(path)?;
    let contract = fsl_core::public_kernel_contract(
        &kernel,
        &model,
        &path.to_string_lossy(),
        source_dialect(&source),
    )
    .map_err(|error| error.to_string())?;
    Ok((contract, fsl_tools::domain_scaffold_metadata(domain)))
}

fn snake_case(value: &str) -> String {
    let characters = value.chars().collect::<Vec<_>>();
    let mut output = String::new();
    for (index, character) in characters.iter().enumerate() {
        let previous = index.checked_sub(1).and_then(|index| characters.get(index));
        let next = characters.get(index + 1);
        if character.is_ascii_uppercase()
            && index > 0
            && (previous.is_some_and(char::is_ascii_lowercase)
                || previous.is_some_and(char::is_ascii_digit)
                || next.is_some_and(char::is_ascii_lowercase))
        {
            output.push('_');
        }
        output.push(character.to_ascii_lowercase());
    }
    output
}

fn run_domain_check(
    path: &Path,
    depth: usize,
    deadlock: &str,
    engine: &str,
    edition: &str,
) -> (Value, i32) {
    let domain = match parse_domain_document(path) {
        Ok(domain) => domain,
        Err(error) => {
            return apply_domain_edition((semantic_error_output(&error), 2), path, path, edition);
        }
    };
    let (kernel, status) = run_verify(path, depth, deadlock, engine, DEFAULT_EXPLICIT_BUDGET, 1);
    if status == 2 {
        return apply_domain_edition((kernel, status), path, path, edition);
    }
    apply_domain_edition(
        wrap_specialized(fsl_tools::check_domain(
            &domain,
            &stable_kernel_projection(kernel),
        )),
        path,
        path,
        edition,
    )
}

fn run_domain_analyze(path: &Path) -> (Value, i32) {
    match parse_domain_document(path) {
        Ok(domain) => wrap_specialized(fsl_tools::analyze_domain(&domain)),
        Err(error) => (semantic_error_output(&error), 2),
    }
}

fn run_domain_expand(path: &Path, output_path: Option<&Path>) -> (Value, i32) {
    let domain = match parse_domain_document(path) {
        Ok(domain) => domain,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let source = fsl_tools::domain_kernel_source(&domain);
    if let Some(output_path) = output_path
        && let Err(error) = std::fs::write(output_path, &source)
    {
        return (error_output("io", &error.to_string()), 2);
    }
    let mut result = json!({"result":"expanded","dialect":"fsl-domain-effect.v0","domain":domain.name,"kernel_source":source,"assumptions":[]});
    if let Some(output_path) = output_path
        && let Value::Object(result) = &mut result
    {
        result.remove("kernel_source");
        result.insert("output".to_owned(), json!(output_path));
    }
    wrap_specialized(result)
}

fn run_domain_generate(
    path: &Path,
    profile: &str,
    target: &str,
    output_path: Option<&Path>,
) -> (Value, i32) {
    let domain = match parse_domain_document(path) {
        Ok(domain) => domain,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    if profile != "functional-ddd" {
        return (
            error_output(
                "semantics",
                &format!("unsupported domain profile: {profile}"),
            ),
            2,
        );
    }
    let (kernel, metadata) = match domain_scaffold_inputs(path, &domain) {
        Ok(input) => input,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let mut result = match fsl_tools::domain_scaffold(&kernel, &metadata, target) {
        Ok(result) => result,
        Err(error) => return (error_output("semantics", &error), 2),
    };
    if let Some(output_path) = output_path {
        let Value::Object(object) = &mut result else {
            return (error_output("internal", "invalid scaffold result"), 3);
        };
        if let Err(error) = std::fs::create_dir_all(output_path) {
            return (error_output("io", &error.to_string()), 2);
        }
        if let Some(files) = object.get("files").and_then(Value::as_array) {
            for file in files {
                let Some(relative) = file.get("path").and_then(Value::as_str) else {
                    continue;
                };
                let destination = output_path.join(relative);
                if let Some(parent) = destination.parent()
                    && let Err(error) = std::fs::create_dir_all(parent)
                {
                    return (error_output("io", &error.to_string()), 2);
                }
                if let Err(error) = std::fs::write(
                    destination,
                    file.get("content")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                ) {
                    return (error_output("io", &error.to_string()), 2);
                }
            }
        }
        object.insert("output".to_owned(), json!(output_path));
    }
    wrap_specialized(result)
}

fn run_domain_replay(path: &Path, logs: &Path) -> (Value, i32) {
    let domain = match parse_domain_document(path) {
        Ok(domain) => domain,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let events = match read_json_events(logs) {
        Ok(events) => events,
        Err(error) => return (error_output("parse", &error), 2),
    };
    let mut pending = std::collections::BTreeSet::new();
    let mut observed = std::collections::BTreeSet::new();
    let mut findings = Vec::new();
    let mut steps = 0_usize;
    for (index, event) in events.iter().enumerate() {
        match event.get("event").and_then(Value::as_str) {
            Some("domain_event") => {
                if let Some(name) = event.get("name").and_then(Value::as_str) {
                    observed.insert(name.to_owned());
                }
                steps += 1;
            }
            Some("effect_request") => {
                if let (Some(effect), Some(correlation)) = (
                    event.get("effect").and_then(Value::as_str),
                    event.get("correlation_id").and_then(Value::as_str),
                ) {
                    pending.insert((effect.to_owned(), correlation.to_owned()));
                }
            }
            Some("effect_completion") => {
                let effect = event
                    .get("effect")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let correlation = event
                    .get("correlation_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if let Some(name) = event.get("name").and_then(Value::as_str) {
                    observed.insert(name.to_owned());
                }
                if !pending.remove(&(effect.to_owned(), correlation.to_owned())) {
                    findings.push(json!({"schema_version":"fsl-domain-finding.v0","fsl":"fsl-domain-effect.v0","result":"violated","kind":"uncorrelated_async_completion","severity":"error","domain":domain.name,"failed_rule":"async_completion_correlated","guarantee_kind":"runtime_observed","witness":{"event_index":index,"effect":effect,"correlation_id":correlation}}));
                }
                steps += 1;
            }
            Some("command") => steps += 1,
            _ => {}
        }
    }
    wrap_specialized(
        json!({"result":if findings.is_empty(){"conformance_checked"}else{"nonconformant"},"dialect":"fsl-domain-effect.v0","finding_schema_version":"fsl-domain-finding.v0","domain":domain.name,"guarantee_kind":"runtime_observed","steps_checked":steps,"events_observed":observed,"pending_effects":pending.iter().map(|(effect,correlation)|json!({"effect":effect,"correlation_id":correlation})).collect::<Vec<_>>(),"findings":findings,"final_state":{},"assumptions":[]}),
    )
}

#[allow(clippy::too_many_lines)]
fn run_domain_testgen(
    path: &Path,
    depth: usize,
    target: &str,
    deadlock_mode: &str,
    strict: bool,
    output_path: Option<&Path>,
) -> (Value, i32) {
    let domain = match parse_domain_document(path) {
        Ok(domain) => domain,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let (generic, generic_status) = run_testgen(path, depth, target, deadlock_mode, strict, None);
    if generic_status == 2 {
        return (generic, generic_status);
    }
    let mut content = generic
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let mut display_names = Vec::new();
    for aggregate in &domain.aggregates {
        for field in &aggregate.state {
            display_names.push((
                format!("{}_{}", snake_case(&aggregate.name), field.name),
                format!("{}.{}", aggregate.name, field.name),
            ));
        }
        for decide in &aggregate.decides {
            display_names.push((
                format!(
                    "{}_{}",
                    snake_case(&aggregate.name),
                    snake_case(&decide.command)
                ),
                format!("{}.{}", aggregate.name, decide.command),
            ));
        }
        for event in &aggregate.events {
            display_names.push((
                format!("event_{}", event.name),
                format!("event.{}", event.name),
            ));
        }
    }
    for effect in &domain.effects {
        display_names.push((
            format!("{}_status", snake_case(&effect.name)),
            format!("effect.{}.status", effect.name),
        ));
        display_names.push((
            format!("{}_attempts", snake_case(&effect.name)),
            format!("effect.{}.attempts", effect.name),
        ));
        for event in &effect.outcomes {
            display_names.push((
                format!(
                    "{}_complete_{}",
                    snake_case(&effect.name),
                    snake_case(event)
                ),
                format!("{}.{}", effect.name, event),
            ));
        }
    }
    display_names.sort_by_key(|(internal, _)| std::cmp::Reverse(internal.len()));
    for (internal, public) in display_names {
        content = content.replace(&internal, &public);
    }
    if target == "vitest" {
        let mut prefix = "// Auto-generated fsl-domain conformance scaffold.\n// Wire makeAdapter() to the generated aggregate adapter or your implementation adapter.\n\n".to_owned();
        let (kernel, metadata) = match domain_scaffold_inputs(path, &domain) {
            Ok(input) => input,
            Err(error) => return (semantic_error_output(&error), 2),
        };
        let adapter_files = match fsl_tools::domain_adapter_files(&kernel, &metadata) {
            Ok(files) => files,
            Err(error) => return (semantic_error_output(&error), 2),
        };
        let adapter_count = adapter_files.len();
        for (index, (relative, source)) in adapter_files.into_iter().enumerate() {
            let _ = writeln!(prefix, "// --- scaffold: {relative} ---");
            for line in source.lines() {
                if line.is_empty() {
                    prefix.push_str("//\n");
                } else {
                    let _ = writeln!(prefix, "// {line}");
                }
            }
            if index + 1 < adapter_count {
                prefix.push('\n');
            }
        }
        content = prefix + &content;
    }
    let (mut result, status) = generated_content_result(
        "domain_testgen",
        &domain.name,
        format!(
            "{}_domain_test",
            path.file_stem()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or("domain")
        ),
        &content,
        output_path,
    );
    if let Value::Object(result) = &mut result {
        result.insert("dialect".to_owned(), json!("fsl-domain-effect.v0"));
        result.insert("domain".to_owned(), json!(domain.name));
        result.insert("target".to_owned(), json!(target));
        result.insert("depth".to_owned(), json!(depth));
        result.insert("warnings".to_owned(), json!([]));
    }
    (result, status)
}

fn type_ref_text(ty: &TypeRef) -> String {
    match ty {
        TypeRef::Int => "Int".to_owned(),
        TypeRef::Bool => "Bool".to_owned(),
        TypeRef::Named(name) => name.clone(),
        TypeRef::Range(lo, hi) => format!("{lo}..{hi}"),
        TypeRef::Map(key, value) => {
            format!("Map<{}, {}>", type_ref_text(key), type_ref_text(value))
        }
        TypeRef::Relation(left, right) => {
            format!(
                "relation {} -> {}",
                type_ref_text(left),
                type_ref_text(right)
            )
        }
        TypeRef::Set(value) => format!("Set<{}>", type_ref_text(value)),
        TypeRef::Seq(value, length) => format!("Seq<{}, {length}>", type_ref_text(value)),
        TypeRef::Option(value) => format!("Option<{}>", type_ref_text(value)),
    }
}

fn public_type(model: &KernelModel, ty: &TypeRef) -> Value {
    match ty {
        TypeRef::Int => json!(["int"]),
        TypeRef::Bool => json!(["bool"]),
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { lo, hi, .. }) => json!(["domain", lo, hi]),
            Some(TypeDef::Enum { .. }) => json!(["enum", fslc_rust::display_name(name)]),
            Some(TypeDef::Struct { .. }) => json!(["struct", fslc_rust::display_name(name)]),
            None => json!(["named", fslc_rust::display_name(name)]),
        },
        TypeRef::Range(lo, hi) => json!(["domain", lo, hi]),
        TypeRef::Map(key, value) => {
            json!(["map", public_type(model, key), public_type(model, value)])
        }
        TypeRef::Relation(left, right) => json!([
            "relation",
            public_type(model, left),
            public_type(model, right)
        ]),
        TypeRef::Set(value) => json!(["set", public_type(model, value)]),
        TypeRef::Seq(value, length) => json!(["seq", public_type(model, value), length]),
        TypeRef::Option(value) => json!(["option", public_type(model, value)]),
    }
}

fn statement_root(target: &KernelLValue) -> &str {
    match target {
        KernelLValue::Var(name) | KernelLValue::Index(name, _) => name,
        KernelLValue::Field(base, _) => statement_root(base),
    }
}

fn statement_writes(statements: &[KernelStatement]) -> Vec<String> {
    fn collect(statements: &[KernelStatement], writes: &mut std::collections::BTreeSet<String>) {
        for statement in statements {
            match statement {
                KernelStatement::Assign { target, .. } => {
                    writes.insert(fslc_rust::display_name(statement_root(target)));
                }
                KernelStatement::If {
                    then_statements,
                    else_statements,
                    ..
                } => {
                    collect(then_statements, writes);
                    collect(else_statements, writes);
                }
                KernelStatement::ForAll { statements, .. } => collect(statements, writes),
            }
        }
    }
    let mut writes = std::collections::BTreeSet::new();
    collect(statements, &mut writes);
    writes.into_iter().collect()
}

fn metadata(meta: Option<&fsl_syntax::MetaTag>) -> Value {
    meta.map_or(
        Value::Null,
        |meta| json!({"id": meta.id, "text": meta.text}),
    )
}

fn param_skeleton(model: &KernelModel, param: &ParamDef) -> Value {
    match param {
        ParamDef::Typed { name, ty } => {
            let (type_name, lo, hi) = match ty {
                TypeRef::Named(type_name) => match model.types.get(type_name) {
                    Some(TypeDef::Domain { lo, hi, .. }) => {
                        (fslc_rust::display_name(type_name), *lo, *hi)
                    }
                    Some(TypeDef::Enum { members, .. }) => (
                        fslc_rust::display_name(type_name),
                        0,
                        i64::try_from(members.len()).unwrap_or_default() - 1,
                    ),
                    _ => (fslc_rust::display_name(type_name), 0, 0),
                },
                TypeRef::Range(lo, hi) => ("Int".to_owned(), *lo, *hi),
                TypeRef::Bool => ("Bool".to_owned(), 0, 1),
                _ => (type_ref_text(ty), 0, 0),
            };
            json!({"name":name,"type":type_name,"lo":lo,"hi":hi})
        }
        ParamDef::Range { name, lo, hi } => {
            json!({"name":name,"type":"Int","lo":lo,"hi":hi})
        }
    }
}

fn map_key_domains(ty: &TypeRef, output: &mut std::collections::BTreeSet<String>) {
    match ty {
        TypeRef::Map(key, value) => {
            if let TypeRef::Named(name) = key.as_ref() {
                output.insert(name.clone());
            }
            map_key_domains(value, output);
        }
        TypeRef::Option(value) | TypeRef::Set(value) | TypeRef::Seq(value, _) => {
            map_key_domains(value, output);
        }
        _ => {}
    }
}

fn enum_value_type<'a>(model: &'a KernelModel, ty: &'a TypeRef) -> Option<&'a str> {
    match ty {
        TypeRef::Named(name) if matches!(model.types.get(name), Some(TypeDef::Enum { .. })) => {
            Some(name)
        }
        TypeRef::Map(_, value) | TypeRef::Option(value) | TypeRef::Set(value) => {
            enum_value_type(model, value)
        }
        _ => None,
    }
}

fn stage_read(expr: &KernelExpr, state: &str) -> bool {
    match expr {
        KernelExpr::Var(name) => name == state,
        KernelExpr::Index(base, _) => {
            matches!(base.as_ref(), KernelExpr::Var(name) if name == state)
        }
        _ => false,
    }
}

fn enum_member(expr: &KernelExpr, members: &[String]) -> Option<String> {
    let KernelExpr::Var(name) = expr else {
        return None;
    };
    members.contains(name).then(|| display(name))
}

fn model_stage_flows(model: &KernelModel) -> Vec<Value> {
    let mut flows = Vec::new();
    for (state, ty) in &model.state {
        let Some(type_name) = enum_value_type(model, ty) else {
            continue;
        };
        let Some(TypeDef::Enum { members, .. }) = model.types.get(type_name) else {
            continue;
        };
        let mut transitions = Vec::new();
        for action in &model.actions {
            let from = action.requires.iter().find_map(|expr| {
                let KernelExpr::Binary { op, left, right } = expr else {
                    return None;
                };
                if op != "==" {
                    return None;
                }
                if stage_read(left, state) {
                    enum_member(right, members)
                } else if stage_read(right, state) {
                    enum_member(left, members)
                } else {
                    None
                }
            });
            let to = action.statements.iter().find_map(|statement| {
                let KernelStatement::Assign { target, value, .. } = statement else {
                    return None;
                };
                (lvalue_root(target) == state)
                    .then(|| enum_member(value, members))
                    .flatten()
            });
            if let (Some(from), Some(to)) = (from, to) {
                transitions.push(json!({
                    "action": display(&action.name),
                    "from": from,
                    "to": to,
                }));
            }
        }
        if !transitions.is_empty() {
            flows.push(json!({
                "state": display(state),
                "type": display(type_name),
                "stages": members.iter().map(|member| display(member)).collect::<Vec<_>>(),
                "transitions": transitions,
            }));
        }
    }
    flows
}

#[allow(clippy::too_many_lines)]
fn model_skeleton(model: &KernelModel) -> Value {
    let actions = model
        .actions
        .iter()
        .map(|action| {
            let origin = model.action_origin(&action.name);
            let mut value = json!({
                "name": origin
                    .and_then(|origin| origin.primary.as_ref())
                    .and_then(|site| site.declaration_path.last())
                    .map_or_else(|| fslc_rust::display_name(&action.name), String::clone),
                "params": action.params.iter().map(|param|param_skeleton(model,param)).collect::<Vec<_>>(),
                "requires_text": action.requires.iter().map(|expr|format!("requires {}",fslc_rust::source_expr_text(model,expr))).collect::<Vec<_>>(),
                "ensures_text": action.ensures.iter().map(|expr|format!("ensures {}",fslc_rust::source_expr_text(model,expr))).collect::<Vec<_>>(),
                "writes": statement_writes(&action.statements), "requirement": Value::Null,
            });
            if let Value::Object(value) = &mut value {
                insert_requirement_metadata(value, &action.annotations, action.meta.as_ref());
            }
            if action.fair
                && let Value::Object(value) = &mut value
            {
                value.insert("fair".to_owned(), json!(true));
            }
            if let Some(origin) = origin
                && let Value::Object(value) = &mut value
            {
                value.insert(
                    "generated_name".to_owned(),
                    json!(fslc_rust::display_name(&action.name)),
                );
                value.insert(
                    "origin".to_owned(),
                    fslc_rust::internal_origin_json(origin),
                );
            }
            value
        })
        .collect::<Vec<_>>();
    let mut properties = Vec::new();
    for (kind, items) in [
        ("invariant", &model.invariants),
        ("trans", &model.transitions),
        ("reachable", &model.reachables),
    ] {
        properties.extend(items.iter().map(|property| {
            let origin = model.property_origin(kind, &property.name);
            let mut value = json!({
                "name": origin
                    .and_then(|origin| origin.primary.as_ref())
                    .and_then(|site| site.declaration_path.last())
                    .map_or_else(|| fslc_rust::display_name(&property.name), String::clone),
                "kind":kind,
                "body_text":fslc_rust::source_expr_text(model,&property.expr),
                "requirement":Value::Null
            });
            if let Value::Object(value) = &mut value {
                insert_requirement_metadata(value, &property.annotations, property.meta.as_ref());
            }
            if let Some(origin) = origin
                && let Value::Object(value) = &mut value
            {
                value.insert(
                    "generated_name".to_owned(),
                    json!(fslc_rust::display_name(&property.name)),
                );
                value.insert("origin".to_owned(), fslc_rust::internal_origin_json(origin));
            }
            value
        }));
    }
    for property in &model.leadstos {
        let mut value = json!({"name":fslc_rust::display_name(&property.name),"kind":"leadsTo","body_text":format!("{} ~> {}",fslc_rust::source_expr_text(model,&property.before),fslc_rust::source_expr_text(model,&property.after)),"requirement":Value::Null});
        if let Value::Object(value) = &mut value {
            insert_requirement_metadata(value, &property.annotations, property.meta.as_ref());
        }
        properties.push(value);
    }
    let mut entity_domains = std::collections::BTreeSet::new();
    for (_, ty) in &model.state {
        map_key_domains(ty, &mut entity_domains);
    }
    let domains = model
        .types
        .iter()
        .filter_map(|(name, ty)| match ty {
            TypeDef::Domain { lo, hi, .. } if !name.starts_with('_') => {
                let count = hi - lo + 1;
                Some(if entity_domains.contains(name) {
                    format!(
                        "{}: {count} instances ({lo}..{hi})",
                        fslc_rust::display_name(name)
                    )
                } else {
                    format!(
                        "{}: values {lo}..{hi} ({count} values)",
                        fslc_rust::display_name(name)
                    )
                })
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let auto_checks = model
        .state
        .iter()
        .filter(|(_, ty)| !matches!(ty, TypeRef::Int | TypeRef::Bool))
        .map(|(name, _)| {
            let target = fslc_rust::display_name(name);
            json!({"kind":"type_bound","name":format!("_bounds_{target}"),"target":target,"requirement":Value::Null})
        })
        .collect::<Vec<_>>();
    json!({
        "spec_kind":Value::Null,
        "state":model.state.iter().map(|(name,ty)|(fslc_rust::display_name(name),public_type(model,ty))).collect::<Map<_,_>>(),
        "actions":actions,"properties":properties,"auto_checks":auto_checks,
        "domains":domains,
        "enums":model.types.iter().filter_map(|(name,ty)| {
            if let TypeDef::Enum{members,..}=ty {
                Some((
                    fslc_rust::display_name(name),
                    json!(members.iter().map(|member|fslc_rust::display_name(member)).collect::<Vec<_>>()),
                ))
            } else {
                None
            }
        }).collect::<Map<_,_>>(),
        "stage_flows":model_stage_flows(model),
        "kpis":model.projections.iter().map(|projection| json!({
            "name":projection.name,
            "entity":projection.entity,
            "stage":projection.stage,
        })).collect::<Vec<_>>()
    })
}

#[allow(clippy::too_many_lines)]
fn explain_witnesses(model: &KernelModel, scenarios: &Value) -> Value {
    let mut witnesses = scenarios
        .get("scenarios")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|scenario| {
            let entry = scenario.as_object()?;
            let kind = entry.get("kind")?.clone();
            let target = [
                "property",
                "action",
                "final_check",
                "acceptance",
                "forbidden",
            ]
            .iter()
            .find_map(|key| entry.get(*key).filter(|value| !value.is_null()).cloned());
            let target_name = target.as_ref().and_then(Value::as_str);
            let requirements = entry
                .get("requirements")
                .and_then(Value::as_array)
                .cloned()
                .or_else(|| {
                    target_name.and_then(|name| {
                        model
                            .actions
                            .iter()
                            .find(|action| fslc_rust::display_name(&action.name) == name)
                            .map(|action| {
                                requirement_metadata(&action.annotations, action.meta.as_ref())
                            })
                            .or_else(|| {
                                model
                                    .reachables
                                    .iter()
                                    .chain(model.invariants.iter())
                                    .chain(model.transitions.iter())
                                    .find(|property| {
                                        fslc_rust::display_name(&property.name) == name
                                    })
                                    .map(|property| {
                                        requirement_metadata(
                                            &property.annotations,
                                            property.meta.as_ref(),
                                        )
                                    })
                            })
                    })
                })
                .unwrap_or_default();
            let requirement = entry
                .get("requirement")
                .filter(|requirement| !requirement.is_null())
                .cloned()
                .or_else(|| requirements.first().cloned())
                .unwrap_or(Value::Null);
            let steps = entry.get("steps").cloned().unwrap_or_else(|| json!([]));
            let narration = steps
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(index, step)| {
                    let action_name = step["action"].as_str().unwrap_or_default();
                    let params = model
                        .actions
                        .iter()
                        .find(|action| fslc_rust::display_name(&action.name) == action_name)
                        .map_or_else(Vec::new, |action| {
                            action
                                .params
                                .iter()
                                .filter_map(|param| {
                                    step["params"]
                                        .get(param.name())
                                        .map(|value| format!("{}={value}", param.name()))
                                })
                                .collect::<Vec<_>>()
                        });
                    if params.is_empty() {
                        format!("{}. {action_name}()", index + 1)
                    } else {
                        format!("{}. {action_name}({})", index + 1, params.join(", "))
                    }
                })
                .collect::<Vec<_>>();
            Some(json!({
                "name":entry.get("name"),
                "kind":kind,
                "target":target,
                "requirement":requirement,
                "requirements":requirements,
                "steps":steps,
                "narration":narration,
                "initial_state":entry.get("initial_state"),
                "expected_states":entry.get("expected_states").cloned().unwrap_or_else(||json!([])),
            }))
        })
        .collect::<Vec<_>>();
    let order = model
        .reachables
        .iter()
        .map(|property| format!("reach_{}", fslc_rust::display_name(&property.name)))
        .chain(
            model
                .actions
                .iter()
                .map(|action| format!("cover_{}", fslc_rust::display_name(&action.name))),
        )
        .enumerate()
        .map(|(index, name)| (name, index))
        .collect::<std::collections::BTreeMap<_, _>>();
    witnesses.sort_by_key(|witness| {
        witness["name"]
            .as_str()
            .and_then(|name| order.get(name))
            .copied()
            .unwrap_or(usize::MAX)
    });
    Value::Array(witnesses)
}

#[derive(Clone)]
struct WeakeningCandidate {
    spec: fsl_syntax::SurfaceSpec,
    op: &'static str,
    span: fsl_syntax::Span,
    target: String,
    origin: Option<&'static str>,
}

fn statement_removals(
    statements: &[KernelStatement],
) -> Vec<(Vec<KernelStatement>, fsl_syntax::Span)> {
    let mut output = Vec::new();
    for (index, statement) in statements.iter().enumerate() {
        match statement {
            KernelStatement::Assign { span, .. } => {
                let mut replacement = statements.to_vec();
                replacement.remove(index);
                output.push((replacement, *span));
            }
            KernelStatement::If {
                condition,
                then_statements,
                else_statements,
                span,
            } => {
                for (nested, nested_span) in statement_removals(then_statements) {
                    let mut replacement = statements.to_vec();
                    replacement[index] = KernelStatement::If {
                        condition: condition.clone(),
                        then_statements: nested,
                        else_statements: else_statements.clone(),
                        span: *span,
                    };
                    output.push((replacement, nested_span));
                }
                for (nested, nested_span) in statement_removals(else_statements) {
                    let mut replacement = statements.to_vec();
                    replacement[index] = KernelStatement::If {
                        condition: condition.clone(),
                        then_statements: then_statements.clone(),
                        else_statements: nested,
                        span: *span,
                    };
                    output.push((replacement, nested_span));
                }
            }
            KernelStatement::ForAll {
                binder,
                statements: body,
                span,
            } => {
                for (nested, nested_span) in statement_removals(body) {
                    let mut replacement = statements.to_vec();
                    replacement[index] = KernelStatement::ForAll {
                        binder: binder.clone(),
                        statements: nested,
                        span: *span,
                    };
                    output.push((replacement, nested_span));
                }
            }
        }
    }
    output
}

fn action_statement_removals(
    statement: &KernelStatement,
) -> Vec<(Option<KernelStatement>, fsl_syntax::Span)> {
    match statement {
        KernelStatement::Assign { span, .. } => vec![(None, *span)],
        KernelStatement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => {
            let mut output = Vec::new();
            for (nested, nested_span) in statement_removals(then_statements) {
                output.push((
                    Some(KernelStatement::If {
                        condition: condition.clone(),
                        then_statements: nested,
                        else_statements: else_statements.clone(),
                        span: *span,
                    }),
                    nested_span,
                ));
            }
            for (nested, nested_span) in statement_removals(else_statements) {
                output.push((
                    Some(KernelStatement::If {
                        condition: condition.clone(),
                        then_statements: then_statements.clone(),
                        else_statements: nested,
                        span: *span,
                    }),
                    nested_span,
                ));
            }
            output
        }
        KernelStatement::ForAll {
            binder,
            statements,
            span,
        } => statement_removals(statements)
            .into_iter()
            .map(|(statements, nested_span)| {
                (
                    Some(KernelStatement::ForAll {
                        binder: binder.clone(),
                        statements,
                        span: *span,
                    }),
                    nested_span,
                )
            })
            .collect(),
    }
}

fn weakening_candidates(spec: &fsl_syntax::SurfaceSpec) -> Vec<WeakeningCandidate> {
    let mut candidates = Vec::new();
    for (item_index, item) in spec.items.iter().enumerate() {
        match item {
            fsl_syntax::SpecItem::Init {
                statements,
                meta,
                annotations,
            } => {
                for (replacement, span) in statement_removals(statements) {
                    let mut mutated = spec.clone();
                    mutated.items[item_index] = fsl_syntax::SpecItem::Init {
                        statements: replacement,
                        meta: meta.clone(),
                        annotations: annotations.clone(),
                    };
                    candidates.push(WeakeningCandidate {
                        spec: mutated,
                        op: "assignment-removal",
                        span,
                        target: "init assignment".to_owned(),
                        origin: Some("init"),
                    });
                }
            }
            fsl_syntax::SpecItem::Action {
                name,
                items,
                span: action_span,
                fair,
                ..
            } => {
                let label = fslc_rust::display_name(name);
                let mut require_number = 0;
                for (part_index, part) in items.iter().enumerate() {
                    if let fsl_syntax::ActionItem::Requires(_, span) = part {
                        require_number += 1;
                        let mut mutated = spec.clone();
                        if let fsl_syntax::SpecItem::Action { items, .. } =
                            &mut mutated.items[item_index]
                        {
                            items.remove(part_index);
                        }
                        candidates.push(WeakeningCandidate {
                            spec: mutated,
                            op: "requires-removal",
                            span: *span,
                            target: format!("{label} requires #{require_number}"),
                            origin: None,
                        });
                    }
                }
                for (part_index, part) in items.iter().enumerate() {
                    let fsl_syntax::ActionItem::Statement(statement) = part else {
                        continue;
                    };
                    for (replacement, span) in action_statement_removals(statement) {
                        let mut mutated = spec.clone();
                        if let fsl_syntax::SpecItem::Action { items, .. } =
                            &mut mutated.items[item_index]
                        {
                            if let Some(replacement) = replacement {
                                items[part_index] = fsl_syntax::ActionItem::Statement(replacement);
                            } else {
                                items.remove(part_index);
                            }
                        }
                        candidates.push(WeakeningCandidate {
                            spec: mutated,
                            op: "assignment-removal",
                            span,
                            target: format!("{label} assignment"),
                            origin: None,
                        });
                    }
                }
                if *fair {
                    let mut mutated = spec.clone();
                    if let fsl_syntax::SpecItem::Action { fair, .. } =
                        &mut mutated.items[item_index]
                    {
                        *fair = false;
                    }
                    candidates.push(WeakeningCandidate {
                        spec: mutated,
                        op: "fair-removal",
                        span: *action_span,
                        target: format!("{label} fair"),
                        origin: None,
                    });
                }
            }
            _ => {}
        }
    }
    candidates
}

fn reachable_counterfactuals(path: &Path, depth: usize) -> Value {
    let source = std::fs::read_to_string(path).unwrap_or_default();
    let Ok(parsed) = parse_surface_document(path) else {
        return json!([]);
    };
    let document = match parsed {
        fsl_syntax::SurfaceDocument::Spec(spec) => spec,
        fsl_syntax::SurfaceDocument::Business(_)
        | fsl_syntax::SurfaceDocument::Requirements(_)
        | fsl_syntax::SurfaceDocument::Compose(_) => {
            let resolver =
                fsl_core::FsResolver::new(path.parent().unwrap_or_else(|| Path::new(".")));
            let Ok(kernel) = fsl_core::parse_kernel_source(&source, &resolver) else {
                return json!([]);
            };
            kernel.into_syntax()
        }
        _ => return json!([]),
    };
    let lines = source.lines().collect::<Vec<_>>();
    let mut output = Vec::new();
    for candidate in weakening_candidates(&document) {
        let Ok(model) = fsl_core::build_surface_model(candidate.spec) else {
            continue;
        };
        if fsl_runtime::find_boundary_violation(model.clone(), depth)
            .ok()
            .flatten()
            .is_some()
        {
            continue;
        }
        let Ok(mut solver) = fsl_solver_z3::Z3Solver::new() else {
            continue;
        };
        let Ok(result) = block_on_native(fsl_verifier::verify_bounded(&model, &mut solver, depth))
        else {
            continue;
        };
        if result.violation.is_some() || result.leadsto_violation.is_some() {
            continue;
        }
        let Some(property) = model.reachables.iter().find(|property| {
            result
                .reachables
                .get(&property.name)
                .is_some_and(Option::is_none)
        }) else {
            continue;
        };
        let line = usize::try_from(candidate.span.start.line).unwrap_or_default();
        let mut weakening = json!({
            "op":candidate.op,
            "loc":candidate.span.python_loc(),
            "target":candidate.target,
            "source_text":lines.get(line.saturating_sub(1)).map(|line|line.trim()),
        });
        if let Some(origin) = candidate.origin
            && let Value::Object(value) = &mut weakening
        {
            value.insert("origin".to_owned(), json!(origin));
            value.insert("label".to_owned(), json!("init weakening"));
        }
        let mut item = json!({
            "property":fslc_rust::display_name(&property.name),
            "weakening":weakening,
            "result":"reachable_failed",
            "requirement":Value::Null,
        });
        if let Value::Object(item) = &mut item {
            insert_requirement_metadata(item, &property.annotations, property.meta.as_ref());
        }
        output.push(item);
    }
    Value::Array(output)
}

#[allow(clippy::too_many_lines)]
fn expression_state_roots(
    model: &KernelModel,
    expr: &KernelExpr,
) -> std::collections::BTreeSet<String> {
    fn collect(expr: &KernelExpr, roots: &mut std::collections::BTreeSet<String>) {
        match expr {
            KernelExpr::Var(name) => {
                roots.insert(name.clone());
            }
            KernelExpr::Some(value)
            | KernelExpr::Neg(value)
            | KernelExpr::Not(value)
            | KernelExpr::Field(value, _)
            | KernelExpr::Stage { entity: value, .. }
            | KernelExpr::UnaryNamed { expr: value, .. } => collect(value, roots),
            KernelExpr::Index(base, index)
            | KernelExpr::BinaryNamed {
                left: base,
                right: index,
                ..
            }
            | KernelExpr::Binary {
                left: base,
                right: index,
                ..
            } => {
                collect(base, roots);
                collect(index, roots);
            }
            KernelExpr::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
            }
            | KernelExpr::TernaryNamed {
                first: condition,
                second: then_expr,
                third: else_expr,
                ..
            } => {
                collect(condition, roots);
                collect(then_expr, roots);
                collect(else_expr, roots);
            }
            KernelExpr::Set(items) | KernelExpr::Seq(items) => {
                for item in items {
                    collect(item, roots);
                }
            }
            KernelExpr::Struct { fields, .. } => {
                for (_, value) in fields {
                    collect(value, roots);
                }
            }
            KernelExpr::Call { args, .. } | KernelExpr::Method { args, .. } => {
                if let KernelExpr::Method { receiver, .. } = expr {
                    collect(receiver, roots);
                }
                for arg in args {
                    collect(arg, roots);
                }
            }
            KernelExpr::Is { expr, .. } => collect(expr, roots),
            KernelExpr::Quantified { binder, body, .. } => {
                collect_binder(binder, roots);
                collect(body, roots);
            }
            KernelExpr::Aggregate { binder, value, .. } => {
                collect_binder(binder, roots);
                if let Some(value) = value {
                    collect(value, roots);
                }
            }
            KernelExpr::Num(_) | KernelExpr::Bool(_) | KernelExpr::None => {}
        }
    }
    fn collect_binder(
        binder: &fsl_core::KernelBinder,
        roots: &mut std::collections::BTreeSet<String>,
    ) {
        match binder {
            fsl_core::KernelBinder::Typed { where_expr, .. } => {
                if let Some(expr) = where_expr {
                    collect(expr, roots);
                }
            }
            fsl_core::KernelBinder::Range {
                lo, hi, where_expr, ..
            } => {
                collect(lo, roots);
                collect(hi, roots);
                if let Some(expr) = where_expr {
                    collect(expr, roots);
                }
            }
            fsl_core::KernelBinder::Collection {
                collection,
                where_expr,
                ..
            } => {
                collect(collection, roots);
                if let Some(expr) = where_expr {
                    collect(expr, roots);
                }
            }
        }
    }
    let state = model
        .state
        .iter()
        .map(|(name, _)| name)
        .collect::<std::collections::BTreeSet<_>>();
    let mut roots = std::collections::BTreeSet::new();
    collect(expr, &mut roots);
    roots.retain(|name| state.contains(name));
    roots
}

fn executed_assignments<'a>(
    model: &KernelModel,
    statements: &'a [KernelStatement],
    state: &fsl_runtime::State,
    bindings: &mut fsl_runtime::Bindings,
) -> Vec<&'a KernelStatement> {
    let mut output = Vec::new();
    for statement in statements {
        match statement {
            KernelStatement::Assign { .. } => output.push(statement),
            KernelStatement::If {
                condition,
                then_statements,
                else_statements,
                ..
            } => {
                let branch = match fsl_runtime::eval(condition, state, bindings, model, None) {
                    Ok(FslValue::Bool(true)) => then_statements,
                    _ => else_statements,
                };
                output.extend(executed_assignments(model, branch, state, bindings));
            }
            KernelStatement::ForAll { statements, .. } => {
                output.extend(executed_assignments(model, statements, state, bindings));
            }
        }
    }
    output
}

fn trace_blame_json(
    model: &KernelModel,
    property: Option<&fsl_core::PropertyDef>,
    trace: &[fsl_core::TraceStep],
) -> Value {
    let mut read = property.map_or_else(std::collections::BTreeSet::new, |property| {
        expression_state_roots(model, &property.expr)
    });
    let mut by_step = std::collections::BTreeMap::<usize, Value>::new();
    for index in (1..trace.len()).rev() {
        let Some(action_trace) = trace[index].action.as_ref() else {
            continue;
        };
        let Some(action) = model
            .actions
            .iter()
            .find(|action| action.name == action_trace.name)
        else {
            continue;
        };
        let previous = &trace[index - 1].state;
        let mut bindings = action_trace.params.clone();
        for guard in &action.guards {
            if let fsl_core::ActionGuard::Let(name, expr) = guard
                && let Ok(value) = fsl_runtime::eval(expr, previous, &mut bindings, model, None)
            {
                bindings.insert(name.clone(), value);
            }
        }
        let mut effects = Vec::new();
        for statement in executed_assignments(model, &action.statements, previous, &mut bindings) {
            let KernelStatement::Assign {
                target,
                value,
                span,
            } = statement
            else {
                continue;
            };
            let root = lvalue_root(target);
            if !read.contains(root) {
                continue;
            }
            effects.push(json!({
                "target": display(root),
                "text": format!("{} = {}", tag_lvalue_text(target), fslc_rust::expr_text(value)),
                "loc": span.python_loc(),
            }));
            read.extend(expression_state_roots(model, value));
        }
        let guards = action
            .requires
            .iter()
            .zip(&action.require_spans)
            .filter(|(expr, _)| !expression_state_roots(model, expr).is_disjoint(&read))
            .map(|(expr, span)| {
                json!({"text": fslc_rust::expr_text(expr), "loc": span.python_loc()})
            })
            .collect::<Vec<_>>();
        by_step.insert(index, json!({"guards": guards, "effects": effects}));
    }
    Value::Object(
        by_step
            .into_iter()
            .map(|(step, blame)| (step.to_string(), blame))
            .collect(),
    )
}

fn canonical_concrete_violation_trace(
    model: &KernelModel,
    invariant: &str,
    steps: usize,
) -> Option<Vec<fsl_core::TraceStep>> {
    let initial = fsl_runtime::Monitor::new(model.clone()).ok()?;
    let initial_trace = vec![fsl_core::TraceStep {
        step: 0,
        state: initial.state.clone(),
        action: None,
        changes: std::collections::BTreeMap::new(),
    }];
    let mut queue = std::collections::VecDeque::from([(initial.clone(), initial_trace, 0_usize)]);
    let mut visited = std::collections::BTreeSet::from([initial.state.clone()]);
    while let Some((monitor, trace, step)) = queue.pop_front() {
        if step >= steps {
            continue;
        }
        for enabled in monitor.enabled().ok()? {
            let mut child = monitor.clone();
            let result = child.step(&enabled).ok()?;
            let mut child_trace = trace.clone();
            child_trace.push(fsl_core::TraceStep {
                step: step + 1,
                state: result.state,
                action: Some(fsl_core::TraceAction {
                    name: enabled.action,
                    params: enabled.params,
                }),
                changes: std::collections::BTreeMap::new(),
            });
            if let Some(violation) = result.violation {
                if violation.name == invariant && violation.step == steps {
                    return Some(child_trace);
                }
                continue;
            }
            if visited.insert(child.state.clone()) {
                queue.push_back((child, child_trace, step + 1));
            }
        }
    }
    None
}

fn canonicalize_initial_int_violation(
    model: &KernelModel,
    violation: &mut fsl_verifier::BmcViolation,
    removed_root: &str,
) {
    if violation.step != 0
        || !model
            .state
            .iter()
            .any(|(name, ty)| name == removed_root && matches!(ty, TypeRef::Int))
    {
        return;
    }
    let Some(entry) = violation.trace.first_mut() else {
        return;
    };
    for candidate in [-1_i64, 0, 1, -2, 2, -3, 3] {
        let mut state = entry.state.clone();
        state.insert(removed_root.to_owned(), FslValue::Int(candidate));
        let first_failed = model.invariants.iter().find_map(|property| {
            match fsl_runtime::eval(
                &property.expr,
                &state,
                &mut fsl_runtime::Bindings::new(),
                model,
                None,
            ) {
                Ok(FslValue::Bool(false)) => Some(property.name.as_str()),
                _ => None,
            }
        });
        if first_failed == Some(violation.name.as_str()) {
            entry.state = state;
            return;
        }
    }
}

fn invariant_violation_explanation(
    model: &KernelModel,
    violation: &fsl_verifier::BmcViolation,
    started: Instant,
) -> (Value, Value) {
    let property = model
        .invariants
        .iter()
        .find(|property| property.name == violation.name);
    let final_state = violation.trace.last().map(|entry| &entry.state);
    let violating = violation_bindings_json(
        model,
        violation.kind.as_str(),
        violation.name.as_str(),
        property.map(|property| &property.expr),
        final_state,
    );
    let trace_blame = trace_blame_json(model, property, &violation.trace);
    let mut trace = fslc_rust::trace_json(model, &violation.trace);
    if let Value::Array(entries) = &mut trace {
        for (index, entry) in entries.iter_mut().enumerate().skip(1) {
            if let Value::Object(entry) = entry {
                entry.insert(
                    "blame".to_owned(),
                    trace_blame
                        .get(index.to_string())
                        .cloned()
                        .unwrap_or_else(|| json!({"guards": [], "effects": []})),
                );
            }
        }
    }
    let last_action = violation
        .trace
        .last()
        .and_then(|entry| entry.action.as_ref())
        .map_or(Value::Null, |action| {
            let definition = model
                .actions
                .iter()
                .find(|definition| definition.name == action.name);
            let origin = model.action_origin(&action.name);
            let mut value = json!({
                "name": origin
                    .and_then(fslc_rust::origin_display_name)
                    .map_or_else(|| display(&action.name), str::to_owned),
                "params": action.params.iter().map(|(name, value)| (
                    name.clone(), fslc_rust::fsl_value_json(value)
                )).collect::<Map<_, _>>(),
                "loc": definition.map(|definition| definition.span.python_loc()),
            });
            if let Some(origin) = origin
                && let Value::Object(value) = &mut value
            {
                value.insert("generated_name".to_owned(), json!(display(&action.name)));
                value.insert("origin".to_owned(), fslc_rust::internal_origin_json(origin));
                if let Some(span) = origin.primary.as_ref().and_then(|site| site.span) {
                    value.insert("loc".to_owned(), span.python_loc());
                }
            }
            value
        });
    let mut explanation = Map::new();
    explanation.insert("violation_kind".to_owned(), json!(violation.kind));
    let origin = model.property_origin("invariant", &violation.name);
    explanation.insert(
        "invariant".to_owned(),
        json!(
            origin
                .and_then(fslc_rust::origin_display_name)
                .map_or_else(|| display(&violation.name), str::to_owned)
        ),
    );
    explanation.insert(
        "loc".to_owned(),
        origin
            .and_then(|origin| origin.primary.as_ref())
            .and_then(|site| site.span)
            .map_or_else(
                || property.map_or(Value::Null, |property| property.span.python_loc()),
                fsl_syntax::Span::python_loc,
            ),
    );
    if let Some(origin) = origin {
        explanation.insert("generated_name".to_owned(), json!(display(&violation.name)));
        explanation.insert("origin".to_owned(), fslc_rust::internal_origin_json(origin));
    }
    explanation.insert("violated_at_step".to_owned(), json!(violation.step));
    explanation.insert("violating_bindings".to_owned(), violating.clone());
    explanation.insert(
        "blame".to_owned(),
        violation_blame_json(
            model,
            violation.kind.as_str(),
            violation.name.as_str(),
            property.map(|property| &property.expr),
            violating,
        ),
    );
    explanation.insert("last_action".to_owned(), last_action);
    finish(&mut explanation, violation.step, started);
    (trace, Value::Object(explanation))
}

#[allow(clippy::too_many_lines)]
fn invariant_counterfactuals(path: &Path, depth: usize) -> Value {
    let source = std::fs::read_to_string(path).unwrap_or_default();
    let Ok(parsed) = parse_surface_document(path) else {
        return json!([]);
    };
    let document = match parsed {
        fsl_syntax::SurfaceDocument::Spec(spec) => spec,
        fsl_syntax::SurfaceDocument::Business(_)
        | fsl_syntax::SurfaceDocument::Requirements(_)
        | fsl_syntax::SurfaceDocument::Compose(_) => {
            let resolver =
                fsl_core::FsResolver::new(path.parent().unwrap_or_else(|| Path::new(".")));
            let Ok(kernel) = fsl_core::parse_kernel_source(&source, &resolver) else {
                return json!([]);
            };
            kernel.into_syntax()
        }
        _ => return json!([]),
    };
    let Ok(original) = fsl_core::build_surface_model(document.clone()) else {
        return json!([]);
    };
    let lines = source.lines().collect::<Vec<_>>();
    let mut found = std::collections::BTreeMap::<String, ((usize, usize, u32), Value)>::new();
    for candidate in fsl_tools::enumerate_builtin_mutants(&document)
        .into_iter()
        .filter(|candidate| {
            matches!(
                candidate.op.as_str(),
                "assignment_remove" | "requires_remove" | "fair_remove"
            )
        })
    {
        let Some(span) = candidate.span else {
            continue;
        };
        let removed_init_root = (candidate.op == "assignment_remove"
            && candidate.action.as_deref() == Some("init"))
        .then(|| removed_init_assignment_root(&document, &candidate.spec))
        .flatten();
        if removed_init_root
            .as_ref()
            .and_then(|root| original.state.iter().find(|(name, _)| name == root))
            .is_some_and(|(_, ty)| type_has_symbolic_bounds(&original, ty))
        {
            continue;
        }
        let started = Instant::now();
        let Ok(model) = fsl_core::build_surface_model(candidate.spec) else {
            continue;
        };
        let Ok(mut solver) = fsl_solver_z3::Z3Solver::new() else {
            continue;
        };
        let Ok(result) = block_on_native(fsl_verifier::verify_bounded(&model, &mut solver, depth))
        else {
            continue;
        };
        let Some(violation) = result.violation.as_ref() else {
            continue;
        };
        if !original
            .invariants
            .iter()
            .any(|property| property.name == violation.name)
        {
            continue;
        }
        let mut canonical_violation = violation.clone();
        if let Some(trace) = canonical_concrete_violation_trace(
            &model,
            &violation.name,
            violation.trace.len().saturating_sub(1),
        ) {
            canonical_violation.trace = trace;
        } else if let Some(root) = removed_init_root.as_deref() {
            canonicalize_initial_int_violation(&model, &mut canonical_violation, root);
        }
        let (trace, explanation) =
            invariant_violation_explanation(&model, &canonical_violation, started);
        let line = usize::try_from(span.start.line).unwrap_or_default();
        let op = match candidate.op.as_str() {
            "assignment_remove" => "assignment-removal",
            "requires_remove" => "requires-removal",
            "fair_remove" => "fair-removal",
            _ => unreachable!("filtered weakening operator"),
        };
        let mut weakening = json!({
            "op": op,
            "loc": span.python_loc(),
            "target": candidate.target,
            "source_text": lines.get(line.saturating_sub(1)).map(|line| line.trim()),
        });
        if candidate.action.as_deref() == Some("init")
            && let Value::Object(value) = &mut weakening
        {
            value.insert("origin".to_owned(), json!("init"));
            value.insert("label".to_owned(), json!("init weakening"));
        }
        let priority = match op {
            "assignment-removal" => 0,
            "requires-removal" => 1,
            "fair-removal" => 2,
            _ => 99,
        };
        let key = (canonical_violation.trace.len(), priority, span.start.line);
        let property = original
            .invariants
            .iter()
            .find(|property| property.name == violation.name);
        let mut item = json!({
            "invariant": display(&violation.name),
            "weakening": weakening,
            "trace": trace,
            "requirement": Value::Null,
            "violation": explanation,
        });
        if let Some(property) = property
            && let Value::Object(item) = &mut item
        {
            insert_requirement_metadata(item, &property.annotations, property.meta.as_ref());
        }
        if found
            .get(&violation.name)
            .is_none_or(|(current, _)| key < *current)
        {
            found.insert(violation.name.clone(), (key, item));
        }
    }
    Value::Array(
        original
            .invariants
            .iter()
            .map(|property| {
                found.get(&property.name).map_or_else(
                    || {
                        let mut item = json!({
                            "invariant": display(&property.name),
                            "weakening": null,
                            "trace": null,
                            "requirement": Value::Null,
                            "note": format!("no counterfactual within depth {depth}"),
                        });
                        if let Value::Object(item) = &mut item {
                            insert_requirement_metadata(
                                item,
                                &property.annotations,
                                property.meta.as_ref(),
                            );
                        }
                        item
                    },
                    |(_, item)| item.clone(),
                )
            })
            .collect(),
    )
}

#[allow(clippy::too_many_lines)]
fn run_explain(path: &Path, depth: usize, readable: bool) -> (Value, i32) {
    let model = match load_model(path) {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let skeleton = model_skeleton(&model);
    let (scenarios, _) = run_scenarios(path, depth, "warn");
    let mut output = envelope();
    output.insert("result".to_owned(), json!("explained"));
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("depth".to_owned(), json!(depth));
    output.insert("skeleton".to_owned(), skeleton);
    output.insert(
        "witnesses".to_owned(),
        explain_witnesses(&model, &scenarios),
    );
    output.insert(
        "counterfactuals".to_owned(),
        invariant_counterfactuals(path, depth),
    );
    let mut reachable_counterfactuals = reachable_counterfactuals(path, depth);
    if let Value::Array(items) = &mut reachable_counterfactuals {
        for item in items {
            if let Value::Object(item) = item {
                item.insert("faithfulness_class".to_owned(), json!("intent_unexercised"));
                item.insert(
                    "recommended_action".to_owned(),
                    json!("add a single-shot reachable for the action / raise --depth"),
                );
            }
        }
    }
    if reachable_counterfactuals
        .as_array()
        .is_some_and(|items| !items.is_empty())
    {
        output.insert(
            "reachable_counterfactuals".to_owned(),
            reachable_counterfactuals,
        );
    }
    if readable {
        let mut text = format!("Spec: {} (depth {depth})\n", model.name);
        let domains = model_skeleton(&model)
            .get("domains")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if !domains.is_empty() {
            text.push_str("\nVerification world:\n");
            for domain in domains.iter().filter_map(Value::as_str) {
                let _ = writeln!(text, "  - {domain}");
            }
        }
        text.push_str("\nState:\n");
        let mut state = model.state.iter().collect::<Vec<_>>();
        state.sort_by_key(|(name, _)| display(name));
        for (name, ty) in state {
            let _ = writeln!(text, "  - {}: {}", display(name), type_ref_text(ty));
        }
        text.push_str("\nActions:\n");
        for action in &model.actions {
            let params = action
                .params
                .iter()
                .map(|param| match param {
                    ParamDef::Typed { name, ty } => format!("{name}: {}", type_ref_text(ty)),
                    ParamDef::Range { name, lo, hi } => format!("{name}: {lo}..{hi}"),
                })
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(
                text,
                "  - {}({params}){}",
                display(&action.name),
                if action.fair { " [fair]" } else { "" },
            );
            for requirement in action
                .annotations
                .requirements()
                .expect("checked model annotations are valid")
            {
                let _ = writeln!(
                    text,
                    "    requirement: {}{}",
                    requirement.id,
                    requirement
                        .text
                        .as_ref()
                        .map_or_else(String::new, |value| format!(": {value}"))
                );
            }
            if !action.requires.is_empty() {
                text.push_str("    requires:\n");
                for requirement in &action.requires {
                    let _ = writeln!(
                        text,
                        "      - requires {}",
                        fslc_rust::expr_text(requirement)
                    );
                }
            }
            let writes = statement_writes(&action.statements);
            if !writes.is_empty() {
                let _ = writeln!(text, "    writes: {}", writes.join(", "));
            }
        }
        let mut properties = Vec::new();
        for (kind, declarations) in [
            ("invariant", &model.invariants),
            ("trans", &model.transitions),
            ("reachable", &model.reachables),
        ] {
            for property in declarations {
                properties.push((kind, &property.name, fslc_rust::expr_text(&property.expr)));
            }
        }
        for property in &model.leadstos {
            properties.push((
                "leadsTo",
                &property.name,
                format!(
                    "{} ~> {}",
                    fslc_rust::expr_text(&property.before),
                    fslc_rust::expr_text(&property.after)
                ),
            ));
        }
        if !properties.is_empty() {
            text.push_str("\nProperties:\n");
            for (kind, name, body) in properties {
                let _ = writeln!(text, "  - {kind} {}", display(name));
                let _ = writeln!(text, "    body: {body}");
            }
        }
        let checks = model
            .state
            .iter()
            .filter(|(_, ty)| !matches!(ty, TypeRef::Int | TypeRef::Bool))
            .collect::<Vec<_>>();
        if !checks.is_empty() {
            text.push_str("\nAutomatic checks:\n");
            for (name, _) in checks {
                let _ = writeln!(
                    text,
                    "  - type_bound: {} (implicit bounded-domain check)",
                    display(name)
                );
            }
        }
        text.pop();
        output.insert("readable".to_owned(), json!(text));
    }
    (Value::Object(output), 0)
}

#[allow(dead_code)]
fn expression_mutant_count(
    expr: &KernelExpr,
    enum_siblings: &std::collections::BTreeMap<String, usize>,
) -> usize {
    match expr {
        KernelExpr::Num(_) => 2,
        KernelExpr::Var(name) => enum_siblings.get(name).copied().unwrap_or_default(),
        KernelExpr::Some(value)
        | KernelExpr::Neg(value)
        | KernelExpr::Not(value)
        | KernelExpr::Field(value, _)
        | KernelExpr::Stage { entity: value, .. }
        | KernelExpr::UnaryNamed { expr: value, .. } => {
            expression_mutant_count(value, enum_siblings)
        }
        KernelExpr::Index(base, index)
        | KernelExpr::BinaryNamed {
            left: base,
            right: index,
            ..
        } => {
            expression_mutant_count(base, enum_siblings)
                + expression_mutant_count(index, enum_siblings)
        }
        KernelExpr::Binary { left, right, .. } => {
            expression_mutant_count(left, enum_siblings)
                + expression_mutant_count(right, enum_siblings)
        }
        KernelExpr::Method { receiver, args, .. } => {
            expression_mutant_count(receiver, enum_siblings)
                + args
                    .iter()
                    .map(|arg| expression_mutant_count(arg, enum_siblings))
                    .sum::<usize>()
        }
        KernelExpr::Is { expr, .. } => expression_mutant_count(expr, enum_siblings),
        KernelExpr::Set(values) | KernelExpr::Seq(values) => values
            .iter()
            .map(|value| expression_mutant_count(value, enum_siblings))
            .sum(),
        KernelExpr::Struct { fields, .. } => fields
            .iter()
            .map(|(_, value)| expression_mutant_count(value, enum_siblings))
            .sum(),
        KernelExpr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => [condition.as_ref(), then_expr, else_expr]
            .into_iter()
            .map(|value| expression_mutant_count(value, enum_siblings))
            .sum(),
        KernelExpr::Call { args, .. } => args
            .iter()
            .map(|arg| expression_mutant_count(arg, enum_siblings))
            .sum(),
        KernelExpr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => [first.as_ref(), second, third]
            .into_iter()
            .map(|value| expression_mutant_count(value, enum_siblings))
            .sum(),
        KernelExpr::Bool(_)
        | KernelExpr::None
        | KernelExpr::Quantified { .. }
        | KernelExpr::Aggregate { .. } => 0,
    }
}

#[allow(dead_code)]
fn statement_mutant_count(
    statement: &KernelStatement,
    enum_siblings: &std::collections::BTreeMap<String, usize>,
) -> usize {
    match statement {
        KernelStatement::Assign { target, value, .. } => {
            let target_count = match target {
                KernelLValue::Index(_, index) => expression_mutant_count(index, enum_siblings),
                KernelLValue::Field(base, _) => match base.as_ref() {
                    KernelLValue::Index(_, index) => expression_mutant_count(index, enum_siblings),
                    _ => 0,
                },
                KernelLValue::Var(_) => 0,
            };
            1 + target_count + expression_mutant_count(value, enum_siblings)
        }
        KernelStatement::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => {
            usize::from(!then_statements.is_empty() && !else_statements.is_empty())
                + expression_mutant_count(condition, enum_siblings)
                + then_statements
                    .iter()
                    .map(|item| statement_mutant_count(item, enum_siblings))
                    .sum::<usize>()
                + else_statements
                    .iter()
                    .map(|item| statement_mutant_count(item, enum_siblings))
                    .sum::<usize>()
        }
        KernelStatement::ForAll { statements, .. } => statements
            .iter()
            .map(|item| statement_mutant_count(item, enum_siblings))
            .sum(),
    }
}

#[allow(dead_code)]
fn builtin_mutant_count(spec: &fsl_syntax::SurfaceSpec) -> usize {
    let mut enum_siblings = std::collections::BTreeMap::new();
    for item in &spec.items {
        if let fsl_syntax::SpecItem::Enum { members, .. } = item {
            for member in members {
                enum_siblings.insert(member.clone(), members.len().saturating_sub(1));
            }
        }
    }
    spec.items
        .iter()
        .map(|item| match item {
            fsl_syntax::SpecItem::Type { lo, hi, .. } => {
                2 * usize::from(matches!(lo.as_ref(), KernelExpr::Num(_)))
                    + 2 * usize::from(matches!(hi.as_ref(), KernelExpr::Num(_)))
            }
            fsl_syntax::SpecItem::Const { value, .. } => {
                expression_mutant_count(value, &enum_siblings)
            }
            fsl_syntax::SpecItem::Init { statements, .. } => statements
                .iter()
                .map(|statement| statement_mutant_count(statement, &enum_siblings))
                .sum(),
            fsl_syntax::SpecItem::Action { items, fair, .. } => {
                usize::from(*fair)
                    + items
                        .iter()
                        .map(|item| match item {
                            fsl_syntax::ActionItem::Requires(expr, _) => {
                                2 + expression_mutant_count(expr, &enum_siblings)
                            }
                            fsl_syntax::ActionItem::Let(_, expr, _) => {
                                expression_mutant_count(expr, &enum_siblings)
                            }
                            fsl_syntax::ActionItem::Statement(statement) => {
                                statement_mutant_count(statement, &enum_siblings)
                            }
                            fsl_syntax::ActionItem::Ensures(_, _) => 0,
                        })
                        .sum::<usize>()
            }
            _ => 0,
        })
        .sum()
}

#[allow(dead_code)]
fn run_mutate_legacy(
    path: &Path,
    depth: usize,
    max_mutants: usize,
    by_requirement: bool,
) -> (Value, i32) {
    let (baseline, status) = run_verify(path, depth, "warn", "bmc", DEFAULT_EXPLICIT_BUDGET, 1);
    if status == 2
        || !matches!(
            baseline.get("result").and_then(Value::as_str),
            Some("verified" | "reachable_failed")
        )
    {
        return (baseline, 0);
    }
    let model = match load_model(path) {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error), 0),
    };
    let mut mutants = Vec::new();
    let mut discovered = None;
    if let Ok(fsl_syntax::SurfaceDocument::Spec(spec)) = parse_surface_document(path) {
        discovered = Some(builtin_mutant_count(&spec));
        for item in &spec.items {
            if let fsl_syntax::SpecItem::Type { name, lo, hi, .. } = item {
                for (bound, expr) in [("lo", lo.as_ref()), ("hi", hi.as_ref())] {
                    if matches!(expr, KernelExpr::Num(_)) {
                        for suffix in ["minus1", "plus1"] {
                            mutants.push(json!({
                                "op":format!("type_bound_{bound}_{suffix}"),
                                "loc":Value::Null,
                                "target":format!("type {name} {bound}"),
                                "status":"survived",
                                "killed_by":Value::Null,
                                "requirement":Value::Null,
                                "source":"builtin",
                            }));
                        }
                    }
                }
            }
        }
    }
    for action in &model.actions {
        for (index, _) in action.requires.iter().enumerate() {
            let mut mutant = json!({"op":"requires_remove","target":format!("{} requires #{}",fslc_rust::display_name(&action.name),index+1),"status":"survived","loc":action.require_spans.get(index).map(|span|span.python_loc()),"killed_by":Value::Null,"requirement":Value::Null,"source":"builtin"});
            if let Value::Object(mutant) = &mut mutant {
                insert_requirement_metadata(mutant, &action.annotations, action.meta.as_ref());
            }
            mutants.push(mutant);
        }
        if !action.statements.is_empty() {
            let mut mutant = json!({"op":"assignment_remove","target":format!("{} assignment",fslc_rust::display_name(&action.name)),"status":"survived","loc":action.span.python_loc(),"killed_by":Value::Null,"requirement":Value::Null,"source":"builtin"});
            if let Value::Object(mutant) = &mut mutant {
                insert_requirement_metadata(mutant, &action.annotations, action.meta.as_ref());
            }
            mutants.push(mutant);
        }
    }
    let discovered = discovered.unwrap_or(mutants.len());
    mutants.truncate(max_mutants);
    let mut output = envelope();
    output.insert("result".to_owned(), json!("mutated"));
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("depth".to_owned(), json!(depth));
    output.insert("baseline".to_owned(), json!("verified"));
    output.insert("mutants".to_owned(), Value::Array(mutants.clone()));
    output.insert(
        "summary".to_owned(),
        json!({
            "total":mutants.len(),"killed":0,"survived":mutants.len(),"invalid":0,
            "kill_rate":if mutants.is_empty(){Value::Null}else{json!(0.0)},
            "by_source":{
                "builtin":{"total":mutants.len(),"killed":0,"survived":mutants.len(),"invalid":0,"kill_rate":if mutants.is_empty(){Value::Null}else{json!(0.0)}},
                "external":{"total":0,"killed":0,"survived":0,"invalid":0,"kill_rate":Value::Null}
            }
        }),
    );
    output.insert("by_requirement".to_owned(), json!({}));
    let mut notes = vec!["possible equivalent mutants should be reviewed manually; survivors are a review queue, not a hard failure".to_owned()];
    if discovered > max_mutants {
        notes.push(format!(
            "mutant cap {max_mutants} reached: {} dropped",
            discovered - max_mutants
        ));
    }
    if by_requirement {
        // The empty index is exact for specifications without requirement metadata.
    }
    output.insert("notes".to_owned(), json!(notes));
    (Value::Object(output), 0)
}

struct MutationOracle {
    clean: bool,
    killed_by: Option<String>,
    killer_requirements: Vec<String>,
}

fn annotation_requirement_ids(annotations: &Annotations) -> Vec<String> {
    annotations
        .requirements()
        .expect("checked model annotations are valid")
        .into_iter()
        .map(|requirement| requirement.id)
        .collect()
}

fn property_requirements(model: &KernelModel, name: &str) -> Vec<String> {
    model
        .invariants
        .iter()
        .chain(&model.transitions)
        .chain(&model.reachables)
        .find(|property| property.name == name)
        .map(|property| annotation_requirement_ids(&property.annotations))
        .or_else(|| {
            model
                .leadstos
                .iter()
                .find(|property| property.name == name)
                .map(|property| annotation_requirement_ids(&property.annotations))
        })
        .unwrap_or_default()
}

fn mutation_model_oracle(mut model: KernelModel, depth: usize) -> MutationOracle {
    loop {
        let Ok(mut solver) = fsl_solver_z3::Z3Solver::new() else {
            return MutationOracle {
                clean: false,
                killed_by: Some("internal".to_owned()),
                killer_requirements: Vec::new(),
            };
        };
        let Ok(result) = block_on_native(fsl_verifier::verify_bounded(&model, &mut solver, depth))
        else {
            return MutationOracle {
                clean: false,
                killed_by: Some("build_spec".to_owned()),
                killer_requirements: Vec::new(),
            };
        };
        if let Some(violation) = result.violation {
            if violation.kind == "ensures"
                && fsl_runtime::replay_trace(model.clone(), &violation.trace).is_err()
                && let Some(action) = model
                    .actions
                    .iter_mut()
                    .find(|action| action.name == violation.name)
                && !action.ensures.is_empty()
            {
                action.ensures.clear();
                action.ensure_spans.clear();
                continue;
            }
            return MutationOracle {
                clean: false,
                killed_by: Some(display(&violation.name)),
                killer_requirements: property_requirements(&model, &violation.name),
            };
        }
        if let Some(property) = model.reachables.iter().find(|property| {
            result
                .reachables
                .get(&property.name)
                .is_some_and(Option::is_none)
        }) {
            return MutationOracle {
                clean: false,
                killed_by: Some(display(&property.name)),
                killer_requirements: annotation_requirement_ids(&property.annotations),
            };
        }
        if let Some(violation) = result.leadsto_violation {
            return MutationOracle {
                clean: false,
                killed_by: Some(display(&violation.name)),
                killer_requirements: property_requirements(&model, &violation.name),
            };
        }
        break;
    }
    MutationOracle {
        clean: true,
        killed_by: None,
        killer_requirements: Vec::new(),
    }
}

fn mutation_oracle(spec: fsl_syntax::SurfaceSpec, depth: usize) -> MutationOracle {
    let Ok(kernel) = fsl_core::lower_direct_spec(spec) else {
        return MutationOracle {
            clean: false,
            killed_by: Some("build_spec".to_owned()),
            killer_requirements: Vec::new(),
        };
    };
    let Ok(model) = fsl_core::build_model(kernel) else {
        return MutationOracle {
            clean: false,
            killed_by: Some("build_spec".to_owned()),
            killer_requirements: Vec::new(),
        };
    };
    mutation_oracle_for_model(model, depth)
}

fn mutation_oracle_for_model(model: KernelModel, depth: usize) -> MutationOracle {
    if let Ok(Some((violation, _))) = fsl_runtime::find_boundary_violation(model.clone(), depth) {
        return MutationOracle {
            clean: false,
            killed_by: Some(violation.name.clone()),
            killer_requirements: property_requirements(&model, &violation.name),
        };
    }
    let mut automatic = model.clone();
    automatic.invariants.clear();
    automatic.transitions.clear();
    automatic.reachables.clear();
    automatic.leadstos.clear();
    let automatic_result = mutation_model_oracle(automatic, depth);
    if !automatic_result.clean {
        return automatic_result;
    }
    mutation_model_oracle(model, depth)
}

fn apply_requirement_mutation_oracle(
    source: &str,
    model: &KernelModel,
    outcome: &mut MutationOracle,
) -> Result<(), String> {
    if !outcome.clean {
        return Ok(());
    }
    if let (Some(failure), _) = validate_requirement_trace_source(source, model)? {
        let kind = failure
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("acceptance");
        *outcome = MutationOracle {
            clean: false,
            killed_by: Some(
                if kind.starts_with("forbidden") {
                    "forbidden"
                } else {
                    "acceptance"
                }
                .to_owned(),
            ),
            killer_requirements: Vec::new(),
        };
    }
    Ok(())
}

fn apply_implements_mutation_oracle(
    source: &str,
    base: &Path,
    model: &KernelModel,
    depth: usize,
    outcome: &mut MutationOracle,
) -> Result<(), String> {
    if !outcome.clean {
        return Ok(());
    }
    let resolver = fsl_core::FsResolver::new(base);
    let Some(contract) = fsl_core::requirements_implements(source, &resolver, model)
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    let checked =
        fsl_runtime::check_refinement(model, &contract.abstraction, &contract.refinement, depth)
            .map_err(|error| error.to_string())?;
    if let Some(failure) = checked.failure {
        let killer_requirements = failure
            .impl_action
            .as_ref()
            .and_then(|instance| {
                model
                    .actions
                    .iter()
                    .find(|action| action.name == instance.name)
            })
            .map_or_else(Vec::new, |action| {
                annotation_requirement_ids(&action.annotations)
            });
        *outcome = MutationOracle {
            clean: false,
            killed_by: Some("refinement".to_owned()),
            killer_requirements,
        };
    }
    Ok(())
}

#[allow(clippy::cast_precision_loss)]
fn mutation_kill_rate(killed: usize, survived: usize) -> Value {
    let judged = killed + survived;
    if judged == 0 {
        Value::Null
    } else {
        let rate = killed as f64 / judged as f64;
        json!((rate * 10_000.0).round() / 10_000.0)
    }
}

fn mutation_summary(mutants: &[Value]) -> Value {
    let summarize = |source: &str| {
        let entries = mutants
            .iter()
            .filter(|item| item["source"].as_str() == Some(source))
            .collect::<Vec<_>>();
        let killed = entries
            .iter()
            .filter(|item| item["status"].as_str() == Some("killed"))
            .count();
        let survived = entries
            .iter()
            .filter(|item| item["status"].as_str() == Some("survived"))
            .count();
        let invalid = entries
            .iter()
            .filter(|item| item["status"].as_str() == Some("invalid"))
            .count();
        json!({"total":entries.len(),"killed":killed,"survived":survived,"invalid":invalid,"kill_rate":mutation_kill_rate(killed,survived)})
    };
    let builtin = summarize("builtin");
    let external = summarize("external");
    let killed = mutants
        .iter()
        .filter(|item| item["status"].as_str() == Some("killed"))
        .count();
    let survived = mutants
        .iter()
        .filter(|item| item["status"].as_str() == Some("survived"))
        .count();
    let invalid = mutants
        .iter()
        .filter(|item| item["status"].as_str() == Some("invalid"))
        .count();
    json!({"total":mutants.len(),"killed":killed,"survived":survived,"invalid":invalid,"kill_rate":mutation_kill_rate(killed,survived),"by_source":{"builtin":builtin,"external":external}})
}

fn requirement_kill_index(model: &KernelModel) -> Map<String, Value> {
    let mut result = Map::new();
    for annotations in model
        .actions
        .iter()
        .map(|item| &item.annotations)
        .chain(
            model
                .invariants
                .iter()
                .chain(&model.transitions)
                .chain(&model.reachables)
                .map(|item| &item.annotations),
        )
        .chain(model.leadstos.iter().map(|item| &item.annotations))
    {
        for requirement in annotation_requirement_ids(annotations) {
            result.entry(requirement).or_insert(json!({"kills":0}));
        }
    }
    result
}

fn assignment_root_from_source(line: &str, column: u32) -> Option<String> {
    let offset = usize::try_from(column).ok()?.saturating_sub(1);
    let statement = line.get(offset..).unwrap_or(line);
    let left = statement.split('=').next()?.trim();
    let boundary = left.find(['[', '.']).unwrap_or(left.len());
    let prefix = &left[..boundary];
    prefix
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .rfind(|part| !part.is_empty())
        .map(str::to_owned)
}

fn lvalue_root(target: &KernelLValue) -> &str {
    match target {
        KernelLValue::Var(name) | KernelLValue::Index(name, _) => name,
        KernelLValue::Field(base, _) => lvalue_root(base),
    }
}

fn collect_assignment_roots(
    statements: &[KernelStatement],
    roots: &mut std::collections::BTreeMap<String, usize>,
) {
    for statement in statements {
        match statement {
            KernelStatement::Assign { target, .. } => {
                *roots.entry(lvalue_root(target).to_owned()).or_default() += 1;
            }
            KernelStatement::If {
                then_statements,
                else_statements,
                ..
            } => {
                collect_assignment_roots(then_statements, roots);
                collect_assignment_roots(else_statements, roots);
            }
            KernelStatement::ForAll { statements, .. } => {
                collect_assignment_roots(statements, roots);
            }
        }
    }
}

fn init_assignment_roots(
    spec: &fsl_syntax::SurfaceSpec,
) -> std::collections::BTreeMap<String, usize> {
    let mut roots = std::collections::BTreeMap::new();
    for item in &spec.items {
        if let fsl_syntax::SpecItem::Init { statements, .. } = item {
            collect_assignment_roots(statements, &mut roots);
        }
    }
    roots
}

fn removed_init_assignment_root(
    original: &fsl_syntax::SurfaceSpec,
    mutated: &fsl_syntax::SurfaceSpec,
) -> Option<String> {
    let original = init_assignment_roots(original);
    let mutated = init_assignment_roots(mutated);
    original.into_iter().find_map(|(root, count)| {
        (mutated.get(&root).copied().unwrap_or_default() < count).then_some(root)
    })
}

fn type_has_symbolic_bounds(model: &KernelModel, ty: &TypeRef) -> bool {
    match ty {
        TypeRef::Int | TypeRef::Bool => false,
        TypeRef::Range(..) | TypeRef::Set(_) | TypeRef::Seq(..) | TypeRef::Relation(..) => true,
        TypeRef::Option(inner) => type_has_symbolic_bounds(model, inner),
        TypeRef::Map(_, value) => type_has_symbolic_bounds(model, value),
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { .. } | TypeDef::Enum { .. }) => true,
            Some(TypeDef::Struct { fields }) => fields
                .iter()
                .any(|(_, field)| type_has_symbolic_bounds(model, field)),
            None => false,
        },
    }
}

fn mutation_action_labels(
    document: &fsl_syntax::SurfaceDocument,
) -> std::collections::BTreeMap<String, String> {
    fn collect(
        action: &fsl_syntax::RequirementAction,
        labels: &mut std::collections::BTreeMap<String, String>,
    ) {
        for item in &action.items {
            if let fsl_syntax::RequirementActionItem::Branches { branches, .. } = item {
                for (index, branch) in branches.iter().enumerate() {
                    labels.insert(
                        format!("{}__b{}", action.name, index + 1),
                        format!(
                            "{}[{}]",
                            action.name,
                            fslc_rust::expr_text(&branch.condition)
                        ),
                    );
                }
            }
        }
    }
    let mut labels = std::collections::BTreeMap::new();
    if let fsl_syntax::SurfaceDocument::Requirements(requirements) = document {
        for item in &requirements.items {
            match item {
                fsl_syntax::RequirementsItem::Requirement { items, .. } => {
                    for item in items {
                        if let fsl_syntax::RequirementBlockItem::Action(action) = item {
                            collect(action, &mut labels);
                        }
                    }
                }
                fsl_syntax::RequirementsItem::Action(action) => collect(action, &mut labels),
                _ => {}
            }
        }
    }
    labels
}

struct ExternalMutation {
    id: String,
    op: String,
    target: String,
    requirement: Value,
    input_kind: Option<String>,
    line: usize,
    source: Option<String>,
    invalid: Option<Value>,
}

fn invalid_mutation_detail(kind: &str, message: impl Into<String>, loc: Option<Value>) -> Value {
    let mut detail = json!({"kind":kind,"message":message.into()});
    if let Some(loc) = loc
        && let Value::Object(detail) = &mut detail
    {
        detail.insert("loc".to_owned(), loc);
    }
    detail
}

fn external_invalid(line: usize, id: String, invalid: Value) -> ExternalMutation {
    ExternalMutation {
        id: id.clone(),
        op: "external".to_owned(),
        target: id,
        requirement: Value::Null,
        input_kind: None,
        line,
        source: None,
        invalid: Some(invalid),
    }
}

fn replace_external_source(
    source: &str,
    instruction: &Map<String, Value>,
) -> Result<String, String> {
    let target = instruction
        .get("target")
        .and_then(Value::as_str)
        .filter(|target| !target.is_empty())
        .ok_or_else(|| "replace.target must be a non-empty string".to_owned())?;
    let replacement = instruction
        .get("replacement")
        .and_then(Value::as_str)
        .ok_or_else(|| "replace.replacement must be a string".to_owned())?;
    let mut starts = Vec::new();
    let mut position = 0;
    while let Some(offset) = source[position..].find(target) {
        let start = position + offset;
        starts.push(start);
        position = start + target.len();
    }
    let selected = match instruction.get("occurrence") {
        None | Some(Value::Null) => {
            if starts.len() != 1 {
                return Err(format!(
                    "replace.target must match exactly once without occurrence; matched {} times",
                    starts.len()
                ));
            }
            0
        }
        Some(value) => {
            let occurrence = value.as_u64().ok_or_else(|| {
                "replace.occurrence must be a positive 1-based integer".to_owned()
            })?;
            if occurrence == 0 {
                return Err("replace.occurrence must be a positive 1-based integer".to_owned());
            }
            let occurrence = usize::try_from(occurrence)
                .map_err(|_| "replace.occurrence is too large".to_owned())?;
            if occurrence > starts.len() {
                return Err(format!(
                    "replace.occurrence {occurrence} exceeds {} match(es)",
                    starts.len()
                ));
            }
            occurrence - 1
        }
    };
    let start = starts[selected];
    Ok(format!(
        "{}{}{}",
        &source[..start],
        replacement,
        &source[start + target.len()..]
    ))
}

#[allow(clippy::too_many_lines)]
fn load_external_mutations(path: &Path, baseline: &str) -> Result<Vec<ExternalMutation>, String> {
    let raw = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let mut output = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (index, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let line_number = index + 1;
        let fallback_id = format!("external:{line_number}");
        let value = match serde_json::from_str::<Value>(line) {
            Ok(value) => value,
            Err(error) => {
                let message = if error.to_string().starts_with("key must be a string") {
                    format!(
                        "Expecting property name enclosed in double quotes: line {} column {} (char {})",
                        error.line(),
                        error.column(),
                        error.column().saturating_sub(1)
                    )
                } else {
                    error.to_string()
                };
                output.push(external_invalid(
                    line_number,
                    fallback_id,
                    invalid_mutation_detail("json", message, None),
                ));
                continue;
            }
        };
        let Value::Object(record) = value else {
            output.push(external_invalid(
                line_number,
                fallback_id,
                invalid_mutation_detail("shape", "each JSONL line must be an object", None),
            ));
            continue;
        };
        let id = record
            .get("id")
            .and_then(Value::as_str)
            .map_or_else(|| fallback_id.clone(), str::to_owned);
        if id.trim().is_empty() || record.get("id").is_some_and(|id| !id.is_string()) {
            output.push(external_invalid(
                line_number,
                fallback_id,
                invalid_mutation_detail("shape", "id must be a non-empty string", None),
            ));
            continue;
        }
        if !seen.insert(id.clone()) {
            output.push(external_invalid(
                line_number,
                id.clone(),
                invalid_mutation_detail(
                    "shape",
                    format!("duplicate external mutant id '{id}'"),
                    None,
                ),
            ));
            continue;
        }
        let full_keys = ["mutated_spec", "spec"]
            .into_iter()
            .filter(|key| record.contains_key(*key))
            .collect::<Vec<_>>();
        let nested_replace = record.contains_key("replace");
        let flat_replace = record.contains_key("target") || record.contains_key("replacement");
        let modes = full_keys.len() + usize::from(nested_replace || flat_replace);
        let (source, input_kind) = if modes != 1 {
            (None, None)
        } else if let Some(key) = full_keys.first() {
            match record.get(*key).and_then(Value::as_str) {
                Some(source) => (Some(source.to_owned()), Some("full_spec".to_owned())),
                None => (None, Some(format!("{key} must be a string"))),
            }
        } else {
            let instruction = if nested_replace {
                record.get("replace").and_then(Value::as_object).cloned()
            } else {
                Some(
                    ["target", "replacement", "occurrence"]
                        .into_iter()
                        .filter_map(|key| {
                            record
                                .get(key)
                                .cloned()
                                .map(|value| (key.to_owned(), value))
                        })
                        .collect(),
                )
            };
            match instruction {
                Some(instruction) => match replace_external_source(baseline, &instruction) {
                    Ok(source) => (Some(source), Some("replacement".to_owned())),
                    Err(error) => (None, Some(error)),
                },
                None => (None, Some("replace must be an object".to_owned())),
            }
        };
        let invalid_message = if modes != 1 {
            Some("provide exactly one mutation form: mutated_spec/spec or replace".to_owned())
        } else if source.is_none() {
            input_kind.clone()
        } else {
            None
        };
        output.push(ExternalMutation {
            id: id.clone(),
            op: record
                .get("op")
                .and_then(Value::as_str)
                .unwrap_or("external")
                .to_owned(),
            target: record
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_owned(),
            requirement: record.get("requirement").cloned().unwrap_or(Value::Null),
            input_kind: source.as_ref().map(|_| input_kind.unwrap_or_default()),
            line: line_number,
            source,
            invalid: invalid_message
                .map(|message| invalid_mutation_detail("instruction", message, None)),
        });
    }
    Ok(output)
}

fn external_mutant_public(
    candidate: &ExternalMutation,
    status: &str,
    killed_by: Option<&str>,
    invalid: Option<Value>,
) -> Value {
    let mut output = json!({
        "id":candidate.id,
        "op":candidate.op,
        "loc":Value::Null,
        "target":candidate.target,
        "status":status,
        "killed_by":killed_by,
        "requirement":candidate.requirement,
        "source":"external",
        "input_kind":candidate.input_kind,
        "line":candidate.line,
    });
    if let Some(invalid) = invalid
        && let Value::Object(output) = &mut output
    {
        output.insert("invalid".to_owned(), invalid);
    }
    output
}

fn surface_document_name(document: &fsl_syntax::SurfaceDocument) -> Option<&str> {
    match document {
        fsl_syntax::SurfaceDocument::Spec(document) => Some(&document.name),
        fsl_syntax::SurfaceDocument::Business(document) => Some(&document.name),
        fsl_syntax::SurfaceDocument::Requirements(document) => Some(&document.name),
        fsl_syntax::SurfaceDocument::Compose(document) => Some(&document.name),
        fsl_syntax::SurfaceDocument::Agent(document) => Some(&document.name),
        _ => None,
    }
}

fn external_mutation_model(
    source: &str,
    base: &Path,
    expected_name: &str,
) -> Result<KernelModel, Value> {
    let document = fsl_syntax::parse_surface_document(source).map_err(|error| {
        if error.span.start.offset >= source.len() {
            invalid_mutation_detail("parse", "Unexpected end-of-input. Expected one of: ", None)
        } else {
            invalid_mutation_detail("parse", error.message, Some(error.span.python_loc()))
        }
    })?;
    let Some(name) = surface_document_name(&document) else {
        return Err(invalid_mutation_detail(
            "semantics",
            "external mutant must be a spec-like FSL file",
            None,
        ));
    };
    if name != expected_name {
        return Err(invalid_mutation_detail(
            "spec_name",
            format!("external mutant spec name '{name}' does not match baseline '{expected_name}'"),
            None,
        ));
    }
    let resolver = fsl_core::FsResolver::new(base);
    let kernel = fsl_core::parse_kernel_source(source, &resolver).map_err(|error| {
        let message = error.to_string();
        let kind = if message.contains("unknown type") {
            "type"
        } else {
            "semantics"
        };
        invalid_mutation_detail(kind, message, None)
    })?;
    fsl_core::build_model(kernel).map_err(|error| {
        let message = error.to_string();
        let kind = if message.contains("unknown type") || message.contains("type mismatch") {
            "type"
        } else {
            "semantics"
        };
        invalid_mutation_detail(kind, message, None)
    })
}

#[allow(clippy::too_many_lines)]
fn run_mutate(
    path: &Path,
    depth: usize,
    max_mutants: usize,
    by_requirement: bool,
    external_mutants: Option<&Path>,
) -> (Value, i32) {
    let (baseline, status) = run_verify(path, depth, "warn", "bmc", DEFAULT_EXPLICIT_BUDGET, 1);
    if status != 0 || baseline.get("result").and_then(Value::as_str) != Some("verified") {
        return (baseline, 0);
    }
    let model = match load_model(path) {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error), 0),
    };
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => return (error_output("io", &error.to_string()), 0),
    };
    let document = match parse_surface_document(path) {
        Ok(document) => document,
        Err(error) => return (semantic_error_output(&error), 0),
    };
    let action_labels = mutation_action_labels(&document);
    let spec = match document {
        fsl_syntax::SurfaceDocument::Spec(spec) => spec,
        fsl_syntax::SurfaceDocument::Business(_)
        | fsl_syntax::SurfaceDocument::Requirements(_)
        | fsl_syntax::SurfaceDocument::Compose(_) => {
            let resolver =
                fsl_core::FsResolver::new(path.parent().unwrap_or_else(|| Path::new(".")));
            match fsl_core::parse_kernel_source(&source, &resolver) {
                Ok(kernel) => kernel.into_syntax(),
                Err(error) => return (semantic_error_output(&error.to_string()), 0),
            }
        }
        _ => {
            return (
                error_output("semantics", "mutate expects a spec-like FSL file"),
                0,
            );
        }
    };
    let all_mutants = fsl_tools::enumerate_builtin_mutants(&spec);
    let discovered = all_mutants.len();
    let dead_actions = baseline
        .get("action_coverage")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(_, covered)| **covered != Value::Bool(true))
        .map(|(name, _)| name.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut by_req = if by_requirement {
        requirement_kill_index(&model)
    } else {
        Map::new()
    };
    let mut public_mutants = Vec::new();
    let source_lines = source.lines().collect::<Vec<_>>();
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    for mutant in all_mutants.into_iter().take(max_mutants) {
        let mutated_spec = mutant.spec.clone();
        let mut outcome = mutation_oracle(mutant.spec, depth);
        if outcome.clean
            && let Ok(kernel) = fsl_core::lower_direct_spec(mutated_spec.clone())
            && let Ok(mutated_model) = fsl_core::build_model(kernel)
        {
            if let Err(error) =
                apply_requirement_mutation_oracle(&source, &mutated_model, &mut outcome)
            {
                outcome = MutationOracle {
                    clean: false,
                    killed_by: Some(error),
                    killer_requirements: Vec::new(),
                };
            } else if apply_implements_mutation_oracle(
                &source,
                base,
                &mutated_model,
                depth,
                &mut outcome,
            )
            .is_err()
            {
                outcome = MutationOracle {
                    clean: false,
                    killed_by: Some("refinement".to_owned()),
                    killer_requirements: Vec::new(),
                };
            }
        }
        if mutant.op == "assignment_remove"
            && mutant.action.as_deref() == Some("init")
            && let Some(span) = mutant.span
            && let Some(root) = source_lines
                .get(
                    usize::try_from(span.start.line)
                        .unwrap_or_default()
                        .saturating_sub(1),
                )
                .and_then(|line| assignment_root_from_source(line, span.start.column))
                .filter(|root| model.state.iter().any(|(name, _)| name == root))
                .or_else(|| removed_init_assignment_root(&spec, &mutated_spec))
            && let Some((_, ty)) = model.state.iter().find(|(name, _)| name == &root)
            && type_has_symbolic_bounds(&model, ty)
        {
            outcome = MutationOracle {
                clean: false,
                killed_by: Some(format!("_bounds_{root}")),
                killer_requirements: Vec::new(),
            };
        }
        let status = if outcome.clean { "survived" } else { "killed" };
        let target = mutant
            .action
            .as_ref()
            .and_then(|action| action_labels.get(action).map(|label| (action, label)))
            .map_or_else(
                || mutant.target.clone(),
                |(action, label)| mutant.target.replacen(action, label, 1),
            );
        let mut public = json!({
            "op":mutant.op,
            "loc":mutant.span.map(fsl_syntax::Span::python_loc),
            "target":target,
            "status":status,
            "killed_by":outcome.killed_by,
            "requirement":metadata(mutant.requirement.as_ref()),
            "source":"builtin",
        });
        let annotations = mutant.action.as_deref().and_then(|name| {
            if name == "init" {
                Some(&model.init_annotations)
            } else {
                model
                    .actions
                    .iter()
                    .find(|action| action.name == name)
                    .map(|action| &action.annotations)
            }
        });
        if let Some(annotations) = annotations
            && let Value::Object(public) = &mut public
        {
            insert_requirement_metadata(public, annotations, mutant.requirement.as_ref());
        }
        if outcome.clean
            && mutant
                .action
                .as_ref()
                .is_some_and(|action| dead_actions.contains(action))
            && let Value::Object(public) = &mut public
        {
            public.insert(
                "note".to_owned(),
                json!("action dead at baseline — survival expected"),
            );
        }
        for requirement in outcome.killer_requirements {
            if let Some(Value::Object(entry)) = by_req.get_mut(&requirement) {
                let kills = entry
                    .get("kills")
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                entry.insert("kills".to_owned(), json!(kills + 1));
            }
        }
        public_mutants.push(public);
    }
    if let Some(external_path) = external_mutants {
        let candidates = match load_external_mutations(external_path, &source) {
            Ok(candidates) => candidates,
            Err(error) => return (error_output("io", &error), 0),
        };
        for candidate in candidates {
            if let Some(invalid) = candidate.invalid.clone() {
                public_mutants.push(external_mutant_public(
                    &candidate,
                    "invalid",
                    None,
                    Some(invalid),
                ));
                continue;
            }
            let Some(mutated_source) = candidate.source.as_deref() else {
                public_mutants.push(external_mutant_public(
                    &candidate,
                    "invalid",
                    None,
                    Some(invalid_mutation_detail(
                        "instruction",
                        "external mutation has no source",
                        None,
                    )),
                ));
                continue;
            };
            let mutated_model = match external_mutation_model(mutated_source, base, &model.name) {
                Ok(model) => model,
                Err(invalid) => {
                    public_mutants.push(external_mutant_public(
                        &candidate,
                        "invalid",
                        None,
                        Some(invalid),
                    ));
                    continue;
                }
            };
            let mut outcome = mutation_oracle_for_model(mutated_model.clone(), depth);
            if let Err(error) =
                apply_requirement_mutation_oracle(mutated_source, &mutated_model, &mut outcome)
                    .and_then(|()| {
                        apply_implements_mutation_oracle(
                            mutated_source,
                            base,
                            &mutated_model,
                            depth,
                            &mut outcome,
                        )
                    })
            {
                public_mutants.push(external_mutant_public(
                    &candidate,
                    "invalid",
                    None,
                    Some(invalid_mutation_detail("semantics", error, None)),
                ));
                continue;
            }
            if outcome.clean {
                public_mutants.push(external_mutant_public(&candidate, "survived", None, None));
            } else if outcome.killed_by.as_deref() == Some("build_spec") {
                public_mutants.push(external_mutant_public(
                    &candidate,
                    "invalid",
                    None,
                    Some(invalid_mutation_detail(
                        "semantics",
                        "invalid external mutant",
                        None,
                    )),
                ));
            } else {
                for requirement in &outcome.killer_requirements {
                    if let Some(Value::Object(entry)) = by_req.get_mut(requirement) {
                        let kills = entry
                            .get("kills")
                            .and_then(Value::as_u64)
                            .unwrap_or_default();
                        entry.insert("kills".to_owned(), json!(kills + 1));
                    }
                }
                public_mutants.push(external_mutant_public(
                    &candidate,
                    "killed",
                    outcome.killed_by.as_deref(),
                    None,
                ));
            }
        }
    }
    let mut notes = vec![
        "possible equivalent mutants should be reviewed manually; survivors are a review queue, not a hard failure".to_owned(),
    ];
    if discovered > max_mutants {
        notes.push(format!(
            "mutant cap {max_mutants} reached: {} dropped",
            discovered - max_mutants
        ));
    }
    if by_requirement {
        for value in by_req.values_mut() {
            if value.get("kills").and_then(Value::as_u64) == Some(0)
                && let Value::Object(value) = value
            {
                value.insert("warning".to_owned(), json!("empty_formalization"));
            }
        }
        notes.push(
            "by_requirement kills are an observed lower bound within this mutant set and depth"
                .to_owned(),
        );
    }
    if external_mutants.is_some() {
        notes.push(
            "invalid external mutants are generation-quality findings and are excluded from kill-rate denominators"
                .to_owned(),
        );
    }
    let mut output = envelope();
    output.insert("result".to_owned(), json!("mutated"));
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("depth".to_owned(), json!(depth));
    output.insert("baseline".to_owned(), json!("verified"));
    output.insert("mutants".to_owned(), Value::Array(public_mutants.clone()));
    output.insert("summary".to_owned(), mutation_summary(&public_mutants));
    output.insert("by_requirement".to_owned(), Value::Object(by_req));
    output.insert("notes".to_owned(), json!(notes));
    (Value::Object(output), 0)
}

fn run_typestate(path: &Path) -> (Value, i32) {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let resolver = fsl_core::FsResolver::new(base);
    let kernel = match fsl_core::parse_kernel_source(&source, &resolver) {
        Ok(kernel) => kernel,
        Err(error) => return (semantic_error_output(&error.to_string()), 2),
    };
    let model = match fsl_core::build_model(kernel.clone()) {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error.to_string()), 2),
    };
    let path_text = path.to_string_lossy();
    let contract = match fsl_core::public_kernel_contract(
        &kernel,
        &model,
        &path_text,
        source_dialect(&source),
    ) {
        Ok(contract) => contract,
        Err(error) => return (semantic_error_output(&error.to_string()), 2),
    };
    let report = match fsl_tools::analyze_typestate(&contract) {
        Ok(report) => report,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let mut output = envelope();
    if let Value::Object(report) = report {
        output.extend(report);
    }
    (Value::Object(output), 0)
}

fn generated_content_result(
    kind: &str,
    spec: &str,
    default_output: String,
    content: &str,
    output_path: Option<&Path>,
) -> (Value, i32) {
    if let Some(path) = output_path
        && let Err(error) = std::fs::write(path, content)
    {
        return (error_output("io", &error.to_string()), 2);
    }
    let mut output = envelope();
    output.insert("result".to_owned(), json!("generated"));
    output.insert("kind".to_owned(), json!(kind));
    output.insert("spec".to_owned(), json!(spec));
    output.insert(
        "output".to_owned(),
        json!(output_path.map_or_else(|| PathBuf::from(default_output), Path::to_path_buf)),
    );
    if output_path.is_none() {
        output.insert("content".to_owned(), json!(content));
    }
    (Value::Object(output), 0)
}

fn run_testgen(
    path: &Path,
    depth: usize,
    target: &str,
    deadlock_mode: &str,
    strict: bool,
    output_path: Option<&Path>,
) -> (Value, i32) {
    let (source, kernel, model) = match load_kernel_model(path) {
        Ok(parts) => parts,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let (scenarios, status) = run_scenarios_mode(path, depth, deadlock_mode, !strict);
    if status == 2 {
        return (scenarios, status);
    }
    let walk = match fslc_rust::testgen_trace_vectors(&model) {
        Ok(walk) => walk,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let input = if source_dialect(&source) == "compose" {
        fsl_tools::compose_testgen_input(
            &model.name,
            path,
            output_path,
            model.state.iter().map(|(name, _)| name.clone()).collect(),
            model
                .actions
                .iter()
                .map(|action| {
                    (
                        action.name.clone(),
                        action
                            .params
                            .iter()
                            .map(|param| param.name().to_owned())
                            .collect(),
                    )
                })
                .collect(),
            &scenarios,
            &walk,
        )
    } else {
        fsl_core::public_kernel_contract(
            &kernel,
            &model,
            &path.to_string_lossy(),
            source_dialect(&source),
        )
        .map_err(|error| error.to_string())
        .and_then(|contract| {
            fsl_tools::public_kernel_testgen_input(&contract, path, output_path, &scenarios, &walk)
        })
    };
    let input = match input {
        Ok(input) => input,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let content = match fsl_tools::generate_testgen(&input, target) {
        Ok(content) => content,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let extension = match target {
        "vitest" => "test.ts",
        "swift" => "swift",
        "kotlin" => "kt",
        "dart" => "dart",
        "phpunit" => "php",
        "pytest" => "py",
        _ => return (semantic_error_output("unknown testgen target"), 2),
    };
    let (mut result, status) = generated_content_result(
        "testgen",
        input.spec_name(),
        format!(
            "test_{}.{}",
            path.file_stem()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or("spec"),
            extension
        ),
        &content,
        output_path,
    );
    if let Value::Object(result) = &mut result {
        result.remove("kind");
        result.insert("target".to_owned(), json!(target));
        if let Some(warnings) = scenarios.get("warnings")
            && warnings.as_array().is_some_and(|items| !items.is_empty())
        {
            result.insert("warnings".to_owned(), warnings.clone());
        }
    }
    (result, status)
}

fn run_html_report(
    path: &Path,
    depth: usize,
    deadlock_mode: &str,
    engine: &str,
    output_path: Option<&Path>,
) -> (Value, i32) {
    let model = match load_model(path) {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    let (verification, _) = run_verify(
        path,
        depth,
        deadlock_mode,
        engine,
        DEFAULT_EXPLICIT_BUDGET,
        1,
    );
    let (mut explained, explain_status) = run_explain(path, depth, false);
    if explain_status != 0 {
        return (explained, explain_status);
    }
    if let Value::Object(explained) = &mut explained {
        explained.remove("fsl");
        if let Some(Value::Array(items)) = explained.get_mut("reachable_counterfactuals") {
            for item in items {
                if let Value::Object(item) = item {
                    item.remove("faithfulness_class");
                    item.remove("recommended_action");
                }
            }
        }
    }
    let html = fsl_tools::render_html_report(
        &path.display().to_string(),
        &source,
        &explained,
        &verification,
        &fsl_tools::undecided_declarations(&model),
    );
    generated_content_result(
        "html_report",
        &model.name,
        format!(
            "{}.html",
            path.file_stem()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or("report")
        ),
        &html,
        output_path,
    )
}

fn load_file_input(path: &Path) -> Result<approval::FileInput, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    Ok(approval::FileInput {
        path: path.display().to_string(),
        digest: approval::sha256_bytes(&bytes),
    })
}

/// Build `--kind requirements_document`'s recorded reproducibility inputs
/// (issue #333): `view`/`lang` are read from the reviewed artifact's own
/// frontmatter (no new `--view`/`--lang` flags on `approval create`, mirroring
/// how `fslc document check` already reads them back rather than trusting a
/// possibly-inconsistent flag), while `--glossary`/`--evidence` are hashed
/// the same way `fslc document generate` hashes them.
fn document_generation_inputs(
    artifact_path: &Path,
    glossary_path: Option<&Path>,
    evidence_paths: &[PathBuf],
) -> Result<approval::GenerationInputs, String> {
    let artifact_text =
        std::fs::read_to_string(artifact_path).map_err(|error| error.to_string())?;
    let frontmatter =
        fsl_tools::parse_frontmatter_only(&artifact_text).map_err(|error| error.to_string())?;
    if frontmatter.view != "requirements" {
        return Err(format!("unsupported document view '{}'", frontmatter.view));
    }
    let glossary = glossary_path.map(load_file_input).transpose()?;
    let evidence = evidence_paths
        .iter()
        .map(|path| load_file_input(path))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(approval::GenerationInputs::Document(
        approval::DocumentGenerationInputs {
            view: "requirements".to_owned(),
            lang: frontmatter.lang,
            glossary,
            evidence,
        },
    ))
}

/// Re-render a `requirements` document deterministically from `(spec,
/// inputs)` alone, with no `--strict` gate and no approval overlay — used by
/// both `fslc approval create --kind requirements_document` (to build the
/// canonical rendering a reviewed artifact must conform to) and `fslc
/// approval check`/`fslc document generate --approval` (to reproduce the
/// same rendering's digest at check time). Returns the claims (needed for
/// `fsl_tools::check_requirements_document`'s conformance gate at create
/// time) alongside the canonical Markdown.
fn render_document_for_approval(
    path: &Path,
    inputs: &approval::DocumentGenerationInputs,
) -> Result<(fsl_tools::RequirementClaimSet, String), Value> {
    let locale = fsl_tools::Locale::parse(&inputs.lang).ok_or_else(|| {
        error_output(
            "document",
            &format!("unsupported document lang '{}'", inputs.lang),
        )
    })?;
    let (source, claims) = load_document_claims(path)?;
    let loaded_glossary = load_glossary(
        inputs
            .glossary
            .as_ref()
            .map(|file| Path::new(file.path.as_str())),
        locale,
    )?;
    let evidence_paths: Vec<PathBuf> = inputs
        .evidence
        .iter()
        .map(|file| PathBuf::from(&file.path))
        .collect();
    let loaded_evidence = load_evidence(&evidence_paths)?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let resolver = fsl_core::FsResolver::new(base);
    let kernel = fsl_core::parse_kernel_source(&source, &resolver)
        .map_err(|error| semantic_error_output(&error.to_string()))?;
    let model = fsl_core::build_model(kernel.clone())
        .map_err(|error| semantic_error_output(&error.to_string()))?;
    let applied_glossary = loaded_glossary
        .as_ref()
        .map(|(glossary, digest)| fsl_tools::AppliedGlossary { glossary, digest });
    let applied_evidence = loaded_evidence
        .as_ref()
        .map(|(files, digest)| fsl_tools::AppliedEvidence { files, digest });
    let rendered = fsl_tools::render_requirements_document(
        &claims,
        &kernel,
        &model,
        &source,
        locale,
        applied_glossary.as_ref(),
        applied_evidence.as_ref(),
        None,
    )
    .map_err(|error| error_output("document", &error))?;
    Ok((claims, rendered.markdown))
}

/// Reproduce a target's canonical rendering fresh from `(spec, inputs)` —
/// the shared re-render dispatch `approval create`'s conformance gate and
/// `approval check`/`approval diff`'s drift comparison both call. Returns the
/// rendered bytes plus, for `requirements_document` only, the RCIR claim-set
/// digest that rendering used (`None` for the three solver-driven kinds,
/// which have no RCIR concept at all).
fn approval_artifact(
    path: &Path,
    kind: &str,
    inputs: &approval::GenerationInputs,
) -> Result<(Vec<u8>, Option<String>), Value> {
    if kind == "requirements_document" {
        let approval::GenerationInputs::Document(document_inputs) = inputs else {
            return Err(error_output(
                "usage",
                "requirements_document approval requires document generation inputs",
            ));
        };
        let (claims, markdown) = render_document_for_approval(path, document_inputs)?;
        return Ok((markdown.into_bytes(), Some(claims.spec.claim_set_digest)));
    }
    let approval::GenerationInputs::Solver(inputs) = inputs else {
        return Err(error_output(
            "usage",
            "ledger/html/scenarios approval requires solver generation inputs",
        ));
    };
    let (result, status) = match kind {
        "ledger" => generate_unapproved_ledger_report(&LedgerReportRequest {
            path,
            depth: inputs.depth,
            deadlock_mode: &inputs.deadlock,
            engine: &inputs.engine,
            impl_log: None,
            evidence_paths: &[],
            output_path: None,
        }),
        "html" => run_html_report(path, inputs.depth, &inputs.deadlock, &inputs.engine, None),
        "scenarios" => run_scenarios(path, inputs.depth, &inputs.deadlock),
        _ => {
            return Err(error_output(
                "usage",
                &format!("unsupported approval target '{kind}'"),
            ));
        }
    };
    if status != 0 {
        return Err(result);
    }
    let bytes = if matches!(kind, "ledger" | "html") {
        result
            .get("content")
            .and_then(Value::as_str)
            .map(|content| content.as_bytes().to_vec())
            .ok_or_else(|| error_output("internal", "generated artifact has no content"))?
    } else {
        let mut encoded = serde_json::to_vec_pretty(&result)
            .map_err(|error| error_output("internal", &error.to_string()))?;
        encoded.push(b'\n');
        encoded
    };
    Ok((bytes, None))
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn run_approval_create(
    path: &Path,
    kind: &str,
    artifact_path: &Path,
    approver: &str,
    selected_requirements: &[String],
    inputs: &approval::GenerationInputs,
    output_path: Option<&Path>,
    signing_key: Option<&Path>,
) -> (Value, i32) {
    if !matches!(
        kind,
        "ledger" | "html" | "scenarios" | "requirements_document"
    ) {
        return (
            error_output(
                "usage",
                "--kind must be ledger, html, scenarios, or requirements_document",
            ),
            2,
        );
    }
    if approver.trim().is_empty() {
        return (error_output("usage", "--approver must not be empty"), 2);
    }
    let model = match load_model(path) {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let (_repo, relative_path, git_commit) = match approval::git_binding(path) {
        Ok(binding) => binding,
        Err(error) => return (error_output("io", &error), 2),
    };
    let digest = match approval::spec_digest(path) {
        Ok(digest) => digest,
        Err(error) => return (semantic_error_output(&error), 2),
    };

    let (target_digest_bytes, target_digest_algorithm, reviewed_digest, claim_set_digest, schema) =
        if kind == "requirements_document" {
            let approval::GenerationInputs::Document(document_inputs) = inputs else {
                return (
                    error_output(
                        "usage",
                        "--kind requirements_document requires document generation inputs",
                    ),
                    2,
                );
            };
            let (claims, fresh_markdown) = match render_document_for_approval(path, document_inputs)
            {
                Ok(result) => result,
                Err(error) => return (error, 2),
            };
            let reviewed_text = match std::fs::read_to_string(artifact_path) {
                Ok(text) => text,
                Err(error) => return (error_output("io", &error.to_string()), 2),
            };
            let report = match fsl_tools::check_requirements_document(
                &reviewed_text,
                &claims,
                &fresh_markdown,
            ) {
                Ok(report) => report,
                Err(error) => return (document_schema_error(&error.to_string()), 2),
            };
            if !report.is_conformant() {
                let mut output = error_output(
                    "semantics",
                    "reviewed document does not conform to a fresh rendering with the recorded inputs",
                );
                output
                    .as_object_mut()
                    .expect("approval error envelope")
                    .insert(
                        "reasons".to_owned(),
                        serde_json::to_value(&report.reasons).expect("serialize drift reasons"),
                    );
                return (output, 2);
            }
            (
                fresh_markdown.into_bytes(),
                approval::REQUIREMENTS_DOCUMENT_DIGEST_ALGORITHM,
                Some(approval::sha256_bytes(reviewed_text.as_bytes())),
                Some(claims.spec.claim_set_digest),
                approval::APPROVAL_SCHEMA_V3,
            )
        } else {
            let approval::GenerationInputs::Solver(_) = inputs else {
                return (
                    error_output(
                        "usage",
                        "--kind ledger/html/scenarios requires solver generation inputs",
                    ),
                    2,
                );
            };
            let (expected_artifact, _) = match approval_artifact(path, kind, inputs) {
                Ok(artifact) => artifact,
                Err(error) => return (error, 2),
            };
            let reviewed_artifact = match std::fs::read(artifact_path) {
                Ok(artifact) => artifact,
                Err(error) => return (error_output("io", &error.to_string()), 2),
            };
            let normalized_reviewed = match approval::normalized_artifact(kind, &reviewed_artifact)
            {
                Ok(artifact) => artifact,
                Err(error) => return (error_output("semantics", &error), 2),
            };
            let normalized_expected = match approval::normalized_artifact(kind, &expected_artifact)
            {
                Ok(artifact) => artifact,
                Err(error) => return (error_output("internal", &error), 3),
            };
            if normalized_reviewed != normalized_expected {
                return (
                    error_output(
                        "semantics",
                        "reviewed artifact does not match a fresh rendering with the recorded inputs",
                    ),
                    2,
                );
            }
            (
                normalized_reviewed,
                approval::ARTIFACT_DIGEST_ALGORITHM,
                None,
                None,
                approval::APPROVAL_SCHEMA,
            )
        };

    let available = approval::requirement_ids(&model);
    if available.is_empty() {
        return (
            error_output(
                "semantics",
                "approval creation requires at least one requirement ID in the specification",
            ),
            2,
        );
    }
    let mut requirements = if selected_requirements.is_empty() {
        available.clone()
    } else {
        selected_requirements.to_vec()
    };
    requirements.sort();
    requirements.dedup();
    if let Some(unknown) = requirements
        .iter()
        .find(|requirement| !available.contains(*requirement))
    {
        return (
            error_output(
                "semantics",
                &format!("approval references unknown requirement '{unknown}'"),
            ),
            2,
        );
    }
    let record = approval::ApprovalRecord {
        schema: schema.to_owned(),
        spec: approval::SpecBinding {
            path: relative_path,
            digest_algorithm: approval::SPEC_DIGEST_ALGORITHM.to_owned(),
            digest,
            git_commit,
        },
        target: approval::TargetBinding {
            kind: kind.to_owned(),
            path: artifact_path.display().to_string(),
            digest_algorithm: target_digest_algorithm.to_owned(),
            digest: approval::sha256_bytes(&target_digest_bytes),
            reviewed_digest_algorithm: reviewed_digest
                .as_ref()
                .map(|_| approval::REVIEWED_REQUIREMENTS_DOCUMENT_DIGEST_ALGORITHM.to_owned()),
            reviewed_digest,
            claim_set_digest_algorithm: claim_set_digest
                .as_ref()
                .map(|_| approval::CLAIM_SET_DIGEST_ALGORITHM.to_owned()),
            claim_set_digest,
            generator: "fslc".to_owned(),
            generator_version: env!("CARGO_PKG_VERSION").to_owned(),
            inputs: inputs.clone(),
        },
        approval: approval::ApprovalMetadata {
            approver: approver.to_owned(),
            approved_at: approval::now_rfc3339(),
            requirements,
        },
    };
    let destination = output_path.map_or_else(
        || {
            let stem = path
                .file_stem()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or("spec");
            path.with_file_name(format!("{stem}.approval.json"))
        },
        Path::to_path_buf,
    );
    let record = if let Some(signing_key) = signing_key {
        let record = match approval::sign_record(record, signing_key) {
            Ok(record) => record,
            Err(error) => return (error_output("io", &error), 2),
        };
        if let Err(error) = approval::write_record_v2(&destination, &record) {
            return (error_output("io", &error), 2);
        }
        serde_json::to_value(record).expect("approval v2 serializes")
    } else {
        if let Err(error) = approval::write_record(&destination, &record) {
            return (error_output("io", &error), 2);
        }
        serde_json::to_value(record).expect("approval v1 serializes")
    };
    let mut output = envelope();
    output.insert("result".to_owned(), json!("created"));
    output.insert("kind".to_owned(), json!("approval_record"));
    output.insert("output".to_owned(), json!(destination));
    output.insert("record".to_owned(), record);
    (Value::Object(output), 0)
}

fn approval_evaluation(
    path: &Path,
    record_path: &Path,
    trust: &approval::TrustStore,
) -> Result<Value, Value> {
    let versioned =
        approval::read_versioned_record(record_path).map_err(|error| error_output("io", &error))?;
    let (record, signature_status, key_id) = match &versioned {
        approval::VersionedApprovalRecord::V1(record) => (record.clone(), "unsigned", Value::Null),
        approval::VersionedApprovalRecord::V2(record) => {
            let verified = trust
                .verify(record)
                .map_err(|error| error_output("io", &error))?;
            if !verified {
                return Ok(json!({
                    "status": "signature-invalid",
                    "signature_status": "signature-invalid",
                    "key_id": record.signature.key_id,
                    "reasons": ["signature_invalid"],
                    "record": record_path.display().to_string(),
                    "target_kind": record.target.kind,
                    "approver": record.approval.approver,
                    "approved_at": record.approval.approved_at,
                    "requirements": record.approval.requirements,
                    "baseline_digest": record.spec.digest,
                }));
            }
            (
                versioned.binding(),
                "signed",
                json!(record.signature.key_id),
            )
        }
    };
    let (repo, relative_path, _head) =
        approval::git_location(path).map_err(|error| error_output("io", &error))?;
    if record.spec.path != relative_path {
        return Err(error_output(
            "semantics",
            &format!(
                "approval record targets '{}' but current spec is '{relative_path}'",
                record.spec.path
            ),
        ));
    }
    approval::verify_git_baseline(&repo, &relative_path, &record.spec.git_commit)
        .map_err(|error| error_output("io", &error))?;
    let current_spec_digest =
        approval::spec_digest(path).map_err(|error| semantic_error_output(&error))?;
    let (artifact, current_claim_set_digest) =
        approval_artifact(path, &record.target.kind, &record.target.inputs)?;
    let normalized_artifact = approval::normalized_artifact(&record.target.kind, &artifact)
        .map_err(|error| error_output("internal", &error))?;
    let current_artifact_digest = approval::sha256_bytes(&normalized_artifact);
    let current_reviewed_digest =
        approval::reviewed_artifact_digest(&record).map_err(|error| error_output("io", &error))?;
    let mut evaluation = approval::evaluate(
        &record,
        record_path,
        &current_spec_digest,
        &current_artifact_digest,
        env!("CARGO_PKG_VERSION"),
        current_claim_set_digest.as_deref(),
        current_reviewed_digest.as_deref(),
    );
    let output = evaluation
        .as_object_mut()
        .expect("approval evaluation object");
    output.insert("signature_status".to_owned(), json!(signature_status));
    output.insert("key_id".to_owned(), key_id);
    Ok(evaluation)
}

fn run_approval_check(path: &Path, record_path: &Path, trust_keys: &[PathBuf]) -> (Value, i32) {
    let trust = match approval::TrustStore::load(trust_keys) {
        Ok(trust) => trust,
        Err(error) => return (error_output("io", &error), 2),
    };
    let evaluation = match approval_evaluation(path, record_path, &trust) {
        Ok(evaluation) => evaluation,
        Err(error) => return (error, 2),
    };
    let mut output = envelope();
    output.insert("result".to_owned(), json!("approval_check"));
    if let Some(fields) = evaluation.as_object() {
        output.extend(fields.clone());
    }
    let status =
        i32::from(evaluation.get("status").and_then(Value::as_str) == Some("signature-invalid"));
    (Value::Object(output), status)
}

fn approval_overlay(
    path: &Path,
    record_paths: &[PathBuf],
    trust_keys: &[PathBuf],
) -> Result<Value, Value> {
    let trust =
        approval::TrustStore::load(trust_keys).map_err(|error| error_output("io", &error))?;
    let mut requirements = Map::new();
    let mut records = Vec::new();
    for record_path in record_paths {
        let evaluation = approval_evaluation(path, record_path, &trust)?;
        for requirement in evaluation
            .get("requirements")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            requirements.insert(requirement.to_owned(), evaluation.clone());
        }
        records.push(evaluation);
    }
    Ok(json!({"requirements": requirements, "records": records}))
}

struct LedgerReportRequest<'a> {
    path: &'a Path,
    depth: usize,
    deadlock_mode: &'a str,
    engine: &'a str,
    impl_log: Option<&'a Path>,
    evidence_paths: &'a [PathBuf],
    output_path: Option<&'a Path>,
}

struct PreparedLedgerReport {
    model: KernelModel,
    verification: Value,
    replay: Option<Value>,
    evidence: Vec<(String, Value)>,
    scenarios: Value,
}

fn run_ledger_report(
    request: &LedgerReportRequest<'_>,
    approval_paths: &[PathBuf],
    trust_keys: &[PathBuf],
) -> (Value, i32) {
    let prepared = match prepare_ledger_report(request) {
        Ok(prepared) => prepared,
        Err(error) => return (error, 2),
    };
    let approvals = if approval_paths.is_empty() {
        None
    } else {
        match approval_overlay(request.path, approval_paths, trust_keys) {
            Ok(approvals) => Some(approvals),
            Err(error) => return (error, 2),
        }
    };
    render_ledger_report(request, &prepared, approvals.as_ref())
}

fn generate_unapproved_ledger_report(request: &LedgerReportRequest<'_>) -> (Value, i32) {
    let prepared = match prepare_ledger_report(request) {
        Ok(prepared) => prepared,
        Err(error) => return (error, 2),
    };
    render_ledger_report(request, &prepared, None)
}

fn prepare_ledger_report(request: &LedgerReportRequest<'_>) -> Result<PreparedLedgerReport, Value> {
    let model = match load_model(request.path) {
        Ok(model) => model,
        Err(error) => return Err(semantic_error_output(&error)),
    };
    let (verification, _) = run_verify(
        request.path,
        request.depth,
        request.deadlock_mode,
        request.engine,
        DEFAULT_EXPLICIT_BUDGET,
        1,
    );
    let replay = request
        .impl_log
        .map(|trace| run_replay(request.path, trace).0);
    let evidence = request
        .evidence_paths
        .iter()
        .map(|evidence_path| {
            let source = std::fs::read_to_string(evidence_path)
                .map_err(|error| error_output("io", &error.to_string()))?;
            let value = serde_json::from_str::<Value>(&source)
                .map_err(|error| error_output("io", &format!("invalid JSON: {error}")))?;
            if !value.is_object() {
                return Err(error_output(
                    "io",
                    "evidence JSON must contain an object envelope",
                ));
            }
            Ok((evidence_path.clone(), value))
        })
        .collect::<Result<Vec<_>, Value>>()?;
    let (scenarios, _) = run_scenarios(request.path, request.depth, request.deadlock_mode);
    let evidence = evidence
        .into_iter()
        .map(|(source, value)| (source.display().to_string(), value))
        .collect::<Vec<_>>();
    Ok(PreparedLedgerReport {
        model,
        verification,
        replay,
        evidence,
        scenarios,
    })
}

fn render_ledger_report(
    request: &LedgerReportRequest<'_>,
    prepared: &PreparedLedgerReport,
    approvals: Option<&Value>,
) -> (Value, i32) {
    let content = fsl_tools::render_ledger_with_approvals(
        &request.path.display().to_string(),
        &prepared.model,
        &prepared.verification,
        &prepared.scenarios,
        prepared.replay.as_ref(),
        &prepared.evidence,
        approvals,
    );
    generated_content_result(
        "audit_ledger",
        &prepared.model.name,
        format!(
            "{}_ledger.md",
            request
                .path
                .file_stem()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or("audit")
        ),
        &content,
        request.output_path,
    )
}

fn expression_identifiers(value: &Value) -> std::collections::BTreeSet<String> {
    fn visit(value: &Value, names: &mut std::collections::BTreeSet<String>) {
        match value {
            Value::Array(parts) => {
                match parts.first().and_then(Value::as_str) {
                    Some("var") => {
                        if let Some(name) = parts.get(1).and_then(Value::as_str) {
                            names.insert(name.to_owned());
                        }
                        return;
                    }
                    Some("index") => {
                        if let Some(name) = parts.get(1).and_then(Value::as_str) {
                            names.insert(name.to_owned());
                        }
                    }
                    _ => {}
                }
                for part in parts.iter().skip(1) {
                    visit(part, names);
                }
            }
            Value::Object(values) => {
                for value in values.values() {
                    visit(value, names);
                }
            }
            _ => {}
        }
    }
    let mut names = std::collections::BTreeSet::new();
    visit(value, &mut names);
    names
}

fn tag_lvalue_text(target: &KernelLValue) -> String {
    match target {
        KernelLValue::Var(name) => name.clone(),
        KernelLValue::Index(name, index) => {
            format!("{name}[{}]", fslc_rust::expr_text(index))
        }
        KernelLValue::Field(base, field) => format!("{}.{}", tag_lvalue_text(base), field),
    }
}

fn tag_statement_effects(
    statements: &[KernelStatement],
    conditions: &[String],
    effects: &mut Vec<Value>,
) {
    for statement in statements {
        match statement {
            KernelStatement::Assign { target, value, .. } => effects.push(json!({
                "target":tag_lvalue_text(target),
                "expression":fslc_rust::expr_text(value),
                "conditions":conditions,
            })),
            KernelStatement::If {
                condition,
                then_statements,
                else_statements,
                ..
            } => {
                let condition = fslc_rust::expr_text(condition);
                let mut then_conditions = conditions.to_vec();
                then_conditions.push(condition.clone());
                tag_statement_effects(then_statements, &then_conditions, effects);
                let mut else_conditions = conditions.to_vec();
                else_conditions.push(format!("not ({condition})"));
                tag_statement_effects(else_statements, &else_conditions, effects);
            }
            KernelStatement::ForAll { statements, .. } => {
                tag_statement_effects(statements, conditions, effects);
            }
        }
    }
}

fn tagged_property(kind: &str, property: &fsl_core::PropertyDef) -> Option<Value> {
    let tags = requirement_metadata(&property.annotations, property.meta.as_ref());
    let tag = tags.first()?.clone();
    Some(json!({
        "kind":kind,
        "name":property.name,
        "node_id":format!("{kind}:{}",property.name),
        "tag":tag,
        "tags":tags,
        "loc":property.span.python_loc(),
        "formal_definition":{"expression":fslc_rust::expr_text(&property.expr)},
        "formal_identifiers":expression_identifiers(&property.expr.python_ast()),
    }))
}

#[allow(clippy::too_many_lines)]
fn tag_review_output(model: &KernelModel) -> Value {
    let mut declarations = Vec::new();
    for action in &model.actions {
        let tags = requirement_metadata(&action.annotations, action.meta.as_ref());
        let Some(tag) = tags.first().cloned() else {
            continue;
        };
        let mut identifiers = std::collections::BTreeSet::new();
        for expr in action.requires.iter().chain(&action.ensures) {
            identifiers.extend(expression_identifiers(&expr.python_ast()));
        }
        for statement in &action.statements {
            identifiers.extend(expression_identifiers(&statement.python_ast()));
        }
        identifiers.extend(action.params.iter().map(|param| param.name().to_owned()));
        let mut effects = Vec::new();
        tag_statement_effects(&action.statements, &[], &mut effects);
        declarations.push(json!({
            "kind":"action",
            "name":action.name,
            "node_id":format!("action:{}",action.name),
            "tag":tag,
            "tags":tags,
            "loc":action.span.python_loc(),
            "formal_definition":{
                "parameters":action.params.iter().map(ParamDef::name).collect::<Vec<_>>(),
                "requires":action.requires.iter().map(fslc_rust::expr_text).collect::<Vec<_>>(),
                "ensures":action.ensures.iter().map(fslc_rust::expr_text).collect::<Vec<_>>(),
                "effects":effects,
            },
            "formal_identifiers":identifiers,
        }));
    }
    declarations.extend(
        model
            .invariants
            .iter()
            .filter_map(|property| tagged_property("invariant", property)),
    );
    declarations.extend(
        model
            .transitions
            .iter()
            .filter_map(|property| tagged_property("trans", property)),
    );
    declarations.extend(
        model
            .reachables
            .iter()
            .filter_map(|property| tagged_property("reachable", property)),
    );
    for property in &model.leadstos {
        let tags = requirement_metadata(&property.annotations, property.meta.as_ref());
        let Some(tag) = tags.first().cloned() else {
            continue;
        };
        let mut identifiers = expression_identifiers(&property.before.python_ast());
        identifiers.extend(expression_identifiers(&property.after.python_ast()));
        if let Some(decreases) = &property.decreases {
            identifiers.extend(expression_identifiers(&decreases.python_ast()));
        }
        let mut formal = Map::new();
        formal.insert(
            "premise".to_owned(),
            json!(fslc_rust::expr_text(&property.before)),
        );
        formal.insert(
            "consequence".to_owned(),
            json!(fslc_rust::expr_text(&property.after)),
        );
        if let Some(within) = property.within {
            formal.insert("within".to_owned(), json!(within));
        }
        if let Some(decreases) = &property.decreases {
            formal.insert(
                "decreases".to_owned(),
                json!(fslc_rust::expr_text(decreases)),
            );
        }
        declarations.push(json!({
            "kind":"leadsTo",
            "name":property.name,
            "node_id":format!("leadsTo:{}",property.name),
            "tag":tag,
            "tags":tags,
            "loc":property.span.python_loc(),
            "formal_definition":formal,
            "formal_identifiers":identifiers,
        }));
    }
    declarations.sort_by_key(|item| {
        (
            item["kind"].as_str().unwrap_or_default().to_owned(),
            item["name"].as_str().unwrap_or_default().to_owned(),
        )
    });
    let mut output = envelope();
    output.insert("result".to_owned(), json!("analyzed"));
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("analysis".to_owned(), json!("tag_review"));
    output.insert("export".to_owned(), json!("tag-review"));
    output.insert("schema_version".to_owned(), json!("tag-review.v0"));
    output.insert(
        "review_contract".to_owned(),
        json!({
            "unit":"declaration",
            "decision":"compare tag.text with formal_definition",
            "formal_status":"not_a_violation",
            "meaning_judgment":"external_review_required",
        }),
    );
    output.insert("declarations".to_owned(), Value::Array(declarations));
    Value::Object(output)
}

fn tag_tokens(
    text: &str,
) -> (
    std::collections::BTreeSet<String>,
    std::collections::BTreeSet<String>,
) {
    let mut bare = std::collections::BTreeSet::new();
    let mut current = String::new();
    for character in text.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            current.push(character);
        } else if !current.is_empty() {
            if current
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
            {
                bare.insert(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }
    if !current.is_empty()
        && current
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
    {
        bare.insert(current);
    }
    let explicit = text
        .split('`')
        .enumerate()
        .filter(|(index, _)| index % 2 == 1)
        .map(|(_, value)| value)
        .filter(|value| {
            !value.is_empty()
                && value
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        })
        .map(str::to_owned)
        .collect();
    (bare, explicit)
}

fn ai_tag_findings(model: &KernelModel) -> Vec<Value> {
    let export = tag_review_output(model);
    let declarations = export["declarations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let states = model
        .state
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let constants = model
        .consts
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let mut catalog = states
        .union(&constants)
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    catalog.extend(model.types.keys().cloned());
    catalog.extend(model.enum_members.keys().cloned());
    catalog.extend(model.actions.iter().map(|action| action.name.clone()));
    let relevant = states
        .union(&constants)
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let mut findings = Vec::new();
    for declaration in declarations {
        let formal = declaration["formal_identifiers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<std::collections::BTreeSet<_>>();
        let text = declaration["tag"]["text"].as_str().unwrap_or_default();
        let (bare, explicit) = tag_tokens(text);
        let local = formal
            .difference(&catalog)
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let mentioned = bare
            .union(&explicit)
            .filter(|token| catalog.contains(*token) || local.contains(*token))
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let stale = bare
            .union(&explicit)
            .filter(|token| {
                (explicit.contains(*token)
                    || token.contains('_')
                    || (token
                        .chars()
                        .all(|character| character.is_ascii_uppercase())
                        && token.len() > 2))
                    && !catalog.contains(*token)
                    && !local.contains(*token)
            })
            .cloned()
            .collect::<Vec<_>>();
        let disjoint = mentioned
            .intersection(&relevant)
            .filter(|token| !formal.contains(*token))
            .cloned()
            .collect::<Vec<_>>();
        for (finding_type, identifiers) in [
            ("tag_stale_reference", stale),
            ("tag_formula_disjoint", disjoint),
        ] {
            if identifiers.is_empty() {
                continue;
            }
            let stale = finding_type == "tag_stale_reference";
            findings.push(fsl_tools::review_finding(
                finding_type,
                if stale { 0.82 } else { 0.74 },
                json!([declaration["node_id"]]),
                json!({
                    "kind":if stale { "tag_mentions_unknown_identifier" } else { "tag_identifier_absent_from_formula" },
                    "declaration":{"kind":declaration["kind"],"name":declaration["name"],"tag":declaration["tag"]},
                    "identifiers":identifiers,
                    "formal_identifiers":declaration["formal_identifiers"],
                }),
                if stale {
                    "The declaration tag contains a code-shaped identifier that is not present in the current specification, which may be a stale reference after a rename or deletion."
                } else {
                    "The tag names a current state variable or constant that the tagged formal definition does not reference, so the human label and checked formula may have drifted apart."
                },
                json!([{"kind":"review_tag_formula_pair","template":if stale { "Update the tag to the current identifier, or confirm that the token is prose and quote/reword it so it is not presented as an FSL identifier." } else { "Review the tag and formal definition together; update whichever side no longer expresses the intended requirement." }}]),
                json!([if stale { "The analyzer does not prove that the prose intended to reference an FSL identifier." } else { "Identifier overlap is not proof that natural-language and formal meanings agree." },"This finding is not a verifier violation."]),
                declaration.get("loc").cloned(),
            ));
        }
    }
    findings
}

#[allow(clippy::too_many_lines)]
fn ai_progressless_findings(model: &KernelModel, tsg: &Value) -> Vec<Value> {
    let Ok(dependencies) = fsl_tools::analyze_model(model, "action_dependency_graph", None) else {
        return Vec::new();
    };
    let scenario_ids = tsg["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|node| matches!(node["kind"].as_str(), Some("acceptance" | "forbidden")))
        .filter_map(|node| node["id"].as_str().map(str::to_owned))
        .collect::<std::collections::BTreeSet<_>>();
    let scenario_actions = tsg["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|edge| scenario_ids.contains(edge["from"].as_str().unwrap_or_default()))
        .filter(|edge| {
            edge["to"]
                .as_str()
                .is_some_and(|target| target.starts_with("action:"))
        })
        .filter_map(|edge| edge["to"].as_str().map(str::to_owned))
        .collect::<std::collections::BTreeSet<_>>();
    let action_meta = model
        .actions
        .iter()
        .map(|action| {
            (
                format!("action:{}", action.name),
                !action.annotations.source_order().is_empty(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let dependency_edges = dependencies["edges"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut findings = Vec::new();
    for cycle in dependencies["cycles"].as_array().into_iter().flatten() {
        let steps = cycle["steps"].as_array().cloned().unwrap_or_default();
        let cycle_actions = steps
            .iter()
            .filter_map(Value::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        if cycle_actions.len() < 2
            || (!cycle_actions
                .iter()
                .any(|action| action_meta.get(*action).copied().unwrap_or(false))
                && cycle_actions
                    .iter()
                    .all(|action| !scenario_actions.contains(*action)))
        {
            continue;
        }
        let mut expanded = Vec::new();
        let mut cycle_states = std::collections::BTreeSet::new();
        for pair in steps.windows(2) {
            let (Some(from), Some(to)) = (pair[0].as_str(), pair[1].as_str()) else {
                continue;
            };
            expanded.push(from.to_owned());
            let state = dependency_edges
                .iter()
                .find(|edge| edge["kind"] == "enables" && edge["from"] == from && edge["to"] == to)
                .and_then(|edge| edge["state"].as_str());
            if let Some(state) = state {
                expanded.push(state.to_owned());
                cycle_states.insert(state.to_owned());
            }
        }
        if let Some(last) = steps.last().and_then(Value::as_str) {
            expanded.push(last.to_owned());
        }
        let mut attached = model.actions.iter().any(|action| {
            action.fair && cycle_actions.contains(format!("action:{}", action.name).as_str())
        });
        if !attached {
            attached = model.leadstos.iter().any(|property| {
                let mut reads = expression_identifiers(&property.before.python_ast());
                reads.extend(expression_identifiers(&property.after.python_ast()));
                reads
                    .iter()
                    .any(|state| cycle_states.contains(&format!("state:{state}")))
            });
        }
        if !attached && let Some(terminal) = &model.terminal {
            attached = expression_identifiers(&terminal.python_ast())
                .iter()
                .any(|state| cycle_states.contains(&format!("state:{state}")));
        }
        if attached {
            continue;
        }
        let involved = expanded
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        findings.push(fsl_tools::review_finding(
            "progressless_cycle",
            0.68,
            json!(involved),
            json!({"kind":"representative_cycle","steps":expanded,"attached_progress":[]}),
            "This requirement/scenario-linked cycle has no explicit leadsTo, bounded exit, terminal exit, or fairness condition attached.",
            json!([
                {"kind":"add_property","template":"Add a leadsTo property that states the cyclic state eventually reaches a terminal state."},
                {"kind":"strengthen_model","template":"Introduce an explicit bound and terminal state for the cyclic behavior."},
                {"kind":"mark_or_fix_fairness","template":"Mark the progress-driving action fair, or add a guard/model change that makes progress explicit."}
            ]),
            json!(["The cycle is wrong.","The spec violates liveness.","A high cycle count is itself a defect."]),
            None,
        ));
    }
    findings
}

fn counter_delta(name: &str, expr: &KernelExpr, model: &KernelModel) -> Option<i64> {
    fn scalar(expr: &KernelExpr, model: &KernelModel) -> Option<i64> {
        match expr {
            KernelExpr::Num(value) => Some(*value),
            KernelExpr::Var(name) => match model.consts.get(name) {
                Some(fsl_core::FslValue::Int(value)) => Some(*value),
                _ => None,
            },
            KernelExpr::Neg(value) => scalar(value, model).map(|value| -value),
            _ => None,
        }
    }
    let KernelExpr::Binary { op, left, right } = expr else {
        return None;
    };
    match op.as_str() {
        "+" if matches!(left.as_ref(), KernelExpr::Var(value) if value == name) => {
            scalar(right, model)
        }
        "+" if matches!(right.as_ref(), KernelExpr::Var(value) if value == name) => {
            scalar(left, model)
        }
        "-" if matches!(left.as_ref(), KernelExpr::Var(value) if value == name) => {
            scalar(right, model).map(|value| -value)
        }
        _ => None,
    }
}

fn scan_counter_statements(
    statements: &[KernelStatement],
    counters: &std::collections::BTreeSet<String>,
    model: &KernelModel,
    nested: bool,
    deltas: &mut std::collections::BTreeMap<String, i64>,
    excluded: &mut std::collections::BTreeSet<String>,
) {
    for statement in statements {
        match statement {
            KernelStatement::Assign { target, value, .. } => {
                let root = lvalue_root(target);
                if !counters.contains(root) {
                    continue;
                }
                if nested || !matches!(target, KernelLValue::Var(name) if name == root) {
                    excluded.insert(root.to_owned());
                } else if let Some(delta) = counter_delta(root, value, model) {
                    *deltas.entry(root.to_owned()).or_default() += delta;
                } else {
                    excluded.insert(root.to_owned());
                }
            }
            KernelStatement::If {
                then_statements,
                else_statements,
                ..
            } => {
                scan_counter_statements(then_statements, counters, model, true, deltas, excluded);
                scan_counter_statements(else_statements, counters, model, true, deltas, excluded);
            }
            KernelStatement::ForAll { statements, .. } => {
                scan_counter_statements(statements, counters, model, true, deltas, excluded);
            }
        }
    }
}

fn integer_gcd(mut left: i64, mut right: i64) -> i64 {
    left = left.abs();
    right = right.abs();
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left.max(1)
}

fn weighted_sum_text(weights: &std::collections::BTreeMap<String, i64>) -> String {
    let mut parts = Vec::new();
    for (name, weight) in weights {
        if *weight == 0 {
            continue;
        }
        let term = if weight.abs() == 1 {
            name.clone()
        } else {
            format!("{}*{name}", weight.abs())
        };
        if parts.is_empty() {
            parts.push(if *weight > 0 {
                term
            } else {
                format!("-{term}")
            });
        } else {
            parts.push(if *weight > 0 {
                format!("+ {term}")
            } else {
                format!("- {term}")
            });
        }
    }
    parts.join(" ")
}

#[allow(clippy::too_many_lines)]
fn ai_conservation_findings(model: &KernelModel) -> Vec<Value> {
    let counters = model
        .state
        .iter()
        .filter(|(_, ty)| matches!(ty, TypeRef::Int))
        .map(|(name, _)| name.clone())
        .collect::<std::collections::BTreeSet<_>>();
    if counters.len() < 2 {
        return Vec::new();
    }
    let mut excluded = std::collections::BTreeSet::new();
    let mut actions = model.actions.iter().collect::<Vec<_>>();
    actions.sort_by_key(|action| &action.name);
    let mut rows = Vec::new();
    for action in actions {
        let mut deltas = std::collections::BTreeMap::new();
        scan_counter_statements(
            &action.statements,
            &counters,
            model,
            false,
            &mut deltas,
            &mut excluded,
        );
        rows.push((format!("action:{}", action.name), deltas));
    }
    let eligible = counters
        .iter()
        .filter(|counter| {
            !excluded.contains(*counter)
                && rows
                    .iter()
                    .any(|(_, row)| row.get(*counter).copied().unwrap_or_default() != 0)
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut findings = Vec::new();
    for left in 0..eligible.len() {
        for right in left + 1..eligible.len() {
            let first = rows.iter().find_map(|(_, row)| {
                let a = row.get(&eligible[left]).copied().unwrap_or_default();
                let b = row.get(&eligible[right]).copied().unwrap_or_default();
                (a != 0 || b != 0).then_some((a, b))
            });
            let Some((a, b)) = first else {
                continue;
            };
            let divisor = integer_gcd(a, b);
            let mut left_weight = b / divisor;
            let mut right_weight = -a / divisor;
            if left_weight < 0 || (left_weight == 0 && right_weight < 0) {
                left_weight = -left_weight;
                right_weight = -right_weight;
            }
            if left_weight == 0
                || right_weight == 0
                || rows.iter().any(|(_, row)| {
                    left_weight * row.get(&eligible[left]).copied().unwrap_or_default()
                        + right_weight * row.get(&eligible[right]).copied().unwrap_or_default()
                        != 0
                })
            {
                continue;
            }
            let weights = std::collections::BTreeMap::from([
                (eligible[left].clone(), left_weight),
                (eligible[right].clone(), right_weight),
            ]);
            let action_effects = rows
                .iter()
                .filter_map(|(action, row)| {
                    let deltas = weights
                        .keys()
                        .filter_map(|name| {
                            let delta = row.get(name).copied().unwrap_or_default();
                            (delta != 0).then_some((name.clone(), delta))
                        })
                        .collect::<std::collections::BTreeMap<_, _>>();
                    (!deltas.is_empty()).then_some(json!({
                        "action":action,
                        "deltas":deltas,
                        "weighted_sum_delta":deltas.iter().map(|(name,delta)|weights[name]*delta).sum::<i64>(),
                    }))
                })
                .collect::<Vec<_>>();
            if action_effects.len() < 2 {
                continue;
            }
            let expression = weighted_sum_text(&weights);
            let involved = weights
                .keys()
                .map(|name| format!("state:{name}"))
                .chain(
                    action_effects
                        .iter()
                        .filter_map(|item| item["action"].as_str().map(str::to_owned)),
                )
                .collect::<std::collections::BTreeSet<_>>();
            findings.push(fsl_tools::review_finding(
                "conservation_candidate",
                0.6,
                json!(involved),
                json!({
                    "kind":"weighted_sum_conservation_candidate",
                    "expression":expression,
                    "weights":weights,
                    "action_net_effects":action_effects,
                    "excluded_counters":excluded,
                }),
                "Counter-like effects structurally preserve this weighted sum, which may indicate an implicit invariant worth declaring and proving.",
                json!([{"kind":"add_invariant_then_verify","template":format!("Declare `invariant Conservation {{ {expression} == <initial value> }}` and run `fslc verify` plus `--engine induction` to prove it.")}]),
                json!(["The weighted sum is actually invariant.","The absence of a candidate means no conservation law exists.","This finding is a proof; it is only structural evidence and must be checked by verify."]),
                None,
            ));
        }
    }
    findings.truncate(8);
    findings
}

struct SemanticReview {
    divergent: Vec<Value>,
    unconstrained: Vec<Value>,
    action_nodes: std::collections::BTreeSet<String>,
    state_nodes: std::collections::BTreeSet<String>,
}

fn semantic_action_record(
    enabled: &fsl_runtime::EnabledAction,
    successor: &fsl_runtime::State,
) -> Value {
    json!({
        "name":fslc_rust::display_name(&enabled.action),
        "params":enabled.params.iter().map(|(name,value)|(name.clone(),fslc_rust::fsl_value_json(value))).collect::<Map<_,_>>(),
        "successor":fslc_rust::state_json(successor),
    })
}

fn predicate_value(
    model: &KernelModel,
    expression: &KernelExpr,
    state: &fsl_runtime::State,
) -> Option<bool> {
    match fsl_runtime::eval(
        expression,
        state,
        &mut fsl_runtime::Bindings::new(),
        model,
        None,
    )
    .ok()?
    {
        fsl_core::FslValue::Bool(value) => Some(value),
        _ => None,
    }
}

#[allow(clippy::too_many_lines)]
fn bounded_semantic_review(
    model: &KernelModel,
    unconstrained_states: &std::collections::BTreeSet<String>,
    acceptance: &[(String, KernelExpr)],
) -> SemanticReview {
    let Ok(initial) = fsl_runtime::Monitor::new(model.clone()) else {
        return SemanticReview {
            divergent: Vec::new(),
            unconstrained: Vec::new(),
            action_nodes: std::collections::BTreeSet::new(),
            state_nodes: std::collections::BTreeSet::new(),
        };
    };
    let initial_trace = vec![fsl_core::TraceStep {
        step: 0,
        state: initial.state.clone(),
        action: None,
        changes: std::collections::BTreeMap::new(),
    }];
    let mut predicates = model
        .invariants
        .iter()
        .filter(|property| !property.name.starts_with('_'))
        .map(|property| {
            (
                "invariant",
                property.name.as_str(),
                format!("invariant:{}", property.name),
                &property.expr,
            )
        })
        .collect::<Vec<_>>();
    predicates.extend(acceptance.iter().map(|(name, expression)| {
        (
            "acceptance",
            name.as_str(),
            format!("acceptance:{name}"),
            expression,
        )
    }));
    let mut queue = std::collections::VecDeque::from([(initial, 0_usize, initial_trace)]);
    let mut visited = std::collections::BTreeSet::new();
    let mut seen_pairs = std::collections::BTreeSet::new();
    let mut seen_states = std::collections::BTreeSet::new();
    let mut divergent = Vec::new();
    let mut unconstrained = Vec::new();
    let mut pair_queries = 0;
    while let Some((monitor, step, trace)) = queue.pop_front() {
        if !visited.insert(monitor.state.clone()) || step > 4 {
            continue;
        }
        let Ok(enabled) = monitor.enabled() else {
            continue;
        };
        let mut groups = std::collections::BTreeMap::<String, Vec<_>>::new();
        for action in &enabled {
            groups
                .entry(action.action.clone())
                .or_default()
                .push(action.clone());
        }
        let names = groups.keys().cloned().collect::<Vec<_>>();
        for left_index in 0..names.len() {
            for right_index in left_index + 1..names.len() {
                let pair = (names[left_index].clone(), names[right_index].clone());
                for left in &groups[&pair.0] {
                    for right in &groups[&pair.1] {
                        if pair_queries >= 256 {
                            break;
                        }
                        pair_queries += 1;
                        let mut left_monitor = monitor.clone();
                        let mut right_monitor = monitor.clone();
                        let (Ok(left_step), Ok(right_step)) =
                            (left_monitor.step(left), right_monitor.step(right))
                        else {
                            continue;
                        };
                        let left_state = left_step.attempted_state.unwrap_or(left_step.state);
                        let right_state = right_step.attempted_state.unwrap_or(right_step.state);
                        let divergent_state = model
                            .state
                            .iter()
                            .filter(|(name, _)| left_state.get(name) != right_state.get(name))
                            .map(|(name, _)| name.clone())
                            .collect::<Vec<_>>();
                        let base_record = || {
                            json!({
                                "bounded_evidence":{"available":true,"depth":4,"reachable_at_step":step},
                                "trace":fslc_rust::trace_json(model,&trace),
                                "state":fslc_rust::state_json(&monitor.state),
                                "actions":[semantic_action_record(left,&left_state),semantic_action_record(right,&right_state)],
                                "action_nodes":[format!("action:{}",left.action),format!("action:{}",right.action)],
                                "divergent_state":divergent_state,
                            })
                        };
                        if !seen_pairs.contains(&pair) {
                            let differing = predicates
                                .iter()
                                .filter_map(|(kind, name, node, expression)| {
                                    let left = predicate_value(model, expression, &left_state)?;
                                    let right = predicate_value(model, expression, &right_state)?;
                                    (left != right).then_some((*kind, *name, node))
                                })
                                .collect::<Vec<_>>();
                            if !differing.is_empty() {
                                let mut record = base_record();
                                if let Value::Object(object) = &mut record {
                                    object.insert(
                                        "kind".to_owned(),
                                        json!("reachable_divergent_choice"),
                                    );
                                    object.insert("differing_predicates".to_owned(), json!(differing.iter().map(|(kind,name,_)|json!({"kind":kind,"name":name})).collect::<Vec<_>>()));
                                    object.insert(
                                        "predicate_nodes".to_owned(),
                                        json!(
                                            differing
                                                .iter()
                                                .map(|(_, _, node)| (*node).clone())
                                                .collect::<Vec<_>>()
                                        ),
                                    );
                                }
                                divergent.push(record);
                                seen_pairs.insert(pair.clone());
                            }
                        }
                        let remaining_states = unconstrained_states
                            .difference(&seen_states)
                            .cloned()
                            .collect::<Vec<_>>();
                        for state in &remaining_states {
                            if left_state.get(state) == right_state.get(state) {
                                continue;
                            }
                            let mut record = base_record();
                            if let Value::Object(object) = &mut record {
                                object.insert(
                                    "kind".to_owned(),
                                    json!("reachable_unconstrained_effect"),
                                );
                                object.insert("state_name".to_owned(), json!(state));
                                object.insert("divergent_state".to_owned(), json!([state]));
                            }
                            unconstrained.push(record);
                            seen_states.insert(state.clone());
                        }
                    }
                }
            }
        }
        if step < 4 {
            for action in enabled {
                let mut child = monitor.clone();
                let Ok(result) = child.step(&action) else {
                    continue;
                };
                if result.violation.is_some() {
                    continue;
                }
                let mut child_trace = trace.clone();
                child_trace.push(fsl_core::TraceStep {
                    step: step + 1,
                    state: child.state.clone(),
                    action: Some(fsl_core::TraceAction {
                        name: action.action,
                        params: action.params,
                    }),
                    changes: std::collections::BTreeMap::new(),
                });
                queue.push_back((child, step + 1, child_trace));
            }
        }
    }
    let action_nodes = divergent
        .iter()
        .chain(&unconstrained)
        .flat_map(|record| record["action_nodes"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let state_nodes = unconstrained
        .iter()
        .filter_map(|record| record["state_name"].as_str())
        .map(|state| format!("state:{state}"))
        .collect();
    SemanticReview {
        divergent,
        unconstrained,
        action_nodes,
        state_nodes,
    }
}

fn semantic_review_findings(review: &SemanticReview) -> Vec<Value> {
    let mut findings = Vec::new();
    for record in &review.divergent {
        let actions = record["actions"].as_array().expect("actions");
        let left = actions[0]["name"].as_str().unwrap_or_default();
        let right = actions[1]["name"].as_str().unwrap_or_default();
        let question = format!(
            "Both {left} and {right} are enabled in this reachable state and produce different contract outcomes. Which outcome is intended, or should both be explicitly allowed by an invariant or acceptance case?"
        );
        let involved = record["predicate_nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(record["action_nodes"].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<std::collections::BTreeSet<_>>();
        let mut finding = fsl_tools::review_finding(
            "divergent_choice",
            0.86,
            json!(involved),
            record.clone(),
            "Two different actions are enabled in the same bounded-reachable state, and choosing between them changes an invariant or acceptance predicate.",
            json!([{"kind":"ask_spec_question","template":question}]),
            json!([
                "Either action is wrong.",
                "The bounded witness proves the product must choose only one action.",
                "No deeper reachable choice exists beyond the analyzed bound."
            ]),
            None,
        );
        if let Value::Object(object) = &mut finding {
            object.insert("spec_question".to_owned(), json!(question));
            object.insert("evidence_basis".to_owned(), json!("bounded_bmc"));
        }
        findings.push(finding);
    }
    for record in &review.unconstrained {
        let actions = record["actions"].as_array().expect("actions");
        let left = actions[0]["name"].as_str().unwrap_or_default();
        let right = actions[1]["name"].as_str().unwrap_or_default();
        let state = record["state_name"].as_str().unwrap_or_default();
        let question = format!(
            "Both {left} and {right} can write different values to {state} in this reachable state, but no guard, property, ensures clause, or scenario constrains that value. What outcome is intended, or should both possibilities be declared explicitly?"
        );
        let actions = record["action_nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<std::collections::BTreeSet<_>>();
        let involved = std::iter::once(format!("state:{state}"))
            .chain(actions)
            .collect::<Vec<_>>();
        let mut finding = fsl_tools::review_finding(
            "unconstrained_effect",
            0.82,
            json!(involved),
            record.clone(),
            "A state value outside the contract-observation graph has multiple concrete next values from the same bounded-reachable state.",
            json!([{"kind":"ask_spec_question","template":question}]),
            json!([
                "The state variable is safe to delete.",
                "Either successor is incorrect.",
                "The bounded witness is an unbounded proof of freedom."
            ]),
            None,
        );
        if let Value::Object(object) = &mut finding {
            object.insert("spec_question".to_owned(), json!(question));
            object.insert("evidence_basis".to_owned(), json!("bounded_bmc"));
        }
        findings.push(finding);
    }
    findings
}

fn acknowledge_undecided_findings(model: &KernelModel, findings: &mut [Value]) {
    let undecided = fsl_tools::undecided_declarations(model);
    for finding in findings {
        if !matches!(
            finding["finding_type"].as_str(),
            Some("divergent_choice" | "unconstrained_effect")
        ) {
            continue;
        }
        let involved = finding["involved_nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        let acknowledged_by = undecided
            .iter()
            .filter(|declaration| {
                declaration["node"]
                    .as_str()
                    .is_some_and(|node| involved.contains(node))
            })
            .map(|declaration| {
                json!({
                    "declaration": declaration["declaration"],
                    "reason": declaration["reason"],
                })
            })
            .collect::<Vec<_>>();
        if acknowledged_by.is_empty() {
            continue;
        }
        if let Value::Object(object) = finding {
            object.insert("acknowledged".to_owned(), json!(true));
            object.insert("acknowledged_by".to_owned(), json!(acknowledged_by));
        }
    }
}

fn ai_review_output(model: &KernelModel, acceptance: &[(String, KernelExpr)]) -> Value {
    let tsg = fsl_tools::build_tsg(model);
    let mut findings = fsl_tools::structural_review_findings(&tsg);
    let unconstrained_states = findings
        .iter()
        .filter(|finding| finding["finding_type"] == "unread_state")
        .filter_map(|finding| finding["involved_nodes"].get(0)?.as_str())
        .filter_map(|node| node.strip_prefix("state:"))
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    let semantic = bounded_semantic_review(model, &unconstrained_states, acceptance);
    findings.retain(|finding| {
        !(finding["finding_type"] == "unread_state"
            && finding["involved_nodes"]
                .get(0)
                .and_then(Value::as_str)
                .is_some_and(|node| semantic.state_nodes.contains(node)))
    });
    findings.extend(ai_progressless_findings(model, &tsg));
    findings.extend(ai_conservation_findings(model));
    findings.extend(ai_tag_findings(model));
    for action in &model.actions {
        if !action.requires.is_empty()
            || semantic
                .action_nodes
                .contains(&format!("action:{}", action.name))
        {
            continue;
        }
        let action_id = format!("action:{}", action.name);
        let writes = tsg["edges"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|edge| edge["from"] == action_id && edge["kind"] == "writes")
            .filter_map(|edge| edge["to"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        findings.push(fsl_tools::review_finding(
            "unguarded_action",
            0.72,
            json!([action_id]),
            json!({"kind":"action_has_no_requires","node":action_id,"writes":writes}),
            "The action has no explicit requires clauses, so it is structurally enabled in every state unless generated lowering adds hidden constraints elsewhere.",
            json!([{"kind":"add_or_confirm_guard","template":"Add a requires clause if the action should be state-dependent, or tag/document why it is intentionally always enabled."}]),
            json!(["The action is wrong.","Always-enabled behavior is invalid.","The action is reachable in every semantic state without considering type bounds and invariants."]),
            Some(action.span.python_loc()),
        ));
    }
    findings.extend(semantic_review_findings(&semantic));
    acknowledge_undecided_findings(model, &mut findings);
    findings.sort_by_key(|finding| {
        (
            finding["finding_type"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            finding["involved_nodes"].to_string(),
        )
    });
    let mut counters = std::collections::BTreeMap::<String, usize>::new();
    for finding in &mut findings {
        let kind = finding["finding_type"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let count = counters.entry(kind.clone()).or_default();
        *count += 1;
        if let Value::Object(object) = finding {
            object.insert(
                "finding_id".to_owned(),
                json!(format!(
                    "STRUCT-{}-{count:04}",
                    kind.replace('_', "-").to_uppercase()
                )),
            );
        }
    }
    let mut output = envelope();
    output.insert("result".to_owned(), json!("analyzed"));
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("analysis".to_owned(), json!("structure"));
    output.insert("profile".to_owned(), json!("ai-review"));
    output.insert("schema_version".to_owned(), json!("analysis-findings.v0"));
    output.insert("findings".to_owned(), Value::Array(findings));
    Value::Object(output)
}

#[derive(Clone)]
struct ProjectAnalysisLayer {
    model: KernelModel,
    tsg: Value,
    nodes: std::collections::BTreeMap<String, Value>,
    covers: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
}

fn project_analysis_node(id: &str, kind: &str, name: &str) -> Value {
    json!({"id":id,"kind":kind,"name":name,"label":name})
}

fn project_analysis_edge(from: &str, kind: &str, to: &str) -> Value {
    json!({"id":format!("edge:{from}:{kind}:{to}"),"kind":kind,"from":from,"to":to})
}

fn insert_analysis_item(items: &mut std::collections::BTreeMap<String, Value>, value: Value) {
    if let Some(id) = value.get("id").and_then(Value::as_str) {
        items.entry(id.to_owned()).or_insert(value);
    }
}

fn prefixed_analysis_node(layer: &str, node: &Value) -> Value {
    let mut node = node.clone();
    if let Value::Object(object) = &mut node {
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        object.insert("id".to_owned(), json!(format!("{layer}:{id}")));
        object.insert("layer".to_owned(), json!(layer));
        if object.get("kind").and_then(Value::as_str) == Some("spec") {
            object.insert(
                "kind".to_owned(),
                json!(match layer {
                    "business" => "business_spec",
                    "requirements" => "requirements_spec",
                    "design" => "design_spec",
                    _ => "spec",
                }),
            );
        }
    }
    node
}

fn prefixed_analysis_edge(layer: &str, edge: &Value) -> Value {
    let mut edge = edge.clone();
    if let Value::Object(object) = &mut edge {
        let id = object.get("id").and_then(Value::as_str).unwrap_or_default();
        let from = object
            .get("from")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let to = object
            .get("to")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        object.insert("id".to_owned(), json!(format!("edge:{layer}:{id}")));
        object.insert("from".to_owned(), json!(format!("{layer}:{from}")));
        object.insert("to".to_owned(), json!(format!("{layer}:{to}")));
        object.insert("layer".to_owned(), json!(layer));
    }
    edge
}

fn add_requirements_layer_nodes(tsg: &mut Value, model: &KernelModel) {
    let mut requirements = std::collections::BTreeMap::<String, Vec<String>>::new();
    let graph_nodes = tsg["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|node| node["id"].as_str().map(str::to_owned))
        .collect::<std::collections::BTreeSet<_>>();
    for (target, links) in model.requirement_targets() {
        let graph_target = target
            .strip_prefix("property:")
            .unwrap_or(&target)
            .to_owned();
        for requirement in links {
            let targets = requirements.entry(requirement.id).or_default();
            if graph_nodes.contains(&graph_target) {
                targets.push(graph_target.clone());
            }
        }
    }
    let mut node_additions = Vec::new();
    let mut edge_additions = Vec::new();
    for (requirement, targets) in requirements {
        let id = format!("requirement:{requirement}");
        node_additions.push(project_analysis_node(&id, "requirement", &requirement));
        edge_additions.push(project_analysis_edge(
            &format!("spec:{}", model.name),
            "declares",
            &id,
        ));
        for target in targets {
            edge_additions.push(project_analysis_edge(&id, "covers", &target));
        }
    }
    if let Some(nodes) = tsg.get_mut("nodes").and_then(Value::as_array_mut) {
        nodes.extend(node_additions);
        nodes.sort_by_key(|node| node["id"].as_str().unwrap_or_default().to_owned());
    }
    if let Some(edges) = tsg.get_mut("edges").and_then(Value::as_array_mut) {
        edges.extend(edge_additions);
        edges.sort_by_key(|edge| edge["id"].as_str().unwrap_or_default().to_owned());
    }
}

#[allow(clippy::too_many_lines)]
fn project_traceability_output(path: &Path) -> Result<Value, String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let sections = parse_project_manifest(&source)?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let manifest = analysis_display_path(path);
    let mut nodes = std::collections::BTreeMap::new();
    let mut edges = std::collections::BTreeMap::new();
    let manifest_id = format!("file:{manifest}");
    let mut manifest_node = project_analysis_node(&manifest_id, "file", &manifest);
    manifest_node
        .as_object_mut()
        .expect("node object")
        .insert("path".to_owned(), json!(manifest));
    insert_analysis_item(&mut nodes, manifest_node);
    let mut layers = std::collections::BTreeMap::new();
    for layer in ["business", "requirements", "design"] {
        let Some(file) = sections
            .get(layer)
            .and_then(|section| section.values.get("file"))
        else {
            continue;
        };
        let layer_path = base.join(file);
        let model = load_model(&layer_path)?;
        let mut tsg = fsl_tools::build_tsg(&model);
        if layer == "requirements" {
            add_requirements_layer_nodes(&mut tsg, &model);
        }
        let display = analysis_display_path(&layer_path);
        let file_id = format!("file:{layer}:{display}");
        let mut file_node = project_analysis_node(&file_id, "file", &display);
        file_node
            .as_object_mut()
            .expect("node object")
            .insert("path".to_owned(), json!(display));
        insert_analysis_item(&mut nodes, file_node);
        let mut layer_nodes = std::collections::BTreeMap::new();
        for node in tsg["nodes"].as_array().into_iter().flatten() {
            let original_id = node["id"].as_str().unwrap_or_default().to_owned();
            layer_nodes.insert(original_id.clone(), node.clone());
            let prefixed = prefixed_analysis_node(layer, node);
            if node["kind"] == "spec" {
                insert_analysis_item(
                    &mut edges,
                    project_analysis_edge(
                        &file_id,
                        "declares",
                        prefixed["id"].as_str().unwrap_or_default(),
                    ),
                );
            }
            insert_analysis_item(&mut nodes, prefixed);
        }
        let mut covers =
            std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
        for edge in tsg["edges"].as_array().into_iter().flatten() {
            insert_analysis_item(&mut edges, prefixed_analysis_edge(layer, edge));
            if edge["kind"] == "covers"
                && edge["from"]
                    .as_str()
                    .is_some_and(|from| from.starts_with("requirement:"))
                && let (Some(from), Some(to)) = (edge["from"].as_str(), edge["to"].as_str())
            {
                covers
                    .entry(to.to_owned())
                    .or_default()
                    .insert(from.to_owned());
            }
        }
        layers.insert(
            layer.to_owned(),
            ProjectAnalysisLayer {
                model,
                tsg,
                nodes: layer_nodes,
                covers,
            },
        );
    }
    for layer in ["business", "requirements", "design"] {
        let Some(section) = sections.get(layer) else {
            continue;
        };
        let Some(target) = section.values.get("refine_against") else {
            continue;
        };
        let Some(mapping) = section.values.get("mapping") else {
            continue;
        };
        let (Some(implementation), Some(abstraction)) = (layers.get(layer), layers.get(target))
        else {
            continue;
        };
        let mapping_path = base.join(mapping);
        let document = parse_surface_document(&mapping_path)?;
        let fsl_syntax::SurfaceDocument::Refinement(refinement) = document else {
            return Err("expected refinement mapping".to_owned());
        };
        let mapping_source = std::fs::read_to_string(&mapping_path)
            .map_err(|error| format!("failed to read {}: {error}", mapping_path.display()))?;
        let checked_refinement =
            fsl_core::parse_refinement(&mapping_source, &implementation.model, &abstraction.model)
                .map_err(|error| error.message)?;
        let display = analysis_display_path(&mapping_path);
        let refinement_id = format!("refinement:{layer}->{target}:{}", refinement.name);
        let file_id = format!("file:{layer}->{target}:{display}");
        let mut file_node = project_analysis_node(&file_id, "file", &display);
        file_node
            .as_object_mut()
            .expect("node object")
            .insert("path".to_owned(), json!(display));
        insert_analysis_item(&mut nodes, file_node);
        let mut ref_node = project_analysis_node(&refinement_id, "refinement", &refinement.name);
        ref_node.as_object_mut().expect("node object").insert(
            "path".to_owned(),
            json!(analysis_display_path(&mapping_path)),
        );
        insert_analysis_item(&mut nodes, ref_node);
        insert_analysis_item(
            &mut edges,
            project_analysis_edge(&file_id, "declares", &refinement_id),
        );
        insert_analysis_item(
            &mut edges,
            project_analysis_edge(
                &refinement_id,
                "implements",
                &format!("{layer}:spec:{}", implementation.model.name),
            ),
        );
        insert_analysis_item(
            &mut edges,
            project_analysis_edge(
                &refinement_id,
                "abstracts",
                &format!("{target}:spec:{}", abstraction.model.name),
            ),
        );
        let impl_states = implementation
            .model
            .state
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for item in &refinement.items {
            match item {
                fsl_syntax::RefinementItem::Map {
                    name,
                    binder,
                    expr,
                    span,
                } => {
                    let map_id = format!("state_map:{layer}->{target}:{name}");
                    let mut map_node = project_analysis_node(&map_id, "state_map", name);
                    if let Value::Object(object) = &mut map_node {
                        object.insert("loc".to_owned(), span.python_loc());
                        object.insert("layer".to_owned(), json!(layer));
                        object.insert("target_layer".to_owned(), json!(target));
                    }
                    insert_analysis_item(&mut nodes, map_node);
                    insert_analysis_item(
                        &mut edges,
                        project_analysis_edge(&refinement_id, "declares", &map_id),
                    );
                    insert_analysis_item(
                        &mut edges,
                        project_analysis_edge(
                            &map_id,
                            "maps_state",
                            &format!("{target}:state:{name}"),
                        ),
                    );
                    let mut reads = expression_identifiers(&expr.python_ast());
                    reads.retain(|read| impl_states.contains(read));
                    if let Some(binder) = binder {
                        let binder = match binder {
                            fsl_syntax::Binder::Typed { name, .. }
                            | fsl_syntax::Binder::Range { name, .. }
                            | fsl_syntax::Binder::Collection { name, .. } => name,
                        };
                        reads.remove(binder);
                    }
                    for read in reads {
                        insert_analysis_item(
                            &mut edges,
                            project_analysis_edge(
                                &map_id,
                                "reads_impl_state",
                                &format!("{layer}:state:{read}"),
                            ),
                        );
                    }
                }
                fsl_syntax::RefinementItem::PreserveProgress { span, .. } => {
                    let id = format!("preserve_progress:{layer}->{target}:{}", refinement.name);
                    let mut node =
                        project_analysis_node(&id, "preserve_progress", "preserve progress");
                    node.as_object_mut()
                        .expect("node object")
                        .insert("loc".to_owned(), span.python_loc());
                    insert_analysis_item(&mut nodes, node);
                    insert_analysis_item(
                        &mut edges,
                        project_analysis_edge(&refinement_id, "preserves_progress", &id),
                    );
                }
                _ => {}
            }
        }
        for correspondence in checked_refinement.action_correspondences.values() {
            let name = &correspondence.impl_action.0;
            let map_id = format!("action_map:{layer}->{target}:{name}");
            let mut map_node = project_analysis_node(&map_id, "action_map", name);
            if let Value::Object(object) = &mut map_node {
                object.insert("loc".to_owned(), correspondence.span.python_loc());
                object.insert("layer".to_owned(), json!(layer));
                object.insert("target_layer".to_owned(), json!(target));
                object.insert("origin".to_owned(), json!(correspondence.origin.as_str()));
            }
            insert_analysis_item(&mut nodes, map_node);
            insert_analysis_item(
                &mut edges,
                project_analysis_edge(&refinement_id, "declares", &map_id),
            );
            let impl_id = format!("{layer}:action:{name}");
            insert_analysis_item(
                &mut edges,
                project_analysis_edge(&map_id, "maps_action", &impl_id),
            );
            match &correspondence.target {
                fsl_core::ActionCorrespondenceTarget::Stutter => {
                    let stutter_id = format!("stutter_map:{layer}->{target}:{name}");
                    let mut stutter = project_analysis_node(&stutter_id, "stutter_map", name);
                    stutter
                        .as_object_mut()
                        .expect("node object")
                        .insert("loc".to_owned(), correspondence.span.python_loc());
                    insert_analysis_item(&mut nodes, stutter);
                    insert_analysis_item(
                        &mut edges,
                        project_analysis_edge(&map_id, "stutters", &stutter_id),
                    );
                }
                fsl_core::ActionCorrespondenceTarget::Action { action, .. } => {
                    let abs_id = format!("{target}:action:{}", action.0);
                    insert_analysis_item(
                        &mut edges,
                        project_analysis_edge(&impl_id, "maps_action", &abs_id),
                    );
                    if let Some(requirements) =
                        abstraction.covers.get(&format!("action:{}", action.0))
                    {
                        for requirement in requirements {
                            let mut anchor = project_analysis_edge(
                                &format!("{target}:{requirement}"),
                                "lower_anchor",
                                &impl_id,
                            );
                            if let Value::Object(object) = &mut anchor {
                                object.insert("formal_status".to_owned(), json!("not_a_violation"));
                                object.insert("via".to_owned(), json!("refinement_action_map"));
                                object.insert("layer".to_owned(), json!(layer));
                            }
                            insert_analysis_item(&mut edges, anchor);
                        }
                    }
                }
            }
        }
    }
    for (upper, lower) in [("business", "requirements"), ("requirements", "design")] {
        let (Some(upper_layer), Some(lower_layer)) = (layers.get(upper), layers.get(lower)) else {
            continue;
        };
        for (id, node) in &upper_layer.nodes {
            if !matches!(node["kind"].as_str(), Some("requirement" | "control"))
                || !lower_layer.nodes.contains_key(id)
            {
                continue;
            }
            let mut anchor = project_analysis_edge(
                &format!("{upper}:{id}"),
                "lower_anchor",
                &format!("{lower}:{id}"),
            );
            if let Value::Object(object) = &mut anchor {
                object.insert("formal_status".to_owned(), json!("not_a_violation"));
                object.insert("via".to_owned(), json!("same_id"));
            }
            insert_analysis_item(&mut edges, anchor);
        }
    }
    let lower_anchors = edges
        .values()
        .filter(|edge| edge["kind"] == "lower_anchor")
        .filter_map(|edge| edge["from"].as_str().map(str::to_owned))
        .collect::<std::collections::BTreeSet<_>>();
    let mut findings = Vec::new();
    let mut counter = 0;
    for layer in ["business", "requirements"] {
        let Some(data) = layers.get(layer) else {
            continue;
        };
        for node in data.tsg["nodes"].as_array().into_iter().flatten() {
            if !matches!(node["kind"].as_str(), Some("requirement" | "control")) {
                continue;
            }
            let id = format!("{layer}:{}", node["id"].as_str().unwrap_or_default());
            if lower_anchors.contains(&id) {
                continue;
            }
            counter += 1;
            findings.push(json!({
                "finding_id":format!("STRUCT-TRACEABILITY-GAP-{counter:04}"),
                "analysis":"structure",
                "finding_type":"traceability_gap",
                "severity":"review_required",
                "confidence":0.74,
                "formal_status":"not_a_violation",
                "involved_nodes":[id],
                "witness":{"kind":"missing_lower_anchor","layer":layer,"node":id},
                "why_it_matters":"An upper-layer requirement/control ID has no visible lower-layer structural anchor in the project traceability graph.",
                "candidate_repairs":[{"kind":"add_lower_anchor","template":"Carry the ID into the lower layer, or map an abstract action/property to a lower-layer action through refinement."}],
                "do_not_assume":["The lower layer violates the upper-layer contract.","Name similarity proves semantic coverage."],
            }));
        }
    }
    let nodes = nodes.into_values().collect::<Vec<_>>();
    let edges = edges.into_values().collect::<Vec<_>>();
    let mut analysis = fsl_tools::complete_analysis_graph("traceability_graph", &nodes, &edges);
    if let Value::Object(object) = &mut analysis {
        object.insert("manifest".to_owned(), json!(analysis_display_path(path)));
        object.insert("findings".to_owned(), Value::Array(findings));
    }
    Ok(analysis)
}

fn analysis_acceptance_predicates(path: &Path) -> Vec<(String, KernelExpr)> {
    let Ok(fsl_syntax::SurfaceDocument::Requirements(requirements)) = parse_surface_document(path)
    else {
        return Vec::new();
    };
    requirements
        .items
        .into_iter()
        .filter_map(|item| match item {
            fsl_syntax::RequirementsItem::Acceptance {
                id,
                expectation: fsl_syntax::AcceptanceExpectation::Expr(expression, _),
                ..
            } => Some((id, expression)),
            _ => None,
        })
        .collect()
}

#[allow(clippy::too_many_lines)]
fn run_analyze(
    path: &Path,
    projection: &str,
    focus: Option<&str>,
    output_format: &str,
    profile: Option<&str>,
    export_kind: Option<&str>,
    code_path: Option<&Path>,
) -> (Value, i32) {
    if !matches!(output_format, "json" | "dot" | "mermaid") {
        return (
            error_output(
                "semantics",
                &format!("unsupported analyze format: {output_format}"),
            ),
            2,
        );
    }
    if projection == "code_audit" {
        if code_path.is_none() {
            return (
                error_output(
                    "semantics",
                    "--projection code_audit requires --code <dir|file>",
                ),
                2,
            );
        }
        if output_format != "json" || focus.is_some() || profile.is_some() || export_kind.is_some()
        {
            return (
                error_output(
                    "semantics",
                    "code_audit cannot be combined with --focus, --profile, --export, or non-JSON --format",
                ),
                2,
            );
        }
    } else if code_path.is_some() {
        return (
            error_output(
                "semantics",
                "--code is supported only with --projection code_audit",
            ),
            2,
        );
    }
    if focus.is_some() && profile.is_none() && projection != "impact_graph" {
        return (
            error_output(
                "semantics",
                "--focus is supported only with --projection impact_graph",
            ),
            2,
        );
    }
    if profile.is_some() && output_format != "json" {
        return (
            error_output(
                "semantics",
                "DOT/Mermaid export is supported for graph projections, not profiles",
            ),
            2,
        );
    }
    if profile.is_some() && focus.is_some() {
        return (
            error_output(
                "semantics",
                "--focus is supported only with graph projections, not profiles",
            ),
            2,
        );
    }
    if path.extension().and_then(std::ffi::OsStr::to_str) == Some("toml") {
        if export_kind.is_some() {
            return (
                error_output(
                    "semantics",
                    "tag-review export requires an FSL specification",
                ),
                2,
            );
        }
        if profile.is_some() {
            return (
                error_output(
                    "semantics",
                    "project traceability analysis does not support --profile",
                ),
                2,
            );
        }
        if projection != "traceability_graph" {
            return (
                error_output(
                    "semantics",
                    "project manifests support only --projection traceability_graph",
                ),
                2,
            );
        }
        let analysis = match project_traceability_output(path) {
            Ok(analysis) => analysis,
            Err(error) => return (error_output("semantics", &error), 2),
        };
        return finish_analysis(analysis, None, projection, output_format);
    }
    if let Some(export_kind) = export_kind {
        if export_kind != "tag-review" {
            return (
                error_output(
                    "semantics",
                    &format!("unsupported analyze export: {export_kind}"),
                ),
                2,
            );
        }
        if profile.is_some() || focus.is_some() || output_format != "json" || projection != "tsg" {
            return (
                error_output(
                    "semantics",
                    "--export tag-review cannot be combined with --profile, --focus, --projection, or non-JSON --format",
                ),
                2,
            );
        }
        let model = match load_model(path) {
            Ok(model) => model,
            Err(error) => return (semantic_error_output(&error), 2),
        };
        return (tag_review_output(&model), 0);
    }
    if let Ok(fsl_syntax::SurfaceDocument::Refinement(refinement)) = parse_surface_document(path) {
        if let Some(profile) = profile {
            let mut output = envelope();
            output.insert("result".to_owned(), json!("analyzed"));
            output.insert("refinement".to_owned(), json!(refinement.name));
            output.insert("analysis".to_owned(), json!("structure"));
            output.insert("profile".to_owned(), json!(profile));
            output.insert("schema_version".to_owned(), json!("analysis-findings.v0"));
            output.insert("findings".to_owned(), json!([]));
            return (Value::Object(output), 0);
        }
        if !matches!(projection, "tsg" | "refinement_graph") {
            return (
                error_output(
                    "semantics",
                    "refinement mappings support only --projection refinement_graph (or the default tsg alias)",
                ),
                2,
            );
        }
        let name = refinement.name.clone();
        return finish_analysis(
            fsl_tools::analyze_refinement(&refinement),
            Some(("refinement", &name)),
            projection,
            output_format,
        );
    }
    let model = match load_model(path) {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    if projection == "code_audit" {
        let analysis = match code_audit::analyze(&model, code_path.expect("checked above")) {
            Ok(analysis) => analysis,
            Err(code_audit::CodeAuditError::Io(message)) => {
                return (error_output("io", &message), 2);
            }
            Err(code_audit::CodeAuditError::Semantics(message)) => {
                return (error_output("semantics", &message), 2);
            }
        };
        return finish_analysis(
            analysis,
            Some(("spec", &model.name)),
            projection,
            output_format,
        );
    }
    if let Some(profile) = profile {
        if profile != "ai-review" {
            return (
                error_output("semantics", &format!("unsupported profile: {profile}")),
                2,
            );
        }
        let acceptance = analysis_acceptance_predicates(path);
        return (ai_review_output(&model, &acceptance), 0);
    }
    match fsl_tools::analyze_model(&model, projection, focus) {
        Ok(analysis @ Value::Object(_)) => finish_analysis(
            analysis,
            Some(("spec", &model.name)),
            projection,
            output_format,
        ),
        Err(message) => {
            let kind = if message.starts_with("unknown analyze focus node") {
                "name"
            } else {
                "semantics"
            };
            (error_output(kind, &message), 2)
        }
        Ok(_) => (
            error_output("internal", "analysis result must be an object"),
            3,
        ),
    }
}

fn collect_analysis_files(path: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            collect_analysis_files(&entry?.path(), files)?;
        }
    } else if path.extension().and_then(std::ffi::OsStr::to_str) == Some("fsl") {
        files.push(path.to_path_buf());
    }
    Ok(())
}

fn analysis_display_path(path: &Path) -> String {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let root = std::env::current_dir()
        .ok()
        .and_then(|path| path.canonicalize().ok());
    root.as_deref()
        .and_then(|root| resolved.strip_prefix(root).ok())
        .unwrap_or(&resolved)
        .to_string_lossy()
        .replace('\\', "/")
}

fn analysis_batch_entry(path: &Path, result: &Value) -> Value {
    let mut entry = Map::new();
    entry.insert("file".to_owned(), json!(analysis_display_path(path)));
    entry.insert(
        "result".to_owned(),
        result.get("result").cloned().unwrap_or(Value::Null),
    );
    for key in [
        "spec",
        "refinement",
        "projection",
        "profile",
        "schema_version",
        "formal_status",
    ] {
        if let Some(value) = result.get(key) {
            entry.insert(key.to_owned(), value.clone());
        }
    }
    if result.get("result").and_then(Value::as_str) == Some("analyzed") {
        let mut summary = Map::new();
        for key in [
            "nodes",
            "edges",
            "findings",
            "components",
            "cycles",
            "errors",
        ] {
            if let Some(values) = result.get(key).and_then(Value::as_array) {
                summary.insert(key.to_owned(), json!(values.len()));
            }
        }
        entry.insert("summary".to_owned(), Value::Object(summary));
        if result.get("profile").and_then(Value::as_str) == Some("ai-review") {
            entry.insert(
                "findings".to_owned(),
                result.get("findings").cloned().unwrap_or_else(|| json!([])),
            );
        }
    } else {
        for key in ["kind", "message", "loc", "expected", "hint"] {
            if let Some(value) = result.get(key) {
                entry.insert(key.to_owned(), value.clone());
            }
        }
    }
    Value::Object(entry)
}

fn run_analyze_batch(
    paths: &[PathBuf],
    projection: &str,
    focus: Option<&str>,
    output_format: &str,
    profile: Option<&str>,
    export_kind: Option<&str>,
    code_path: Option<&Path>,
) -> (Value, i32) {
    if projection == "code_audit" || code_path.is_some() {
        return (
            error_output(
                "semantics",
                "code_audit accepts exactly one specification file",
            ),
            2,
        );
    }
    if export_kind.is_some() {
        return (
            error_output(
                "semantics",
                "tag-review export accepts exactly one specification file",
            ),
            2,
        );
    }
    if output_format != "json" {
        return (
            error_output("semantics", "batch analyze supports only --format json"),
            2,
        );
    }
    if focus.is_some() {
        return (
            error_output(
                "semantics",
                "batch analyze does not support --focus; run impact_graph per file",
            ),
            2,
        );
    }
    let mut files = Vec::new();
    for path in paths {
        if !path.exists() {
            return (
                error_output("io", &format!("file not found: {}", path.display())),
                2,
            );
        }
        if let Err(error) = collect_analysis_files(path, &mut files) {
            return (error_output("io", &error.to_string()), 2);
        }
    }
    files.sort_by_key(|path| analysis_display_path(path));
    files.dedup_by(|left, right| {
        left.canonicalize().unwrap_or_else(|_| left.clone())
            == right.canonicalize().unwrap_or_else(|_| right.clone())
    });
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for path in files {
        let (result, _) = run_analyze(&path, projection, None, "json", profile, None, None);
        entries.push(analysis_batch_entry(&path, &result));
        if result.get("result").and_then(Value::as_str) != Some("analyzed") {
            errors.push(json!({
                "file":analysis_display_path(&path),
                "result":result.get("result"),
                "kind":result.get("kind"),
                "message":result.get("message"),
                "loc":result.get("loc"),
            }));
        }
    }
    let failed = !errors.is_empty();
    let mut output = envelope();
    output.insert(
        "result".to_owned(),
        json!(if failed { "error" } else { "analyzed" }),
    );
    output.insert("analysis".to_owned(), json!("structure"));
    output.insert("mode".to_owned(), json!("batch"));
    output.insert("projection".to_owned(), json!(projection));
    output.insert("profile".to_owned(), json!(profile));
    output.insert("files".to_owned(), Value::Array(entries));
    output.insert("errors".to_owned(), Value::Array(errors));
    if failed {
        output.insert("kind".to_owned(), json!("batch"));
        output.insert(
            "message".to_owned(),
            json!("one or more files failed structural analysis"),
        );
    }
    (Value::Object(output), i32::from(failed) * 2)
}

fn finish_analysis(
    analysis: Value,
    identity: Option<(&str, &str)>,
    projection: &str,
    output_format: &str,
) -> (Value, i32) {
    if output_format != "json" {
        if analysis["nodes"].as_array().is_none_or(Vec::is_empty) || !analysis["edges"].is_array() {
            return (
                error_output(
                    "semantics",
                    &format!("--format {output_format} requires a graph projection"),
                ),
                2,
            );
        }
        let content = match fsl_tools::export_analysis_graph(&analysis, output_format) {
            Ok(content) => content,
            Err(message) => return (error_output("semantics", &message), 2),
        };
        let mut output = envelope();
        output.insert("result".to_owned(), json!("analyzed"));
        output.insert(
            "analysis".to_owned(),
            analysis
                .get("analysis")
                .cloned()
                .unwrap_or_else(|| json!("structure")),
        );
        output.insert(
            "projection".to_owned(),
            analysis
                .get("projection")
                .cloned()
                .unwrap_or_else(|| json!(projection)),
        );
        output.insert("format".to_owned(), json!(output_format));
        output.insert("content".to_owned(), json!(content));
        return (Value::Object(output), 0);
    }
    let Value::Object(analysis) = analysis else {
        return (
            error_output("internal", "analysis result must be an object"),
            3,
        );
    };
    let mut output = envelope();
    output.insert("result".to_owned(), json!("analyzed"));
    if let Some((key, value)) = identity {
        output.insert(key.to_owned(), json!(value));
    }
    output.extend(analysis);
    (Value::Object(output), 0)
}

const DIFF_FINDING_KINDS: [&str; 7] = [
    "behavior_added",
    "behavior_removed",
    "invariant_weakened",
    "invariant_strengthened",
    "forbidden_relaxed",
    "scope_changed",
    "unknown",
];

fn diff_shape_mismatch(implementation: &KernelModel, abstraction: &KernelModel) -> Option<Value> {
    let implementation_state = implementation
        .state
        .iter()
        .map(|(name, _)| display(name))
        .collect::<std::collections::BTreeSet<_>>();
    let abstraction_state = abstraction
        .state
        .iter()
        .map(|(name, _)| display(name))
        .collect::<std::collections::BTreeSet<_>>();
    let implementation_actions = implementation
        .actions
        .iter()
        .map(|action| display(&action.name))
        .collect::<std::collections::BTreeSet<_>>();
    let abstraction_actions = abstraction
        .actions
        .iter()
        .map(|action| display(&action.name))
        .collect::<std::collections::BTreeSet<_>>();
    if implementation_state == abstraction_state && implementation_actions == abstraction_actions {
        return None;
    }
    Some(json!({
        "state":{
            "only_impl":implementation_state.difference(&abstraction_state).collect::<Vec<_>>(),
            "only_abs":abstraction_state.difference(&implementation_state).collect::<Vec<_>>(),
        },
        "actions":{
            "only_impl":implementation_actions.difference(&abstraction_actions).collect::<Vec<_>>(),
            "only_abs":abstraction_actions.difference(&implementation_actions).collect::<Vec<_>>(),
        },
    }))
}

fn semantic_diff_direction(
    implementation: &KernelModel,
    abstraction: &KernelModel,
    depth: usize,
    explicit_mapping: Option<&str>,
) -> Result<(Value, Option<Value>), String> {
    if explicit_mapping.is_none()
        && let Some(mismatch) = diff_shape_mismatch(implementation, abstraction)
    {
        return Ok((
            json!({
                "result":"unknown","reason":"state_or_action_names_differ",
                "mismatch":mismatch,
            }),
            None,
        ));
    }
    let automatic = explicit_mapping.is_none();
    let generated;
    let source = if let Some(source) = explicit_mapping {
        source
    } else {
        generated = format!(
            "refinement SemanticDiff {{ impl {} abs {} maps auto }}",
            implementation.name, abstraction.name
        );
        &generated
    };
    let mapping = match fsl_core::parse_refinement(source, implementation, abstraction) {
        Ok(mapping) => mapping,
        Err(error) if automatic => {
            return Ok((
                json!({
                    "result":"unknown","reason":"automatic_mapping_failed",
                    "message":error.message,
                }),
                None,
            ));
        }
        Err(error) => return Err(error.message),
    };
    let checked = match fsl_runtime::check_refinement(implementation, abstraction, &mapping, depth)
    {
        Ok(checked) => checked,
        Err(error) if automatic => {
            return Ok((
                json!({
                    "result":"unknown","reason":"automatic_mapping_failed",
                    "message":error.to_string(),
                }),
                None,
            ));
        }
        Err(error) => return Err(error.to_string()),
    };
    if let Some(failure) = checked.failure {
        let impl_action = failure.impl_action.as_ref().map(|action| {
            let definition = implementation
                .actions
                .iter()
                .find(|candidate| candidate.name == action.name);
            json!({
                "name":display(&action.name),
                "params":action.params.iter().map(|(name,value)|(
                    name.clone(),fslc_rust::fsl_value_json(value)
                )).collect::<Map<_,_>>(),
                "loc":definition.map(|definition|definition.span.python_loc()),
            })
        });
        let mismatch = mismatch_paths(
            abstraction,
            failure.alpha_after_expected.as_ref(),
            failure.alpha_after_actual.as_ref(),
        );
        let public = json!({
            "result":"refinement_failed","checked_to_depth":depth,
            "kind":failure.kind,"violated_at_step":failure.step,
        });
        let raw = json!({
            "kind":failure.kind,"violated_at_step":failure.step,
            "impl_action":impl_action,"mismatch":mismatch,
            "impl_trace":fslc_rust::trace_json(implementation,&failure.impl_trace),
        });
        return Ok((public, Some(raw)));
    }
    Ok((json!({"result":"refines","checked_to_depth":depth}), None))
}

fn diff_counterexample(raw: Option<&Value>) -> Value {
    let raw = raw.unwrap_or(&Value::Null);
    let mut violation = Map::new();
    for key in ["kind", "violated_at_step", "impl_action", "mismatch"] {
        if let Some(value) = raw.get(key) {
            violation.insert(key.to_owned(), value.clone());
        }
    }
    json!({
        "trace_type":"counterexample",
        "trace":raw.get("impl_trace").cloned().unwrap_or_else(||json!([])),
        "violation":violation,
    })
}

fn compare_diff_invariants(old: &KernelModel, new: &KernelModel) -> Vec<Value> {
    if old.state != new.state {
        return Vec::new();
    }
    let implication = |antecedent: &KernelModel, consequent: &KernelModel| {
        let mut solver = fsl_solver_z3::Z3Solver::new().map_err(|error| error.to_string())?;
        block_on_native(fsl_verifier::invariant_implication(
            antecedent,
            consequent,
            &mut solver,
        ))
        .map_err(|error| error.to_string())
    };
    let old_to_new = implication(old, new);
    let new_to_old = implication(new, old);
    match (old_to_new, new_to_old) {
        (
            Ok(fsl_verifier::ImplicationResult::Implied),
            Ok(fsl_verifier::ImplicationResult::Counterexample(state)),
        ) => vec![json!({
            "kind":"invariant_weakened",
            "witness":{"trace_type":"state_counterexample","state":fslc_rust::state_json(&state)},
        })],
        (
            Ok(fsl_verifier::ImplicationResult::Counterexample(state)),
            Ok(fsl_verifier::ImplicationResult::Implied),
        ) => vec![json!({
            "kind":"invariant_strengthened",
            "witness":{"trace_type":"state_counterexample","state":fslc_rust::state_json(&state)},
        })],
        (
            Ok(fsl_verifier::ImplicationResult::Implied),
            Ok(fsl_verifier::ImplicationResult::Implied),
        ) => Vec::new(),
        (Ok(left), Ok(right)) => vec![json!({
            "kind":"unknown","subject":"invariants",
            "reason":"invariant_sets_are_incomparable",
            "old_to_new":format!("{left:?}"),"new_to_old":format!("{right:?}"),
        })],
        (left, right) => vec![json!({
            "kind":"unknown","subject":"invariants",
            "reason":"invariant_implication_failed",
            "message":left.err().or_else(||right.err()).unwrap_or_default(),
        })],
    }
}

fn forbidden_diff_findings(old_path: &Path, new: &KernelModel) -> Vec<Value> {
    let Ok(source) = std::fs::read_to_string(old_path) else {
        return Vec::new();
    };
    let Ok(Some(contract)) = fsl_core::requirements_trace_contract(&source) else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    for case in contract.forbidden {
        let Ok(mut monitor) = fsl_runtime::Monitor::new(new.clone()) else {
            continue;
        };
        let mut accepted_trace = vec![json!({
            "step":0,"state":fslc_rust::state_json(&monitor.state),
        })];
        let mut accepted_final = false;
        for (index, step) in case.steps.iter().enumerate() {
            let Ok((arguments, instance)) = requirement_step_match(&monitor, step) else {
                break;
            };
            let Some(instance) = instance else {
                break;
            };
            let Ok(stepped) = monitor.step(&instance) else {
                break;
            };
            if stepped.violation.is_some() {
                break;
            }
            accepted_trace.push(json!({
                "step":index+1,"state":fslc_rust::state_json(&monitor.state),
                "action":{"name":display(&instance.action),
                    "params":instance.params.iter().map(|(name,value)|(
                        name.clone(),fslc_rust::fsl_value_json(value)
                    )).collect::<Map<_,_>>()},
            }));
            accepted_final = index + 1 == case.steps.len();
            let _ = arguments;
        }
        if accepted_final {
            findings.push(json!({
                "kind":"forbidden_relaxed","id":case.id,
                "witness":{
                    "trace_type":"counterexample","trace":accepted_trace,
                    "accepted_step":case.steps.last().map(|step|step.name.clone()),
                    "state":fslc_rust::state_json(&monitor.state),
                },
            }));
        }
    }
    findings
}

fn add_verify_items(scope: &mut ScopeBounds, items: &[fsl_syntax::VerifyItem]) {
    for item in items {
        match item {
            fsl_syntax::VerifyItem::Instances(name, value, _) => {
                scope.instances.insert(name.clone(), *value);
            }
            fsl_syntax::VerifyItem::Values(name, lo, hi, _) => {
                if let (fsl_syntax::Expr::Num(lo), fsl_syntax::Expr::Num(hi)) =
                    (lo.as_ref(), hi.as_ref())
                {
                    scope.values.insert(name.clone(), (*lo, *hi));
                }
            }
        }
    }
}

fn declared_scope(source: &str) -> ScopeBounds {
    let Ok(document) = fsl_syntax::parse_surface_document(source) else {
        return ScopeBounds::default();
    };
    let mut scope = ScopeBounds::default();
    let mut add_spec_item = |item: &fsl_syntax::SpecItem| {
        if let fsl_syntax::SpecItem::VerifyBounds { items, .. } = item {
            add_verify_items(&mut scope, items);
        }
    };
    match &document {
        fsl_syntax::SurfaceDocument::Spec(spec) => {
            for item in &spec.items {
                add_spec_item(item);
            }
        }
        fsl_syntax::SurfaceDocument::Requirements(requirements) => {
            for item in &requirements.items {
                if let fsl_syntax::RequirementsItem::Common(item) = item {
                    add_spec_item(item);
                }
            }
        }
        fsl_syntax::SurfaceDocument::Compose(compose) => {
            for item in &compose.items {
                if let fsl_syntax::ComposeItem::Common(item) = item {
                    add_spec_item(item);
                }
            }
        }
        fsl_syntax::SurfaceDocument::Business(business) => {
            for item in &business.items {
                if let fsl_syntax::BusinessItem::VerifyBounds { items, .. } = item {
                    add_verify_items(&mut scope, items);
                }
            }
        }
        _ => {}
    }
    scope
}

fn public_scope(scope: &ScopeBounds) -> Value {
    json!({
        "instances":scope.instances,
        "values":scope.values.iter().map(|(name,(lo,hi))|(
            name.clone(),json!([lo,hi])
        )).collect::<Map<_,_>>(),
    })
}

#[allow(clippy::too_many_lines)]
fn run_diff(
    old: &Path,
    new: &Path,
    depth: usize,
    mapping: Option<&Path>,
    forbid: &[String],
) -> (Value, i32) {
    let old_source = match std::fs::read_to_string(old) {
        Ok(source) => source,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    let new_source = match std::fs::read_to_string(new) {
        Ok(source) => source,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    let old_scope = declared_scope(&old_source);
    let new_scope = declared_scope(&new_source);
    let scope_changed = old_scope != new_scope;
    let overrides = ScopeBounds {
        instances: new_scope
            .instances
            .iter()
            .filter(|(name, _)| old_scope.instances.contains_key(*name))
            .map(|(name, value)| (name.clone(), *value))
            .collect(),
        values: new_scope
            .values
            .iter()
            .filter(|(name, _)| old_scope.values.contains_key(*name))
            .map(|(name, value)| (name.clone(), *value))
            .collect(),
    };
    let old_model = match if scope_changed {
        load_model_scoped(old, &overrides)
    } else {
        load_model(old)
    } {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let new_model = match load_model(new) {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let mapping_source = match mapping.map(std::fs::read_to_string).transpose() {
        Ok(source) => source,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    let mut explicit_direction = None;
    if let Some(source) = mapping_source.as_deref() {
        let surface = match fsl_syntax::parse_surface_document(source) {
            Ok(fsl_syntax::SurfaceDocument::Refinement(surface)) => surface,
            Ok(_) => return (error_output("type", "expected refinement mapping file"), 2),
            Err(error) => return (error_output("parse", &error.to_string()), 2),
        };
        let implementation = surface.items.iter().find_map(|item| match item {
            fsl_syntax::RefinementItem::Impl(name) => Some(name.as_str()),
            _ => None,
        });
        let abstraction = surface.items.iter().find_map(|item| match item {
            fsl_syntax::RefinementItem::Abs(name) => Some(name.as_str()),
            _ => None,
        });
        explicit_direction = match (implementation, abstraction) {
            (Some(implementation), Some(abstraction))
                if implementation == new_model.name && abstraction == old_model.name =>
            {
                Some("new_to_old")
            }
            (Some(implementation), Some(abstraction))
                if implementation == old_model.name && abstraction == new_model.name =>
            {
                Some("old_to_new")
            }
            _ => {
                return (
                    error_output("type", "diff mapping must map NEW to OLD or OLD to NEW"),
                    2,
                );
            }
        };
    }
    let (new_public, new_raw) = match semantic_diff_direction(
        &new_model,
        &old_model,
        depth,
        (explicit_direction == Some("new_to_old"))
            .then_some(mapping_source.as_deref())
            .flatten(),
    ) {
        Ok(result) => result,
        Err(error) => return (error_output("type", &error), 2),
    };
    let (old_public, old_raw) = match semantic_diff_direction(
        &old_model,
        &new_model,
        depth,
        (explicit_direction == Some("old_to_new"))
            .then_some(mapping_source.as_deref())
            .flatten(),
    ) {
        Ok(result) => result,
        Err(error) => return (error_output("type", &error), 2),
    };
    let mut findings = Vec::new();
    for (direction, public, raw, kind) in [
        (
            "new_to_old",
            &new_public,
            new_raw.as_ref(),
            "behavior_added",
        ),
        (
            "old_to_new",
            &old_public,
            old_raw.as_ref(),
            "behavior_removed",
        ),
    ] {
        match public.get("result").and_then(Value::as_str) {
            Some("refinement_failed") => findings.push(json!({
                "kind":kind,"direction":direction,"witness":diff_counterexample(raw),
            })),
            Some("unknown") => findings.push(json!({
                "kind":"unknown","direction":direction,
                "reason":public.get("reason").cloned().unwrap_or(Value::Null),
                "detail":public.get("mismatch").or_else(||public.get("detail"))
                    .or_else(||public.get("message")).cloned().unwrap_or(Value::Null),
            })),
            _ => {}
        }
    }
    findings.extend(compare_diff_invariants(&old_model, &new_model));
    findings.extend(forbidden_diff_findings(old, &new_model));
    if scope_changed {
        findings.push(json!({
            "kind":"scope_changed","old":public_scope(&old_scope),
            "new":public_scope(&new_scope),"comparison":"new",
        }));
    }
    let present = findings
        .iter()
        .filter_map(|finding| finding.get("kind").and_then(Value::as_str))
        .collect::<std::collections::BTreeSet<_>>();
    let summary = DIFF_FINDING_KINDS
        .iter()
        .filter(|kind| present.contains(**kind))
        .map(|kind| Value::String((*kind).to_owned()))
        .collect::<Vec<_>>();
    let unknown_forbid = forbid
        .iter()
        .filter(|kind| !DIFF_FINDING_KINDS.contains(&kind.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !unknown_forbid.is_empty() {
        return (
            error_output(
                "semantics",
                &format!(
                    "unknown --forbid finding kind(s): {}",
                    unknown_forbid.join(", ")
                ),
            ),
            2,
        );
    }
    let mut forbidden = forbid.to_vec();
    forbidden.sort();
    forbidden.dedup();
    let violations = forbidden
        .iter()
        .filter(|kind| present.contains(kind.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let mut output = envelope();
    output.insert("result".to_owned(), json!("semantic_diff"));
    output.insert(
        "old".to_owned(),
        json!({"file":old.display().to_string(),"spec":old_model.name}),
    );
    output.insert(
        "new".to_owned(),
        json!({"file":new.display().to_string(),"spec":new_model.name}),
    );
    output.insert(
        "bounded".to_owned(),
        json!({"depth":depth,"completeness":"bounded"}),
    );
    output.insert(
        "scope".to_owned(),
        json!({
            "old":public_scope(&old_scope),"new":public_scope(&new_scope),"comparison":"new",
            "applied_to_old":public_scope(&overrides),
        }),
    );
    output.insert(
        "directions".to_owned(),
        json!({"new_to_old":new_public,"old_to_new":old_public}),
    );
    output.insert(
        "summary".to_owned(),
        if summary.is_empty() {
            json!(["no_semantic_change"])
        } else {
            Value::Array(summary)
        },
    );
    output.insert("findings".to_owned(), Value::Array(findings));
    output.insert(
        "gate".to_owned(),
        json!({"forbidden":forbidden,"violations":violations,"passed":violations.is_empty()}),
    );
    let status = i32::from(!violations.is_empty());
    (Value::Object(output), status)
}

fn git_stdout(arguments: &[&str], cwd: &Path) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(arguments)
        .current_dir(cwd)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn materialize_git_tree(repo: &Path, revision: &str, destination: &Path) -> Result<(), String> {
    std::fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    let archive = destination.with_extension("tar");
    let output = std::process::Command::new("git")
        .args(["archive", "--format=tar", "--output"])
        .arg(&archive)
        .arg(revision)
        .current_dir(repo)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let output = std::process::Command::new("tar")
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(destination)
        .output()
        .map_err(|error| error.to_string())?;
    let _ = std::fs::remove_file(&archive);
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn run_approval_diff(
    path: &Path,
    record_path: &Path,
    depth: usize,
    trust_keys: &[PathBuf],
) -> (Value, i32) {
    let versioned = match approval::read_versioned_record(record_path) {
        Ok(record) => record,
        Err(error) => return (error_output("io", &error), 2),
    };
    let trust = match approval::TrustStore::load(trust_keys) {
        Ok(trust) => trust,
        Err(error) => return (error_output("io", &error), 2),
    };
    let (record, signature_status, key_id) = match &versioned {
        approval::VersionedApprovalRecord::V1(record) => (record.clone(), "unsigned", Value::Null),
        approval::VersionedApprovalRecord::V2(record) => match trust.verify(record) {
            Ok(true) => (
                versioned.binding(),
                "signed",
                json!(record.signature.key_id),
            ),
            Ok(false) => {
                let mut output = envelope();
                output.insert("result".to_owned(), json!("approval_diff"));
                output.insert("status".to_owned(), json!("signature-invalid"));
                output.insert("signature_status".to_owned(), json!("signature-invalid"));
                output.insert("key_id".to_owned(), json!(record.signature.key_id));
                return (Value::Object(output), 1);
            }
            Err(error) => return (error_output("io", &error), 2),
        },
    };
    let (repo, relative_path, _head) = match approval::git_location(path) {
        Ok(location) => location,
        Err(error) => return (error_output("io", &error), 2),
    };
    if relative_path != record.spec.path {
        return (
            error_output(
                "semantics",
                &format!(
                    "approval record targets '{}' but current spec is '{relative_path}'",
                    record.spec.path
                ),
            ),
            2,
        );
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let root =
        std::env::temp_dir().join(format!("fslc-approval-diff-{}-{nonce}", std::process::id()));
    let base_tree = root.join("base");
    if let Err(error) = materialize_git_tree(&repo, &record.spec.git_commit, &base_tree) {
        let _ = std::fs::remove_dir_all(&root);
        return (error_output("io", &error), 2);
    }
    let old = base_tree.join(&record.spec.path);
    if !old.is_file() {
        let _ = std::fs::remove_dir_all(&root);
        return (
            error_output(
                "io",
                &format!(
                    "approved spec '{}' is missing from baseline {}",
                    record.spec.path, record.spec.git_commit
                ),
            ),
            2,
        );
    }
    let materialized_digest = match approval::spec_digest(&old) {
        Ok(digest) => digest,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&root);
            return (semantic_error_output(&error), 2);
        }
    };
    if materialized_digest != record.spec.digest {
        let _ = std::fs::remove_dir_all(&root);
        return (
            error_output(
                "semantics",
                "approval baseline commit does not match the recorded specification digest",
            ),
            2,
        );
    }
    let (mut result, status) = run_diff(&old, path, depth, None, &[]);
    if let Value::Object(output) = &mut result {
        if let Some(Value::Object(old)) = output.get_mut("old") {
            old.insert(
                "file".to_owned(),
                json!(format!("{}:{}", record.spec.git_commit, record.spec.path)),
            );
        }
        output.insert(
            "approval".to_owned(),
            json!({
                "record": record_path.display().to_string(),
                "baseline_digest": record.spec.digest,
                "baseline_commit": record.spec.git_commit,
                "signature_status": signature_status,
                "key_id": key_id,
            }),
        );
    }
    let _ = std::fs::remove_dir_all(&root);
    (result, status)
}

#[allow(clippy::too_many_lines)]
fn run_diff_git(
    range: &str,
    spec: Option<&Path>,
    depth: usize,
    mapping: Option<&Path>,
    forbid: &[String],
) -> (Value, i32) {
    let Some((base_input, head_input)) = range.split_once("..") else {
        return (error_output("io", "--git range must be BASE..HEAD"), 2);
    };
    if base_input.is_empty() || head_input.is_empty() || head_input.contains("..") {
        return (error_output("io", "--git range must be BASE..HEAD"), 2);
    }
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    let repo = match git_stdout(&["rev-parse", "--show-toplevel"], &cwd) {
        Ok(repo) => PathBuf::from(repo),
        Err(error) => return (error_output("io", &error), 2),
    };
    let base = match git_stdout(&["rev-parse", "--verify", base_input], &repo) {
        Ok(commit) => commit,
        Err(error) => return (error_output("io", &error), 2),
    };
    let head = match git_stdout(&["rev-parse", "--verify", head_input], &repo) {
        Ok(commit) => commit,
        Err(error) => return (error_output("io", &error), 2),
    };
    let specs = if let Some(spec) = spec {
        let relative = spec
            .strip_prefix(&repo)
            .unwrap_or(spec)
            .to_string_lossy()
            .to_string();
        vec![relative]
    } else {
        let changed = match git_stdout(
            &[
                "diff",
                "--name-only",
                "--diff-filter=AMR",
                &base,
                &head,
                "--",
                "*.fsl",
            ],
            &repo,
        ) {
            Ok(changed) => changed,
            Err(error) => return (error_output("io", &error), 2),
        };
        changed
            .lines()
            .filter(|path| {
                Path::new(path)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("fsl"))
            })
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    if specs.is_empty() {
        return (
            error_output("io", "Git range contains no changed .fsl files"),
            2,
        );
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let root = std::env::temp_dir().join(format!("fslc-diff-{}-{nonce}", std::process::id()));
    let base_tree = root.join("base");
    let head_tree = root.join("head");
    if let Err(error) = materialize_git_tree(&repo, &base, &base_tree)
        .and_then(|()| materialize_git_tree(&repo, &head, &head_tree))
    {
        let _ = std::fs::remove_dir_all(&root);
        return (error_output("io", &error), 2);
    }
    let vcs = json!({
        "kind":"git","range":range,
        "base":{"revision":base_input,"commit":base},
        "head":{"revision":head_input,"commit":head},
        "materialization":"git_archive_full_tree",
    });
    let mut comparisons = Vec::new();
    let mut status = 0;
    for relative in &specs {
        let old = base_tree.join(relative);
        let new = head_tree.join(relative);
        if !old.is_file() || !new.is_file() {
            let _ = std::fs::remove_dir_all(&root);
            return (
                error_output(
                    "io",
                    &format!("'{relative}' must exist in both revisions for semantic diff"),
                ),
                2,
            );
        }
        let resolved_mapping = mapping.map(|path| {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                let candidate = head_tree.join(path);
                if candidate.is_file() {
                    candidate
                } else {
                    path.to_path_buf()
                }
            }
        });
        let (mut comparison, comparison_status) =
            run_diff(&old, &new, depth, resolved_mapping.as_deref(), forbid);
        status = status.max(comparison_status);
        if let Value::Object(output) = &mut comparison {
            output.insert(
                "old".to_owned(),
                json!({
                    "file":format!("{base_input}:{relative}"),
                    "spec":output.get("old").and_then(|old|old.get("spec")).cloned().unwrap_or(Value::Null),
                }),
            );
            output.insert(
                "new".to_owned(),
                json!({
                    "file":format!("{head_input}:{relative}"),
                    "spec":output.get("new").and_then(|new|new.get("spec")).cloned().unwrap_or(Value::Null),
                }),
            );
            output.insert("vcs".to_owned(), vcs.clone());
        }
        comparisons.push(comparison);
    }
    let _ = std::fs::remove_dir_all(&root);
    if spec.is_some() {
        return (comparisons.remove(0), status);
    }
    let violations = comparisons
        .iter()
        .flat_map(|comparison| {
            comparison
                .get("gate")
                .and_then(|gate| gate.get("violations"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    (
        json!({
            "fsl":"1.0","result":"semantic_diff_batch","vcs":vcs,"specs":specs,
            "comparisons":comparisons,
            "gate":{"violations":violations,"passed":violations.is_empty()},
        }),
        status,
    )
}

fn validate_requirement_traces(
    path: &Path,
    model: &KernelModel,
) -> Result<(Option<Value>, bool), String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    validate_requirement_trace_source(&source, model)
}

fn requirement_step_match(
    monitor: &fsl_runtime::Monitor,
    step: &fsl_core::RequirementsTraceStep,
) -> Result<(Vec<FslValue>, Option<fsl_runtime::EnabledAction>), String> {
    fslc_rust::verification_output::requirement_step_match(monitor, step)
}

fn validate_requirement_trace_source(
    source: &str,
    model: &KernelModel,
) -> Result<(Option<Value>, bool), String> {
    fslc_rust::verification_output::validate_requirement_trace_source(&envelope(), source, model)
}

fn governance_result(
    path: &Path,
    depth: usize,
) -> Result<Option<Value>, fslc_rust::verification_output::GovernanceOutputError> {
    let source = std::fs::read_to_string(path).map_err(|error| {
        fslc_rust::verification_output::GovernanceOutputError::new(error.to_string(), 1, 1)
    })?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let resolver = fsl_core::FsResolver::new(base);
    fslc_rust::verification_output::governance_output(&source, &resolver, |preservation| {
        let (result, status) = run_refine(
            &base.join(&preservation.after_path),
            &base.join(&preservation.before_path),
            &base.join(&preservation.refinement_path),
            depth,
        );
        if status > 1 {
            return Err(fslc_rust::verification_output::GovernanceOutputError::new(
                result
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("governance preservation refinement failed"),
                preservation.span.start.line,
                preservation.span.start.column,
            ));
        }
        Ok(result
            .get("result")
            .cloned()
            .unwrap_or_else(|| json!("error")))
    })
}

fn implements_result(
    path: &Path,
    model: &KernelModel,
    depth: usize,
) -> Result<Option<Value>, String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let resolver = fsl_core::FsResolver::new(path.parent().unwrap_or_else(|| Path::new(".")));
    fslc_rust::verification_output::requirements_implements_output(&source, &resolver, model, depth)
}

fn mismatch_paths(
    model: &KernelModel,
    expected: Option<&std::collections::BTreeMap<String, FslValue>>,
    actual: Option<&std::collections::BTreeMap<String, FslValue>>,
) -> Vec<String> {
    fn collect(
        model: &KernelModel,
        ty: &TypeRef,
        expected: Option<&FslValue>,
        actual: Option<&FslValue>,
        path: &str,
        paths: &mut Vec<String>,
    ) {
        if expected == actual {
            return;
        }
        if let (
            TypeRef::Map(_, value_ty),
            Some(FslValue::Map(expected)),
            Some(FslValue::Map(actual)),
        ) = (ty, expected, actual)
        {
            let mut keys = expected.keys().chain(actual.keys()).collect::<Vec<_>>();
            keys.sort();
            keys.dedup();
            for key in keys {
                let key_text = display_binding(key);
                let expected_value = expected.get(key);
                let actual_value = actual.get(key);
                if expected_value == actual_value {
                    continue;
                }
                if let (
                    TypeRef::Named(type_name),
                    Some(FslValue::Struct {
                        fields: expected_fields,
                        ..
                    }),
                    Some(FslValue::Struct {
                        fields: actual_fields,
                        ..
                    }),
                ) = (value_ty.as_ref(), expected_value, actual_value)
                    && let Some(fields) = model.struct_fields(type_name)
                {
                    for (field, _) in fields {
                        if expected_fields.get(field) != actual_fields.get(field) {
                            paths.push(format!("{path}[{key_text}].{field}"));
                        }
                    }
                    continue;
                }
                paths.push(format!("{path}[{key_text}]"));
            }
            return;
        }
        paths.push(path.to_owned());
    }

    let mut paths = Vec::new();
    for (name, ty) in &model.state {
        collect(
            model,
            ty,
            expected.and_then(|state| state.get(name)),
            actual.and_then(|state| state.get(name)),
            &display(name),
            &mut paths,
        );
    }
    paths
}

#[allow(clippy::too_many_lines)]
fn run_refine(
    implementation_path: &Path,
    abstraction_path: &Path,
    mapping_path: &Path,
    depth: usize,
) -> (Value, i32) {
    let implementation = match load_model(implementation_path) {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let abstraction = match load_model(abstraction_path) {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    let source = match std::fs::read_to_string(mapping_path) {
        Ok(source) => source,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    let mapping = match fsl_core::parse_refinement(&source, &implementation, &abstraction) {
        Ok(mapping) => mapping,
        Err(error) => return (error_output("type", &error.message), 2),
    };
    let checked =
        match fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, depth) {
            Ok(checked) => checked,
            Err(error) => return (error_output("type", &error.to_string()), 2),
        };
    let progress = if checked.failure.is_none() && !mapping.progress.is_empty() {
        let mut solver = match fsl_solver_z3::Z3Solver::new() {
            Ok(solver) => solver,
            Err(error) => return (error_output("internal", &error.to_string()), 3),
        };
        match block_on_native(fsl_verifier::check_refinement_progress(
            &implementation,
            &abstraction,
            &mapping,
            &mut solver,
            depth,
        )) {
            Ok(progress) => Some(progress),
            Err(error) => return (error_output("semantics", &error.to_string()), 2),
        }
    } else {
        None
    };
    let mut output = envelope();
    output.insert("impl".to_owned(), json!(checked.implementation));
    output.insert("abs".to_owned(), json!(checked.abstraction));
    if let Some(failure) = checked.failure {
        output.insert("result".to_owned(), json!("refinement_failed"));
        output.insert("kind".to_owned(), json!(failure.kind));
        if let Some(at) = failure.at {
            output.insert("at".to_owned(), json!(at));
        }
        output.insert("violated_at_step".to_owned(), json!(failure.step));
        if let Some(action) = failure.impl_action {
            let definition = implementation
                .actions
                .iter()
                .find(|definition| definition.name == action.name);
            output.insert(
                "impl_action".to_owned(),
                json!({
                    "name": display(&action.name),
                    "params": action.params.iter().map(|(name, value)| (
                        name.clone(), fslc_rust::fsl_value_json(value)
                    )).collect::<Map<_, _>>(),
                    "loc": definition.map(|definition| definition.span.python_loc()),
                }),
            );
        }
        if let Some(state) = &failure.alpha_before {
            output.insert("abs_before".to_owned(), fslc_rust::state_json(state));
        }
        if let Some(state) = &failure.alpha_after_expected {
            output.insert(
                "abs_after_expected".to_owned(),
                fslc_rust::state_json(state),
            );
        }
        if let Some(state) = &failure.alpha_after_actual {
            output.insert("abs_after_actual".to_owned(), fslc_rust::state_json(state));
        }
        output.insert(
            "mismatch".to_owned(),
            Value::Array(
                mismatch_paths(
                    &abstraction,
                    failure.alpha_after_expected.as_ref(),
                    failure.alpha_after_actual.as_ref(),
                )
                .into_iter()
                .map(Value::String)
                .collect(),
            ),
        );
        output.insert(
            "impl_trace".to_owned(),
            fslc_rust::trace_json(&implementation, &failure.impl_trace),
        );
        output.insert(
            "hint".to_owned(),
            json!("the impl step does not correspond to the mapped abs action; fix the map expressions, the action correspondence, or guard the impl action"),
        );
        output.insert("trace_type".to_owned(), json!("refinement"));
        return (Value::Object(output), 1);
    }
    if let Some(violation) = progress
        .as_ref()
        .and_then(|progress| progress.violation.as_ref())
    {
        let Some(details) = &violation.leads_to else {
            return (
                error_output("internal", "missing refinement progress diagnostics"),
                3,
            );
        };
        if let Err(error) = fsl_runtime::replay_trace(implementation.clone(), &violation.trace) {
            return (error_output("internal", &error.to_string()), 3);
        }
        let declaration = mapping
            .progress
            .iter()
            .find(|declaration| declaration.leads_to == violation.name);
        let property = abstraction
            .leadstos
            .iter()
            .find(|property| property.name == violation.name);
        output.insert("result".to_owned(), json!("refinement_failed"));
        output.insert("kind".to_owned(), json!("progress_lost"));
        output.insert(
            "progress_failure".to_owned(),
            json!(if details.stutter {
                "deadlock_or_stall_blocks_progress"
            } else {
                "lasso_blocks_progress"
            }),
        );
        output.insert("violation_kind".to_owned(), json!("leadsTo"));
        output.insert("invariant".to_owned(), json!(display(&violation.name)));
        output.insert(
            "bindings".to_owned(),
            Value::Object(
                details
                    .bindings
                    .iter()
                    .map(|(name, value)| (name.clone(), fslc_rust::fsl_value_json(value)))
                    .collect(),
            ),
        );
        output.insert("pending_since".to_owned(), json!(details.pending_since));
        output.insert("stutter".to_owned(), json!(details.stutter));
        output.insert(
            "impl_trace".to_owned(),
            fslc_rust::trace_json(&implementation, &violation.trace),
        );
        output.insert(
            "progress".to_owned(),
            json!({
                "leadsTo": display(&violation.name),
                "actions": declaration
                    .map(|item| item.actions.iter().map(|action| action.0.clone()).collect::<Vec<_>>())
                    .unwrap_or_default(),
            }),
        );
        output.insert(
            "hint".to_owned(),
            json!("the impl refines the abstract safety contract, but admits an execution where the pulled-back abstract leadsTo remains pending. Fairness must come from lower-layer `fair action` declarations for the implementation actions named by preserve progress; action mappings do not create fairness or prove implementation conformance by themselves"),
        );
        if let Some(property) = property {
            output.insert("loc".to_owned(), property.span.python_loc());
        }
        if let Some(loop_start) = details.loop_start {
            output.insert("loop_start".to_owned(), json!(loop_start));
        }
        output.insert(
            "faithfulness_class".to_owned(),
            json!("liveness_not_refined"),
        );
        output.insert(
            "recommended_action".to_owned(),
            json!(
                "re-prove liveness at each layer or add preserve progress to the refinement mapping"
            ),
        );
        output.insert("trace_type".to_owned(), json!("refinement"));
        return (Value::Object(output), 1);
    }
    output.insert("result".to_owned(), json!("refines"));
    output.insert("checked_to_depth".to_owned(), json!(checked.depth));
    output.insert(
        "action_map".to_owned(),
        Value::Object(
            checked
                .action_map
                .iter()
                .map(|(name, target)| (display(name), json!(display(target))))
                .collect(),
        ),
    );
    if checked.abs_has_ensures {
        output.insert(
            "note".to_owned(),
            json!("abs ensures are not checked during refinement; verify/prove the abstract spec separately"),
        );
    }
    if let Some(progress) = progress
        && !progress.checked.is_empty()
    {
        output.insert(
            "progress".to_owned(),
            Value::Object(
                progress
                    .checked
                    .iter()
                    .map(|(name, actions)| {
                        (
                            display(name),
                            json!({
                                "checked_to_depth": depth,
                                "actions": actions,
                            }),
                        )
                    })
                    .collect(),
            ),
        );
    }
    (Value::Object(output), 0)
}

fn run_refine_chain(
    implementation: &Path,
    first_abstraction: &Path,
    first_mapping: &Path,
    rest: &[PathBuf],
    depth: usize,
) -> (Value, i32) {
    let mut specifications = vec![
        implementation.to_path_buf(),
        first_abstraction.to_path_buf(),
    ];
    let mut mappings = vec![first_mapping.to_path_buf()];
    for pair in rest.chunks_exact(2) {
        specifications.push(pair[0].clone());
        mappings.push(pair[1].clone());
    }
    let mut names = Vec::new();
    let mut composed = std::collections::BTreeMap::<String, String>::new();
    for index in 0..mappings.len() {
        let (link, status) = run_refine(
            &specifications[index],
            &specifications[index + 1],
            &mappings[index],
            depth,
        );
        let impl_name = link
            .get("impl")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let abs_name = link
            .get("abs")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if index == 0 {
            names.push(impl_name.clone());
        }
        names.push(abs_name.clone());
        if status != 0 || link.get("result").and_then(Value::as_str) != Some("refines") {
            let mut failed = link.as_object().cloned().unwrap_or_default();
            failed.insert("chain".to_owned(), json!(names));
            failed.insert(
                "failed_link".to_owned(),
                json!({
                    "from": impl_name,
                    "to": abs_name,
                    "kind": link.get("kind").cloned().unwrap_or(Value::Null),
                }),
            );
            return (Value::Object(failed), status.max(1));
        }
        let action_map = link
            .get("action_map")
            .and_then(Value::as_object)
            .map(|mapping| {
                mapping
                    .iter()
                    .filter_map(|(name, target)| {
                        target
                            .as_str()
                            .map(|target| (name.clone(), target.to_owned()))
                    })
                    .collect::<std::collections::BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        if index == 0 {
            composed = action_map;
        } else {
            for target in composed.values_mut() {
                if target != "stutter" {
                    *target = action_map
                        .get(target)
                        .cloned()
                        .unwrap_or_else(|| "stutter".to_owned());
                }
            }
        }
    }
    let mut output = envelope();
    output.insert("result".to_owned(), json!("refines"));
    output.insert(
        "impl".to_owned(),
        names.first().map_or(Value::Null, |name| json!(name)),
    );
    output.insert(
        "abs".to_owned(),
        names.last().map_or(Value::Null, |name| json!(name)),
    );
    output.insert("checked_to_depth".to_owned(), json!(depth));
    output.insert(
        "action_map".to_owned(),
        Value::Object(
            composed
                .into_iter()
                .map(|(name, target)| (name, json!(target)))
                .collect(),
        ),
    );
    output.insert("chain".to_owned(), json!(names));
    (Value::Object(output), 0)
}

fn select_properties(
    model: &mut KernelModel,
    selected: Option<&str>,
    excluded: &[String],
) -> Result<(), String> {
    let available = model
        .state
        .iter()
        .filter(|(_, ty)| has_bounds(model, ty))
        .map(|(name, _)| format!("_bounds_{}", display(name)))
        .chain(model.invariants.iter().map(|item| display(&item.name)))
        .chain(model.transitions.iter().map(|item| display(&item.name)))
        .chain(model.leadstos.iter().map(|item| display(&item.name)))
        .chain(model.reachables.iter().map(|item| display(&item.name)))
        .collect::<Vec<_>>();
    if let Some(name) = selected {
        if !available.iter().any(|candidate| candidate == name) {
            let mut display_available = available.clone();
            display_available.sort();
            display_available.dedup();
            return Err(format!(
                "no such property: {name} (available: {})",
                display_available.join(", ")
            ));
        }
        model.invariants.retain(|item| display(&item.name) == name);
        model.transitions.retain(|item| display(&item.name) == name);
        model.leadstos.retain(|item| display(&item.name) == name);
        model.reachables.retain(|item| display(&item.name) == name);
    }
    if !excluded.is_empty() {
        let selected_available = model
            .invariants
            .iter()
            .map(|item| display(&item.name))
            .chain(model.transitions.iter().map(|item| display(&item.name)))
            .chain(model.leadstos.iter().map(|item| display(&item.name)))
            .chain(model.reachables.iter().map(|item| display(&item.name)))
            .collect::<Vec<_>>();
        let mut missing = excluded
            .iter()
            .filter(|name| {
                !selected_available
                    .iter()
                    .any(|candidate| candidate == *name)
            })
            .cloned()
            .collect::<Vec<_>>();
        missing.sort();
        missing.dedup();
        if !missing.is_empty() {
            return Err(format!(
                "no such property: {} (available: {})",
                missing.join(", "),
                selected_available.join(", ")
            ));
        }
        model
            .invariants
            .retain(|item| !excluded.contains(&display(&item.name)));
        model
            .transitions
            .retain(|item| !excluded.contains(&display(&item.name)));
        model
            .leadstos
            .retain(|item| !excluded.contains(&display(&item.name)));
        model
            .reachables
            .retain(|item| !excluded.contains(&display(&item.name)));
    }
    Ok(())
}

fn selected_implicit_bounds(
    model: &KernelModel,
    selected: Option<&str>,
    excluded: &[String],
) -> Option<std::collections::BTreeSet<String>> {
    if selected.is_none() && !excluded.iter().any(|name| name.starts_with("_bounds_")) {
        return None;
    }
    let mut bounds = model
        .state
        .iter()
        .filter(|(_, ty)| has_bounds(model, ty))
        .map(|(name, _)| format!("_bounds_{name}"))
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(selected) = selected {
        bounds.retain(|name| display(name) == selected);
    }
    bounds.retain(|name| !excluded.contains(&display(name)));
    Some(bounds)
}

#[allow(clippy::too_many_lines)]
fn run_verify(
    path: &Path,
    depth: usize,
    deadlock_mode: &str,
    engine: &str,
    explicit_budget: usize,
    k_ind: usize,
) -> (Value, i32) {
    if let Ok(source) = std::fs::read_to_string(path) {
        match fsl_syntax::parse_document(fsl_syntax::SourceFile::new(&source)) {
            Err(error) => return (surface_parse_error_output(&error), 2),
            Ok(fsl_syntax::ParsedDocument {
                surface: fsl_syntax::SurfaceDocument::Agent(_),
                ..
            }) => {
                return (
                    error_output(
                        "parse",
                        "agent documents cannot be verified as Kernel specs",
                    ),
                    2,
                );
            }
            Ok(_) => {}
        }
    }
    if let Err(error) = validate_specialized_document(path) {
        return (semantic_error_output(&error), 2);
    }
    let mut has_trace_contract = false;
    let mut implements = None;
    if let Ok(model) = load_model(path) {
        match validate_requirement_traces(path, &model) {
            Ok((Some(failure), _)) => return (failure, 2),
            Ok((None, has_contract)) => has_trace_contract = has_contract,
            Err(error) => return (semantic_error_output(&error), 2),
        }
        implements = match implements_result(path, &model, depth) {
            Ok(implements) => implements,
            Err(error) => return (error_output("type", &error), 2),
        };
    }
    let deadlock = match DeadlockMode::parse(deadlock_mode) {
        Ok(mode) => mode,
        Err(error) => return (error_output("usage", &error), 2),
    };
    let selection = ModelSelection {
        path,
        model: None,
        scope: None,
        property: None,
        excluded: &[],
    };
    let (mut output, status) = match VerificationEngine::parse(engine) {
        Ok(VerificationEngine::Bmc) => run_bmc_filtered(BmcRequest {
            selection,
            depth,
            deadlock,
            initial_state: None,
        }),
        Ok(VerificationEngine::Induction) => run_induction_filtered(InductionRequest {
            selection,
            depth,
            deadlock,
            k: k_ind,
            auxiliary: &[],
        }),
        Ok(VerificationEngine::Explicit) => run_explicit_filtered(ExplicitRequest {
            selection,
            depth,
            deadlock,
            budget: explicit_budget,
        }),
        Ok(VerificationEngine::Auto) => run_auto_filtered(ExplicitRequest {
            selection,
            depth,
            deadlock,
            budget: explicit_budget,
        }),
        Err(error) => return (error_output("usage", &error), 2),
    };
    if let Value::Object(envelope) = &mut output
        && envelope.get("result").and_then(Value::as_str) != Some("error")
        && let Some(implements) = implements
    {
        envelope.insert("implements".to_owned(), implements);
        if let Some(Value::Array(warnings)) = envelope.get_mut("warnings") {
            warnings.retain(|warning| {
                warning.get("message").and_then(Value::as_str)
                    != Some(
                        "spec declares no user invariants (only implicit type bounds are checked)",
                    )
            });
        }
    }
    if has_trace_contract
        && let Value::Object(envelope) = &mut output
        && let Some(Value::Array(warnings)) = envelope.get_mut("warnings")
    {
        warnings.retain(|warning| {
            warning.get("message").and_then(Value::as_str)
                != Some("spec declares no user invariants (only implicit type bounds are checked)")
        });
    }
    (output, status)
}

fn apply_vacuity_mode(output: &mut Value, mode: &str) -> Option<i32> {
    let findings = output
        .get("warnings")
        .and_then(Value::as_array)
        .map(|warnings| {
            warnings
                .iter()
                .filter(|warning| {
                    warning
                        .get("kind")
                        .and_then(Value::as_str)
                        .is_some_and(|kind| kind.starts_with("vacuous_"))
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if mode == "ignore" {
        if let Some(warnings) = output.get_mut("warnings").and_then(Value::as_array_mut) {
            warnings.retain(|warning| {
                !warning
                    .get("kind")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind.starts_with("vacuous_"))
            });
        }
        return None;
    }
    if mode != "error" || findings.is_empty() {
        return None;
    }
    let mut error = envelope();
    error.insert("result".to_owned(), json!("error"));
    if let Some(spec) = output.get("spec") {
        error.insert("spec".to_owned(), spec.clone());
    }
    error.insert(
        "kind".to_owned(),
        findings[0]
            .get("kind")
            .cloned()
            .unwrap_or_else(|| json!("vacuous")),
    );
    error.insert("findings".to_owned(), Value::Array(findings));
    if let Some(checked) = output.get("checked_to_depth") {
        error.insert("checked_to_depth".to_owned(), checked.clone());
    }
    if let Some(cost) = output.get("cost") {
        error.insert("cost".to_owned(), cost.clone());
    }
    error.insert("trace_type".to_owned(), json!("vacuity"));
    *output = Value::Object(error);
    Some(2)
}

fn coverage_hint(depth: usize) -> String {
    format!(
        "these requires clauses are unsatisfiable at every step up to depth {depth}; weaken one of them, add an action that establishes them, or increase --depth"
    )
}

fn invariant_names(model: &KernelModel) -> Vec<String> {
    invariant_names_selected(model, None)
}

fn invariant_names_selected(
    model: &KernelModel,
    checked_bounds: Option<&std::collections::BTreeSet<String>>,
) -> Vec<String> {
    let mut names = model
        .state
        .iter()
        .filter(|(_, ty)| has_bounds(model, ty))
        .map(|(name, _)| format!("_bounds_{name}"))
        .filter(|name| checked_bounds.is_none_or(|selected| selected.contains(name)))
        .map(|name| display(&name))
        .collect::<Vec<_>>();
    names.extend(
        model
            .invariants
            .iter()
            .map(|property| display(&property.name)),
    );
    names
}

fn bindings_json(bindings: &[std::collections::BTreeMap<String, FslValue>]) -> Value {
    Value::Array(
        bindings
            .iter()
            .map(|binding| {
                Value::Object(
                    binding
                        .iter()
                        .map(|(name, value)| (name.clone(), fslc_rust::fsl_value_json(value)))
                        .collect(),
                )
            })
            .collect(),
    )
}

fn violation_bindings_json(
    model: &KernelModel,
    kind: &str,
    name: &str,
    expr: Option<&KernelExpr>,
    state: Option<&std::collections::BTreeMap<String, FslValue>>,
) -> Value {
    let Some(state) = state else {
        return Value::Null;
    };
    if kind == "type_bound" {
        let state_name = name.strip_prefix("_bounds_").unwrap_or(name);
        let Some((_, TypeRef::Map(_, value_ty))) = model
            .state
            .iter()
            .find(|(candidate, _)| candidate == state_name)
        else {
            return Value::Null;
        };
        let Some(FslValue::Map(entries)) = state.get(state_name) else {
            return Value::Null;
        };
        let bad = entries
            .iter()
            .filter(|(_, value)| {
                !fsl_runtime::value_conforms(value, value_ty, model).unwrap_or(false)
            })
            .map(|(key, _)| {
                let mut binding = std::collections::BTreeMap::new();
                binding.insert("key".to_owned(), key.clone());
                binding
            })
            .collect::<Vec<_>>();
        return if bad.is_empty() {
            Value::Null
        } else {
            bindings_json(&bad)
        };
    }
    let Some(expr) = expr else {
        return Value::Null;
    };
    fsl_runtime::violating_bindings(expr, state, model)
        .ok()
        .flatten()
        .map_or(Value::Null, |bindings| bindings_json(&bindings))
}

fn violation_blame_json(
    model: &KernelModel,
    kind: &str,
    name: &str,
    expr: Option<&KernelExpr>,
    violating_bindings: Value,
) -> Value {
    if kind == "type_bound" {
        let state_name = name.strip_prefix("_bounds_").unwrap_or(name);
        return json!({
            "conjuncts": [{
                "index": 0,
                "text": format!("{} stays within its declared type bounds", display(state_name)),
                "holds": false,
            }]
        });
    }
    let Some(expr) = expr else {
        return json!({"conjuncts": []});
    };
    let mut conjunct = json!({
        "index": 0,
        "text": fslc_rust::source_expr_text(model, expr),
        "holds": false,
    });
    if !violating_bindings.is_null()
        && let Value::Object(entry) = &mut conjunct
    {
        entry.insert("violating_bindings".to_owned(), violating_bindings);
    }
    json!({"conjuncts": [conjunct]})
}

fn has_bounds(model: &KernelModel, ty: &TypeRef) -> bool {
    match ty {
        TypeRef::Int | TypeRef::Bool | TypeRef::Relation(_, _) => false,
        TypeRef::Range(_, _) | TypeRef::Set(_) | TypeRef::Seq(_, _) => true,
        TypeRef::Option(inner) => has_bounds(model, inner),
        TypeRef::Map(_, value) => has_bounds(model, value),
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { .. } | TypeDef::Enum { .. }) => true,
            Some(TypeDef::Struct { fields }) => fields.iter().any(|(_, ty)| has_bounds(model, ty)),
            None => false,
        },
    }
}

enum ValidatedReplayStep {
    Action(Option<std::collections::BTreeMap<String, FslValue>>),
    Stutter,
}

struct ValidatedReplayEvent {
    step: ValidatedReplayStep,
    state: Value,
}

fn validate_versioned_replay_events(
    model: &KernelModel,
    events: &[fslc_rust::replay_trace::ReplayEvent],
) -> Result<Vec<ValidatedReplayEvent>, String> {
    events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let state = replay_snapshot_json(
                event
                    .state
                    .as_ref()
                    .expect("versioned replay event requires state"),
                model,
            )?;
            let step = match &event.step {
                fslc_rust::replay_trace::ReplayStep::Stutter => ValidatedReplayStep::Stutter,
                fslc_rust::replay_trace::ReplayStep::Action { name, params } => {
                    let params = model
                        .actions
                        .iter()
                        .find(|action| action.name == *name)
                        .map(|action| parse_versioned_params(model, action, params, index))
                        .transpose()?;
                    ValidatedReplayStep::Action(params)
                }
            };
            Ok(ValidatedReplayEvent { step, state })
        })
        .collect()
}

fn parse_versioned_params(
    model: &KernelModel,
    action: &fsl_core::ActionDef,
    values: &Map<String, Value>,
    event_index: usize,
) -> Result<std::collections::BTreeMap<String, FslValue>, String> {
    if values.len() != action.params.len() {
        return Err(format!(
            "event {event_index} parameter mismatch for action '{}'",
            action.name
        ));
    }
    action
        .params
        .iter()
        .map(|param| {
            let value = values.get(param.name()).ok_or_else(|| {
                format!("event {event_index} missing parameter '{}'", param.name())
            })?;
            let ty = match param {
                ParamDef::Typed { ty, .. } => ty.clone(),
                ParamDef::Range { lo, hi, .. } => TypeRef::Range(*lo, *hi),
            };
            Ok((
                param.name().to_owned(),
                snapshot_value(
                    model,
                    &ty,
                    value,
                    &format!("events[{event_index}].params.{}", param.name()),
                )?,
            ))
        })
        .collect()
}

fn parse_params(
    model: &KernelModel,
    action: &fsl_core::ActionDef,
    values: &Map<String, Value>,
) -> Result<std::collections::BTreeMap<String, FslValue>, String> {
    if values.len() != action.params.len() {
        return Err(format!(
            "parameter mismatch for action '{}'",
            display(&action.name)
        ));
    }
    action
        .params
        .iter()
        .map(|param| {
            let value = values
                .get(param.name())
                .ok_or_else(|| format!("missing parameter '{}'", param.name()))?;
            Ok((
                param.name().to_owned(),
                parse_param_value(model, param, value)?,
            ))
        })
        .collect()
}

fn parse_param_value(
    model: &KernelModel,
    param: &ParamDef,
    value: &Value,
) -> Result<FslValue, String> {
    match param {
        ParamDef::Range { .. } => value
            .as_i64()
            .map(FslValue::Int)
            .ok_or_else(|| format!("parameter '{}' must be an integer", param.name())),
        ParamDef::Typed { ty, .. } => match ty {
            TypeRef::Bool => value
                .as_bool()
                .map(FslValue::Bool)
                .or_else(|| value.as_i64().map(|value| FslValue::Bool(value != 0)))
                .ok_or_else(|| format!("parameter '{}' must be Boolean", param.name())),
            TypeRef::Int | TypeRef::Range(_, _) => value
                .as_i64()
                .map(FslValue::Int)
                .ok_or_else(|| format!("parameter '{}' must be an integer", param.name())),
            TypeRef::Named(type_name) => match model.types.get(type_name) {
                Some(TypeDef::Domain { .. }) => value
                    .as_i64()
                    .map(FslValue::Int)
                    .ok_or_else(|| format!("parameter '{}' must be an integer", param.name())),
                Some(TypeDef::Enum { members, .. }) => {
                    let member = value.as_str().ok_or_else(|| {
                        format!("parameter '{}' must be an enum member", param.name())
                    })?;
                    if !members.iter().any(|candidate| candidate == member) {
                        return Err(format!("unknown enum member '{member}'"));
                    }
                    Ok(FslValue::Enum {
                        type_name: type_name.clone(),
                        member: member.to_owned(),
                    })
                }
                Some(TypeDef::Struct { .. }) | None => {
                    Err(format!("parameter '{}' has unsupported type", param.name()))
                }
            },
            _ => Err(format!("parameter '{}' has non-scalar type", param.name())),
        },
    }
}

fn load_model(path: &Path) -> Result<KernelModel, String> {
    load_kernel_model(path).map(|(_, _, model)| model)
}

fn load_kernel_model(path: &Path) -> Result<(String, KernelSpec, KernelModel), String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let resolver = fsl_core::FsResolver::new(base);
    let kernel =
        fsl_core::parse_kernel_source_with_file(&source, &resolver, path.to_string_lossy())
            .map_err(|error| {
                if error.message == "top-level document has not reached the kernel lowering gate" {
                    "spec has no state block".to_owned()
                } else {
                    error.to_string()
                }
            })?;
    let model = fsl_core::build_model(kernel.clone()).map_err(|error| error.to_string())?;
    Ok((source, kernel, model))
}

#[allow(clippy::too_many_lines)]
fn snapshot_value(
    model: &KernelModel,
    ty: &TypeRef,
    value: &Value,
    path: &str,
) -> Result<FslValue, String> {
    let type_error = || format!("{path} has a value incompatible with {ty:?}");
    match ty {
        TypeRef::Bool => value.as_bool().map(FslValue::Bool).ok_or_else(type_error),
        TypeRef::Int => value.as_i64().map(FslValue::Int).ok_or_else(type_error),
        TypeRef::Range(lo, hi) => {
            let number = value.as_i64().ok_or_else(type_error)?;
            if number < *lo || number > *hi {
                return Err(format!(
                    "{path} value {number} is out of range [{lo}..{hi}]"
                ));
            }
            Ok(FslValue::Int(number))
        }
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { lo, hi, .. }) => {
                snapshot_value(model, &TypeRef::Range(*lo, *hi), value, path)
            }
            Some(TypeDef::Enum { members, .. }) => {
                let member = value.as_str().ok_or_else(type_error)?;
                if !members.iter().any(|candidate| candidate == member) {
                    return Err(format!(
                        "{path} enum member '{member}' is not one of {}",
                        members.join(", ")
                    ));
                }
                Ok(FslValue::Enum {
                    type_name: name.clone(),
                    member: member.to_owned(),
                })
            }
            Some(TypeDef::Struct { fields }) => {
                let object = value.as_object().ok_or_else(type_error)?;
                for key in object.keys() {
                    if !fields.iter().any(|(field, _)| field == key) {
                        return Err(format!("unknown struct field '{path}.{key}'"));
                    }
                }
                let parsed = fields
                    .iter()
                    .map(|(field, field_ty)| {
                        let field_value = object
                            .get(field)
                            .ok_or_else(|| format!("missing struct field '{path}.{field}'"))?;
                        Ok((
                            field.clone(),
                            snapshot_value(
                                model,
                                field_ty,
                                field_value,
                                &format!("{path}.{field}"),
                            )?,
                        ))
                    })
                    .collect::<Result<_, String>>()?;
                Ok(FslValue::Struct {
                    type_name: name.clone(),
                    fields: parsed,
                })
            }
            None => Err(format!("unknown type '{name}'")),
        },
        TypeRef::Option(inner) => {
            if value.is_null() {
                Ok(FslValue::None)
            } else {
                Ok(FslValue::Some(Box::new(snapshot_value(
                    model, inner, value, path,
                )?)))
            }
        }
        TypeRef::Map(key_ty, value_ty) => {
            let object = value.as_object().ok_or_else(type_error)?;
            let keys = model
                .map_key_values(key_ty)
                .map_err(|error| error.to_string())?;
            let mut entries = std::collections::BTreeMap::new();
            for key in keys {
                let rendered = match &key {
                    FslValue::Int(value) => value.to_string(),
                    FslValue::Bool(value) => value.to_string(),
                    FslValue::Enum { member, .. } => member.clone(),
                    _ => return Err(format!("{path} has a non-scalar map key")),
                };
                let entry = object
                    .get(&rendered)
                    .ok_or_else(|| format!("missing map key '{path}[{rendered}]'"))?;
                entries.insert(
                    key,
                    snapshot_value(model, value_ty, entry, &format!("{path}[{rendered}]"))?,
                );
            }
            if object.len() != entries.len() {
                return Err(format!("{path} contains an unknown map key"));
            }
            Ok(FslValue::Map(entries))
        }
        TypeRef::Set(element_ty) => {
            let items = value.as_array().ok_or_else(type_error)?;
            Ok(FslValue::Set(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        snapshot_value(model, element_ty, item, &format!("{path}[{index}]"))
                    })
                    .collect::<Result<_, _>>()?,
            ))
        }
        TypeRef::Seq(element_ty, capacity) => {
            let items = value.as_array().ok_or_else(type_error)?;
            if items.len() > *capacity {
                return Err(format!(
                    "{path} sequence length {} exceeds capacity {capacity}",
                    items.len()
                ));
            }
            Ok(FslValue::Seq(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        snapshot_value(model, element_ty, item, &format!("{path}[{index}]"))
                    })
                    .collect::<Result<_, _>>()?,
            ))
        }
        TypeRef::Relation(source_ty, target_ty) => {
            let items = value.as_array().ok_or_else(type_error)?;
            Ok(FslValue::Relation(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        let pair = item
                            .as_array()
                            .filter(|pair| pair.len() == 2)
                            .ok_or_else(|| format!("{path}[{index}] must be a pair"))?;
                        Ok((
                            snapshot_value(
                                model,
                                source_ty,
                                &pair[0],
                                &format!("{path}[{index}][0]"),
                            )?,
                            snapshot_value(
                                model,
                                target_ty,
                                &pair[1],
                                &format!("{path}[{index}][1]"),
                            )?,
                        ))
                    })
                    .collect::<Result<_, String>>()?,
            ))
        }
    }
}

fn load_state_snapshot(
    path: &Path,
    model: &KernelModel,
) -> Result<std::collections::BTreeMap<String, FslValue>, (String, String)> {
    let source = std::fs::read_to_string(path).map_err(|error| {
        (
            "io".to_owned(),
            if error.kind() == std::io::ErrorKind::NotFound {
                format!("file not found: {}", path.display())
            } else {
                error.to_string()
            },
        )
    })?;
    let value: Value = serde_json::from_str(&source).map_err(|error| {
        (
            "io".to_owned(),
            format!(
                "invalid state JSON at line {}, column {}: {}",
                error.line(),
                error.column(),
                error
            ),
        )
    })?;
    let object = value.as_object().ok_or_else(|| {
        (
            "type".to_owned(),
            "state snapshot must be a JSON object".to_owned(),
        )
    })?;
    load_snapshot_value_object(object, model).map_err(|error| ("type".to_owned(), error))
}

fn load_snapshot_value_object(
    object: &Map<String, Value>,
    model: &KernelModel,
) -> Result<std::collections::BTreeMap<String, FslValue>, String> {
    for key in object.keys() {
        if !model.state.iter().any(|(name, _)| display(name) == *key) {
            return Err(format!("unknown state variable '{key}'"));
        }
    }
    model
        .state
        .iter()
        .map(|(name, ty)| {
            let display_name = display(name);
            let value = object
                .get(&display_name)
                .ok_or_else(|| format!("missing state variable '{display_name}'"))?;
            Ok((
                name.clone(),
                snapshot_value(model, ty, value, &format!("state.{display_name}"))?,
            ))
        })
        .collect()
}

fn load_model_scoped(path: &Path, scope: &ScopeBounds) -> Result<KernelModel, String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let kernel =
        fsl_core::parse_kernel_source_with_bounds(&source, &scope.instances, &scope.values)
            .map_err(|error| {
                if error.message.starts_with("--instances/--values") {
                    error.message
                } else {
                    error.to_string()
                }
            })?;
    fsl_core::build_model(kernel).map_err(|error| error.to_string())
}

fn envelope() -> Map<String, Value> {
    let mut output = Map::new();
    output.insert("fsl".to_owned(), json!("1.0"));
    output
}

fn with_version_metadata((mut output, status): (Value, i32)) -> (Value, i32) {
    output
        .as_object_mut()
        .expect("check/verify envelope")
        .insert(
            "versions".to_owned(),
            fsl_core::version_metadata(
                "fslc-rust",
                env!("CARGO_PKG_VERSION"),
                "native-z3",
                fsl_solver_z3::version(),
            ),
        );
    (output, status)
}

fn error_output(kind: &str, message: &str) -> Value {
    let mut output = envelope();
    output.insert("result".to_owned(), json!("error"));
    output.insert("kind".to_owned(), json!(kind));
    output.insert("message".to_owned(), json!(message));
    Value::Object(output)
}

fn surface_parse_error_output(error: &fsl_syntax::ParseError) -> Value {
    fslc_rust::frontend_output::render_surface_parse_error(envelope(), error)
}

fn format_error_output(error: &fsl_syntax::FormatError) -> Value {
    let span = error.span();
    let mut output = error_output("format", &error.to_string());
    output
        .as_object_mut()
        .expect("format error envelope")
        .extend([
            ("code".to_owned(), json!("FSL-FMT-UNSAFE")),
            ("loc".to_owned(), span.python_loc()),
            ("span".to_owned(), json!(span)),
        ]);
    output
}

fn normalized_exit_status(output: &Value, reported_status: i32) -> i32 {
    if output.get("result").and_then(Value::as_str) == Some("error")
        && output.get("kind").and_then(Value::as_str) == Some("internal")
    {
        3
    } else {
        reported_status
    }
}

fn semantic_error_output(message: &str) -> Value {
    fslc_rust::verification_output::render_semantic_error(envelope(), message)
}

fn finish(output: &mut Map<String, Value>, checked: usize, started: Instant) {
    output.insert("checked_to_depth".to_owned(), json!(checked));
    output.insert("completeness".to_owned(), json!("bounded"));
    output.insert(
        "cost".to_owned(),
        json!({"elapsed_s": started.elapsed().as_secs_f64()}),
    );
}

fn display(name: &str) -> String {
    fslc_rust::display_name(name)
}

fn block_on_native<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native Z3 backend unexpectedly yielded Pending"),
    }
}

#[cfg(test)]
mod exit_status_tests {
    use super::*;

    #[test]
    fn internal_error_envelopes_always_exit_three() {
        assert_eq!(
            normalized_exit_status(&error_output("internal", "fault"), 2),
            3
        );
        assert_eq!(
            normalized_exit_status(&error_output("semantics", "bad spec"), 2),
            2
        );
    }

    #[test]
    fn literate_materialization_paths_are_process_owned() {
        let source = Path::new("spec.md");
        assert_ne!(
            literate_materialization_path(source, "spec", 41),
            literate_materialization_path(source, "spec", 42)
        );
    }

    #[test]
    fn requirement_edges_reference_existing_tsg_nodes() {
        let source = r#"
spec InitTraceability {
  state { ready: Bool }
  @requirement("REQ-INIT", "startup is traceable")
  init { ready = false }
}
"#;
        let kernel = fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new("."))
            .expect("parse spec");
        let model = fsl_core::build_model(kernel).expect("build model");
        let mut tsg = fsl_tools::analyze_model(&model, "tsg", None).expect("build tsg");
        add_requirements_layer_nodes(&mut tsg, &model);

        let nodes = tsg["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|node| node["id"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(nodes.contains("requirement:REQ-INIT"));
        for edge in tsg["edges"].as_array().unwrap() {
            assert!(nodes.contains(edge["from"].as_str().unwrap()));
            assert!(nodes.contains(edge["to"].as_str().unwrap()));
        }
    }
}
