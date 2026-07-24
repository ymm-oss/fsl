// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::collections::{BTreeMap, BTreeSet};

use fsl_syntax::{
    ActionItem, Annotations, DbColumn, DbColumnRef, DbEnvironmentArtifact, DbSystem, Expr, LValue,
    MetaTag, Span, SpecItem, StateField, Statement, SurfaceSpec, TypeExpr,
};

use crate::{
    INIT_TARGET, LoweringStep, OriginChain, OriginId, OriginRegistry, OriginSite, SPEC_TARGET,
    TERMINAL_TARGET, action_guard_target, action_statement_target, action_target,
    init_statement_target, property_target, state_target, type_target,
};

fn safe(name: &str) -> String {
    let mut output = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if output.is_empty() {
        output.push('x');
    }
    if output.starts_with(|character: char| character.is_ascii_digit()) {
        output.insert(0, '_');
    }
    output
}

fn column_member(column: &DbColumnRef) -> String {
    format!("col_{}_{}", safe(&column.0), safe(&column.1))
}

fn column_label(column: &DbColumnRef) -> String {
    format!("{}.{}", column.0, column.1)
}

fn invariant_name(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|part| safe(part))
        .collect::<Vec<_>>()
        // `__` is reserved for composed-name display (`component__name` ->
        // `component.name`). Keep the database dialect's public double
        // underscores through parsing and restore them at presentation time.
        .join("QqDbSepqQ")
}

fn entry_name_parts(entry: &DbEnvironmentArtifact) -> Vec<String> {
    let mut parts = Vec::new();
    if let Some((lo, hi)) = entry.schema_window {
        parts.extend(["schema".to_owned(), lo.to_string(), hi.to_string()]);
    }
    for condition in &entry.flag_conditions {
        parts.extend([
            "flag".to_owned(),
            condition.flag.clone(),
            condition.variant.clone(),
        ]);
    }
    if parts.is_empty() {
        parts.push("all".to_owned());
    }
    parts
}

fn named(name: &str) -> TypeExpr {
    TypeExpr::Name(name.to_owned())
}

fn binary(op: &str, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: op.to_owned(),
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn indexed(name: &str, member: &str) -> Expr {
    Expr::Index(
        Box::new(Expr::Var(name.to_owned())),
        Box::new(Expr::Var(member.to_owned())),
    )
}

fn assignment(name: &str, member: &str, value: Expr, span: Span) -> ActionItem {
    ActionItem::Statement(Statement::Assign {
        target: LValue::Index(name.to_owned(), Expr::Var(member.to_owned())),
        value,
        span,
    })
}

fn requirement(expression: Expr, span: Span) -> ActionItem {
    ActionItem::Requires(expression, span)
}

fn metadata(id: &str, text: String, span: Span) -> MetaTag {
    MetaTag {
        id: id.to_owned(),
        text: Some(text),
        span: Some(span),
    }
}

fn source_site(span: Span, path: Vec<String>) -> OriginSite {
    OriginSite {
        source_file: None,
        span: Some(span),
        dialect: "dbsystem".to_owned(),
        declaration_path: path,
    }
}

fn db_origin(
    span: Span,
    path: Vec<String>,
    step: &str,
    generated: bool,
    secondary: Vec<OriginSite>,
) -> OriginChain {
    OriginChain {
        id: OriginId(format!(
            "db:{}:{}:{}:{}:{}",
            path.join("/"),
            span.start.offset,
            span.end.offset,
            span.start.line,
            span.start.column
        )),
        dialect: "dbsystem".to_owned(),
        primary: Some(source_site(span, path)),
        secondary,
        lowering_steps: vec![LoweringStep {
            kind: step.to_owned(),
            detail: None,
        }],
        generated,
    }
}

fn column_path(system: &DbSystem, column: &DbColumn) -> Vec<String> {
    vec![
        system.name.clone(),
        "database".to_owned(),
        system.database.name.clone(),
        "table".to_owned(),
        column.table.clone(),
        "column".to_owned(),
        column.name.clone(),
    ]
}

fn bind_action_origins(
    registry: &mut OriginRegistry,
    name: &str,
    items: &[ActionItem],
    action_origin: OriginChain,
    item_origins: &[OriginChain],
) {
    registry.bind(action_target(name), action_origin);
    let mut guard_index = 0;
    let mut statement_index = 0;
    for (item, origin) in items.iter().zip(item_origins) {
        match item {
            ActionItem::Requires(..) | ActionItem::Let(..) => {
                registry.bind(action_guard_target(name, guard_index), origin.clone());
                guard_index += 1;
            }
            ActionItem::Ensures(..) => {
                registry.bind(
                    format!("action:{name}:ensure:{guard_index}"),
                    origin.clone(),
                );
                guard_index += 1;
            }
            ActionItem::Statement(..) => {
                registry.bind(
                    action_statement_target(name, statement_index),
                    origin.clone(),
                );
                statement_index += 1;
            }
        }
    }
}

fn compatibility_expr(window: (i64, i64), member: &str) -> Expr {
    binary(
        "or",
        Expr::Not(Box::new(binary(
            "and",
            binary(
                ">=",
                Expr::Var("schema_version".to_owned()),
                Expr::Num(window.0),
            ),
            binary(
                "<=",
                Expr::Var("schema_version".to_owned()),
                Expr::Num(window.1),
            ),
        ))),
        indexed("column_exists", member),
    )
}

/// Build the executable database catalog directly as typed surface IR plus
/// source-backed origins. No generated FSL source is involved.
#[allow(clippy::too_many_lines)]
pub(crate) fn lower_db_surface(system: &DbSystem) -> (SurfaceSpec, OriginRegistry) {
    let columns = system
        .database
        .tables
        .iter()
        .flat_map(|table| table.columns.iter())
        .map(|column| ((column.table.clone(), column.name.clone()), column))
        .collect::<BTreeMap<_, _>>();
    let artifacts = system
        .artifacts
        .iter()
        .map(|artifact| (artifact.name.as_str(), artifact))
        .collect::<BTreeMap<_, _>>();
    let mut schema_values = vec![system.database.initial_schema];
    for migration in &system.migrations {
        schema_values.extend([migration.from_schema, migration.to_schema]);
    }
    for environment in &system.environments {
        schema_values.extend([environment.schema_window.0, environment.schema_window.1]);
        for entry in &environment.artifacts {
            if let Some((lo, hi)) = entry.schema_window {
                schema_values.extend([lo, hi]);
            }
        }
    }
    let schema_lo = schema_values.iter().min().copied().unwrap_or(0);
    let schema_hi = schema_values.iter().max().copied().unwrap_or(0);
    let rules = if system.check.rules.is_empty() {
        [
            "all_active_reads_exist",
            "all_active_writes_exist",
            "removed_only_after_unused",
            "not_null_after_backfill",
        ]
        .into_iter()
        .collect::<BTreeSet<_>>()
    } else {
        system
            .check
            .rules
            .iter()
            .map(|rule| rule.name.as_str())
            .collect()
    };
    let rule_by_name = system
        .check
        .rules
        .iter()
        .map(|rule| (rule.name.as_str(), rule))
        .collect::<BTreeMap<_, _>>();

    let mut registry = OriginRegistry::default();
    registry.bind(
        SPEC_TARGET,
        db_origin(
            system.span,
            vec![system.name.clone()],
            "lower_db_system",
            false,
            Vec::new(),
        ),
    );
    let database_path = vec![
        system.name.clone(),
        "database".to_owned(),
        system.database.name.clone(),
    ];
    let database_origin = db_origin(
        system.database.span,
        database_path.clone(),
        "lower_db_database",
        true,
        Vec::new(),
    );
    registry.bind(type_target("SchemaVersion"), database_origin.clone());
    registry.bind(type_target("Column"), database_origin.clone());
    registry.bind(state_target("schema_version"), database_origin.clone());
    for name in ["column_exists", "column_backfilled", "column_not_null"] {
        let target = state_target(name);
        registry.bind(target, database_origin.clone());
    }

    let mut items = vec![
        SpecItem::Type {
            name: "SchemaVersion".to_owned(),
            lo: Box::new(Expr::Num(schema_lo)),
            hi: Box::new(Expr::Num(schema_hi)),
            symmetric: false,
        },
        SpecItem::Enum {
            name: "Column".to_owned(),
            members: columns.keys().map(column_member).collect(),
            symmetric: false,
        },
        SpecItem::State(vec![
            StateField::generated(
                "schema_version",
                named("SchemaVersion"),
                system.database.span,
            ),
            StateField::generated(
                "column_exists",
                TypeExpr::Map(Box::new(named("Column")), Box::new(TypeExpr::Bool)),
                system.database.span,
            ),
            StateField::generated(
                "column_backfilled",
                TypeExpr::Map(Box::new(named("Column")), Box::new(TypeExpr::Bool)),
                system.database.span,
            ),
            StateField::generated(
                "column_not_null",
                TypeExpr::Map(Box::new(named("Column")), Box::new(TypeExpr::Bool)),
                system.database.span,
            ),
        ]),
    ];

    let mut init_statements = vec![Statement::Assign {
        target: LValue::Var("schema_version".to_owned()),
        value: Expr::Num(system.database.initial_schema),
        span: system.database.span,
    }];
    let mut init_origins = vec![database_origin.clone()];
    for (key, column) in &columns {
        let member = column_member(key);
        let origin = db_origin(
            column.span,
            column_path(system, column),
            "lower_db_column_initial_state",
            true,
            Vec::new(),
        );
        for (name, value) in [
            ("column_exists", column.present),
            ("column_backfilled", column.backfilled),
            ("column_not_null", column.not_null),
        ] {
            init_statements.push(Statement::Assign {
                target: LValue::Index(name.to_owned(), Expr::Var(member.clone())),
                value: Expr::Bool(value),
                span: column.span,
            });
            init_origins.push(origin.clone());
        }
    }
    registry.bind(INIT_TARGET, database_origin);
    for (index, origin) in init_origins.into_iter().enumerate() {
        registry.bind(init_statement_target(index), origin);
    }
    items.push(SpecItem::Init {
        statements: init_statements,
        meta: None,
        annotations: Annotations::default(),
    });

    for migration in &system.migrations {
        let action_name = format!("migrate_{}", safe(&migration.name));
        let migration_path = vec![
            system.name.clone(),
            "migration".to_owned(),
            migration.name.clone(),
        ];
        let action_origin = db_origin(
            migration.span,
            migration_path.clone(),
            "lower_db_migration",
            true,
            Vec::new(),
        );
        let mut action_items = vec![requirement(
            binary(
                "==",
                Expr::Var("schema_version".to_owned()),
                Expr::Num(migration.from_schema),
            ),
            migration.span,
        )];
        let mut item_origins = vec![action_origin.clone()];
        for (op_index, operation) in migration.ops.iter().enumerate() {
            let member = column_member(&operation.column);
            let op_origin = db_origin(
                operation.span,
                vec![
                    system.name.clone(),
                    "migration".to_owned(),
                    migration.name.clone(),
                    "operation".to_owned(),
                    op_index.to_string(),
                    operation.op.clone(),
                ],
                "lower_db_migration_operation",
                true,
                Vec::new(),
            );
            let mut push = |item: ActionItem| {
                action_items.push(item);
                item_origins.push(op_origin.clone());
            };
            match operation.op.as_str() {
                "add" => {
                    push(assignment(
                        "column_exists",
                        &member,
                        Expr::Bool(true),
                        operation.span,
                    ));
                    push(assignment(
                        "column_backfilled",
                        &member,
                        Expr::Bool(false),
                        operation.span,
                    ));
                    push(assignment(
                        "column_not_null",
                        &member,
                        Expr::Bool(operation.nullability.as_deref() == Some("not_null")),
                        operation.span,
                    ));
                }
                "backfill" => {
                    push(requirement(
                        indexed("column_exists", &member),
                        operation.span,
                    ));
                    push(assignment(
                        "column_backfilled",
                        &member,
                        Expr::Bool(true),
                        operation.span,
                    ));
                }
                "set_not_null" => {
                    push(requirement(
                        indexed("column_exists", &member),
                        operation.span,
                    ));
                    push(requirement(
                        indexed("column_backfilled", &member),
                        operation.span,
                    ));
                    push(assignment(
                        "column_not_null",
                        &member,
                        Expr::Bool(true),
                        operation.span,
                    ));
                }
                "drop" => {
                    for name in ["column_exists", "column_backfilled", "column_not_null"] {
                        push(assignment(name, &member, Expr::Bool(false), operation.span));
                    }
                }
                "rename" => {
                    if let Some(target_column) = operation.columns.first() {
                        let target = column_member(target_column);
                        push(requirement(
                            indexed("column_exists", &member),
                            operation.span,
                        ));
                        for name in ["column_exists", "column_backfilled", "column_not_null"] {
                            push(assignment(name, &member, Expr::Bool(false), operation.span));
                        }
                        push(assignment(
                            "column_exists",
                            &target,
                            Expr::Bool(true),
                            operation.span,
                        ));
                        push(assignment(
                            "column_backfilled",
                            &target,
                            Expr::Bool(true),
                            operation.span,
                        ));
                        push(assignment(
                            "column_not_null",
                            &target,
                            indexed("column_not_null", &member),
                            operation.span,
                        ));
                    }
                }
                "split" => {
                    push(requirement(
                        indexed("column_exists", &member),
                        operation.span,
                    ));
                    for name in ["column_exists", "column_backfilled", "column_not_null"] {
                        push(assignment(name, &member, Expr::Bool(false), operation.span));
                    }
                    for target_column in &operation.columns {
                        let target = column_member(target_column);
                        push(assignment(
                            "column_exists",
                            &target,
                            Expr::Bool(true),
                            operation.span,
                        ));
                        push(assignment(
                            "column_backfilled",
                            &target,
                            Expr::Bool(true),
                            operation.span,
                        ));
                        push(assignment(
                            "column_not_null",
                            &target,
                            Expr::Bool(false),
                            operation.span,
                        ));
                    }
                }
                "merge" => {
                    for source_column in &operation.columns {
                        let source = column_member(source_column);
                        push(requirement(
                            indexed("column_exists", &source),
                            operation.span,
                        ));
                        for name in ["column_exists", "column_backfilled", "column_not_null"] {
                            push(assignment(name, &source, Expr::Bool(false), operation.span));
                        }
                    }
                    push(assignment(
                        "column_exists",
                        &member,
                        Expr::Bool(true),
                        operation.span,
                    ));
                    push(assignment(
                        "column_backfilled",
                        &member,
                        Expr::Bool(true),
                        operation.span,
                    ));
                    push(assignment(
                        "column_not_null",
                        &member,
                        Expr::Bool(false),
                        operation.span,
                    ));
                }
                _ => {}
            }
        }
        action_items.push(ActionItem::Statement(Statement::Assign {
            target: LValue::Var("schema_version".to_owned()),
            value: Expr::Num(migration.to_schema),
            span: migration.span,
        }));
        item_origins.push(action_origin.clone());
        bind_action_origins(
            &mut registry,
            &action_name,
            &action_items,
            action_origin,
            &item_origins,
        );
        items.push(SpecItem::Action {
            name: action_name,
            params: Vec::new(),
            items: action_items,
            span: migration.span,
            fair: false,
            meta: Some(metadata(
                "DB-MIGRATION",
                format!(
                    "{}: schema {} -> {}",
                    migration.name, migration.from_schema, migration.to_schema
                ),
                migration.span,
            )),
            sync: false,
            annotations: migration.decl_annotations.clone(),
        });
    }
    if system.migrations.is_empty() {
        let action_name = format!("observe_schema_{}", system.database.initial_schema);
        let origin = db_origin(
            system.database.span,
            database_path,
            "lower_db_static_snapshot",
            true,
            Vec::new(),
        );
        let action_items = vec![
            requirement(
                binary(
                    "==",
                    Expr::Var("schema_version".to_owned()),
                    Expr::Num(system.database.initial_schema),
                ),
                system.database.span,
            ),
            ActionItem::Statement(Statement::Assign {
                target: LValue::Var("schema_version".to_owned()),
                value: Expr::Num(system.database.initial_schema),
                span: system.database.span,
            }),
        ];
        bind_action_origins(
            &mut registry,
            &action_name,
            &action_items,
            origin.clone(),
            &[origin.clone(), origin],
        );
        items.push(SpecItem::Action {
            name: action_name,
            params: Vec::new(),
            items: action_items,
            span: system.database.span,
            fair: false,
            meta: Some(metadata(
                "DB-SNAPSHOT",
                "static compatibility snapshot".to_owned(),
                system.database.span,
            )),
            sync: false,
            annotations: Annotations::default(),
        });
    }

    if rules.contains("not_null_after_backfill") {
        let rule = rule_by_name.get("not_null_after_backfill").copied();
        let annotations = rule
            .map(|rule| rule.annotations.clone())
            .unwrap_or_default();
        for (key, column) in &columns {
            let member = column_member(key);
            let name = invariant_name(&["db_not_null_after_backfill", &key.0, &key.1]);
            let span = rule.map_or(column.span, |rule| rule.span);
            let mut secondary = Vec::new();
            if rule.is_some() {
                secondary.push(source_site(column.span, column_path(system, column)));
            }
            registry.bind(
                property_target("invariant", &name),
                db_origin(
                    span,
                    vec![
                        system.name.clone(),
                        "check".to_owned(),
                        "not_null_after_backfill".to_owned(),
                        key.0.clone(),
                        key.1.clone(),
                    ],
                    "lower_db_not_null_rule",
                    true,
                    secondary,
                ),
            );
            items.push(SpecItem::Invariant {
                name,
                expr: Box::new(binary(
                    "or",
                    Expr::Not(Box::new(indexed("column_not_null", &member))),
                    binary(
                        "and",
                        indexed("column_exists", &member),
                        indexed("column_backfilled", &member),
                    ),
                )),
                span,
                meta: Some(metadata(
                    "DB-NOT-NULL",
                    format!(
                        "{} can be not_null only after it exists and is backfilled",
                        column_label(key)
                    ),
                    span,
                )),
                annotations: annotations.clone(),
            });
        }
    }

    for environment in &system.environments {
        for entry in &environment.artifacts {
            let Some(artifact) = artifacts.get(entry.artifact.as_str()) else {
                continue;
            };
            let window = entry.schema_window.unwrap_or(environment.schema_window);
            let name_parts = entry_name_parts(entry);
            for (capability, rule, prefix, id, verb) in [
                (
                    "reads",
                    "all_active_reads_exist",
                    "db_read",
                    "DB-COMPAT-READ",
                    "read",
                ),
                (
                    "writes",
                    "all_active_writes_exist",
                    "db_write",
                    "DB-COMPAT-WRITE",
                    "write",
                ),
            ] {
                if !rules.contains(rule) && !rules.contains("removed_only_after_unused") {
                    continue;
                }
                let mut annotations = rule_by_name
                    .get(rule)
                    .map(|rule| rule.annotations.clone())
                    .unwrap_or_default();
                if let Some(extra) = rule_by_name.get("removed_only_after_unused") {
                    annotations.extend(extra.annotations.source_order().iter().cloned());
                }
                for column in artifact.capabilities.get(capability).into_iter().flatten() {
                    let mut parts = vec![
                        prefix.to_owned(),
                        environment.name.clone(),
                        entry.role.clone(),
                        entry.artifact.clone(),
                    ];
                    parts.extend(name_parts.clone());
                    parts.extend([column.0.clone(), column.1.clone()]);
                    let refs = parts.iter().map(String::as_str).collect::<Vec<_>>();
                    let name = invariant_name(&refs);
                    let mut secondary = vec![source_site(
                        artifact.span,
                        vec![
                            system.name.clone(),
                            "artifact".to_owned(),
                            artifact.name.clone(),
                            capability.to_owned(),
                        ],
                    )];
                    if let Some(column_decl) = columns.get(column) {
                        secondary.push(source_site(
                            column_decl.span,
                            column_path(system, column_decl),
                        ));
                    }
                    for rule_name in [rule, "removed_only_after_unused"] {
                        if let Some(rule_decl) = rule_by_name.get(rule_name) {
                            let site = source_site(
                                rule_decl.span,
                                vec![
                                    system.name.clone(),
                                    "check".to_owned(),
                                    rule_name.to_owned(),
                                ],
                            );
                            if !secondary.contains(&site) {
                                secondary.push(site);
                            }
                        }
                    }
                    registry.bind(
                        property_target("invariant", &name),
                        db_origin(
                            entry.span,
                            vec![
                                system.name.clone(),
                                "environment".to_owned(),
                                environment.name.clone(),
                                entry.role.clone(),
                                entry.artifact.clone(),
                            ],
                            "lower_db_artifact_compatibility",
                            true,
                            secondary,
                        ),
                    );
                    items.push(SpecItem::Invariant {
                        name,
                        expr: Box::new(compatibility_expr(window, &column_member(column))),
                        span: entry.span,
                        meta: Some(metadata(
                            id,
                            format!(
                                "{} artifact {} in {} must not {} missing column {}",
                                entry.role,
                                entry.artifact,
                                environment.name,
                                verb,
                                column_label(column)
                            ),
                            entry.span,
                        )),
                        annotations: annotations.clone(),
                    });
                }
            }
        }
    }

    let final_schema = system
        .migrations
        .last()
        .map_or(system.database.initial_schema, |migration| {
            migration.to_schema
        });
    let terminal_span = system
        .migrations
        .last()
        .map_or(system.database.span, |migration| migration.span);
    registry.bind(
        TERMINAL_TARGET,
        db_origin(
            terminal_span,
            vec![system.name.clone(), "terminal_schema".to_owned()],
            "lower_db_terminal_schema",
            true,
            Vec::new(),
        ),
    );
    items.push(SpecItem::Terminal {
        expr: Box::new(binary(
            "==",
            Expr::Var("schema_version".to_owned()),
            Expr::Num(final_schema),
        )),
        span: terminal_span,
    });

    (
        SurfaceSpec {
            name: system.name.clone(),
            meta: Some(metadata(
                "db",
                "database multi-environment compatibility".to_owned(),
                system.span,
            )),
            items,
        },
        registry,
    )
}
