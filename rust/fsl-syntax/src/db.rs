// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::{ParseError, Span, Token, TokenKind, lex};

pub type DbColumnRef = (String, String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DbColumn {
    pub table: String,
    pub name: String,
    pub db_type: String,
    pub present: bool,
    pub backfilled: bool,
    pub not_null: bool,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DbTable {
    pub name: String,
    pub columns: Vec<DbColumn>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DbDatabase {
    pub name: String,
    pub initial_schema: i64,
    pub tables: Vec<DbTable>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DbMigrationOp {
    pub op: String,
    pub column: DbColumnRef,
    pub nullability: Option<String>,
    pub columns: Vec<DbColumnRef>,
    pub annotations: Vec<String>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DbMigration {
    pub name: String,
    pub from_schema: i64,
    pub to_schema: i64,
    pub ops: Vec<DbMigrationOp>,
    pub annotations: Vec<String>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DbArtifact {
    pub name: String,
    pub capabilities: BTreeMap<String, Vec<DbColumnRef>>,
    pub offline_ttls: BTreeMap<DbColumnRef, i64>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DbFlag {
    pub name: String,
    pub variants: Vec<String>,
    pub default: String,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DbFlagCondition {
    pub flag: String,
    pub variant: String,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DbEnvironmentArtifact {
    pub role: String,
    pub artifact: String,
    pub schema_window: Option<(i64, i64)>,
    pub flag_conditions: Vec<DbFlagCondition>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DbEnvironment {
    pub name: String,
    pub schema_window: (i64, i64),
    pub artifacts: Vec<DbEnvironmentArtifact>,
    pub flags: Vec<DbFlag>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DbCheck {
    pub rules: Vec<String>,
    pub span: Option<Span>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DbSystem {
    pub name: String,
    pub database: DbDatabase,
    pub migrations: Vec<DbMigration>,
    pub artifacts: Vec<DbArtifact>,
    pub environments: Vec<DbEnvironment>,
    pub check: DbCheck,
    pub span: Span,
}

impl DbSystem {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        json!({
            "$type": "DbSystem",
            "name": self.name,
            "database": self.database.python_ast(),
            "migrations": self.migrations.iter().map(DbMigration::python_ast).collect::<Vec<_>>(),
            "artifacts": self.artifacts.iter().map(DbArtifact::python_ast).collect::<Vec<_>>(),
            "environments": self.environments.iter().map(DbEnvironment::python_ast).collect::<Vec<_>>(),
            "check": self.check.python_ast(),
            "loc": self.span.python_loc(),
        })
    }
}

impl DbDatabase {
    fn python_ast(&self) -> Value {
        json!({
            "$type": "DbDatabase", "name": self.name, "initial_schema": self.initial_schema,
            "tables": self.tables.iter().map(DbTable::python_ast).collect::<Vec<_>>(),
            "loc": self.span.python_loc(),
        })
    }
}

impl DbTable {
    fn python_ast(&self) -> Value {
        json!({
            "$type": "DbTable", "name": self.name,
            "columns": self.columns.iter().map(DbColumn::python_ast).collect::<Vec<_>>(),
            "loc": self.span.python_loc(),
        })
    }
}

impl DbColumn {
    fn python_ast(&self) -> Value {
        json!({
            "$type": "DbColumn", "table": self.table, "name": self.name,
            "db_type": self.db_type, "present": self.present, "backfilled": self.backfilled,
            "not_null": self.not_null, "loc": self.span.python_loc(),
        })
    }
}

impl DbMigration {
    fn python_ast(&self) -> Value {
        json!({
            "$type": "DbMigration", "name": self.name, "from_schema": self.from_schema,
            "to_schema": self.to_schema,
            "ops": self.ops.iter().map(DbMigrationOp::python_ast).collect::<Vec<_>>(),
            "annotations": self.annotations, "loc": self.span.python_loc(),
        })
    }
}

impl DbMigrationOp {
    fn python_ast(&self) -> Value {
        json!({
            "$type": "DbMigrationOp", "op": self.op, "column": self.column,
            "nullability": self.nullability, "columns": self.columns,
            "annotations": self.annotations, "loc": self.span.python_loc(),
        })
    }
}

impl DbArtifact {
    fn python_ast(&self) -> Value {
        let capability = |name: &str| self.capabilities.get(name).cloned().unwrap_or_default();
        let offline_ttls = self
            .offline_ttls
            .iter()
            .map(|((table, column), ttl)| (format!("('{table}', '{column}')"), json!(ttl)))
            .collect::<serde_json::Map<_, _>>();
        json!({
            "$type": "DbArtifact", "name": self.name,
            "reads": capability("reads"), "writes": capability("writes"),
            "requires": capability("requires"), "provides": capability("provides"),
            "calls": capability("calls"), "accepts": capability("accepts"),
            "expects": capability("expects"), "responds": capability("responds"),
            "emits_offline": capability("emits_offline"), "offline_ttls": offline_ttls,
            "loc": self.span.python_loc(),
        })
    }
}

impl DbFlag {
    fn python_ast(&self) -> Value {
        json!({
            "$type": "DbFlag", "name": self.name, "variants": self.variants,
            "default": self.default, "loc": self.span.python_loc(),
        })
    }
}

impl DbFlagCondition {
    fn python_ast(&self) -> Value {
        json!({
            "$type": "DbFlagCondition", "flag": self.flag, "variant": self.variant,
            "loc": self.span.python_loc(),
        })
    }
}

impl DbEnvironmentArtifact {
    fn python_ast(&self) -> Value {
        json!({
            "$type": "DbEnvironmentArtifact", "role": self.role, "artifact": self.artifact,
            "schema_window": self.schema_window,
            "flag_conditions": self.flag_conditions.iter().map(DbFlagCondition::python_ast).collect::<Vec<_>>(),
            "loc": self.span.python_loc(),
        })
    }
}

impl DbEnvironment {
    fn python_ast(&self) -> Value {
        json!({
            "$type": "DbEnvironment", "name": self.name, "schema_window": self.schema_window,
            "artifacts": self.artifacts.iter().map(DbEnvironmentArtifact::python_ast).collect::<Vec<_>>(),
            "flags": self.flags.iter().map(DbFlag::python_ast).collect::<Vec<_>>(),
            "loc": self.span.python_loc(),
        })
    }
}

impl DbCheck {
    fn python_ast(&self) -> Value {
        json!({
            "$type": "DbCheck", "rules": self.rules,
            "loc": self.span.map(Span::python_loc),
        })
    }
}

enum DbItem {
    Database(DbDatabase),
    Migration(DbMigration),
    Artifact(DbArtifact),
    Environment(DbEnvironment),
    Check(DbCheck),
}

/// Parse one specialized `dbsystem` source into typed frontend IR.
///
/// # Errors
///
/// Returns [`ParseError`] when lexical, syntactic, or structural analysis fails.
pub fn parse_db_system(source: &str) -> Result<DbSystem, ParseError> {
    let tokens = lex(source).map_err(|error| ParseError {
        message: error.message,
        span: error.span,
    })?;
    parse_db_system_tokens(&tokens, 0)
}

pub(crate) fn parse_db_system_tokens(
    tokens: &[Token],
    cursor: usize,
) -> Result<DbSystem, ParseError> {
    let mut parser = DbParser { tokens, cursor };
    let system = parser.system()?;
    if !matches!(parser.peek().kind, TokenKind::Eof) {
        return Err(parser.error("unexpected token after dbsystem"));
    }
    Ok(system)
}

struct DbParser<'a> {
    tokens: &'a [Token],
    cursor: usize,
}

impl DbParser<'_> {
    fn system(&mut self) -> Result<DbSystem, ParseError> {
        let span = self.peek().span;
        self.expect_ident_value("dbsystem")?;
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut database = None;
        let mut migrations = Vec::new();
        let mut artifacts = Vec::new();
        let mut environments = Vec::new();
        let mut check = DbCheck {
            rules: Vec::new(),
            span: None,
        };
        while !self.eat_symbol("}") {
            match self.item()? {
                DbItem::Database(value) => {
                    if database.replace(value).is_some() {
                        return Err(self.error("dbsystem may declare only one database"));
                    }
                }
                DbItem::Migration(value) => migrations.push(value),
                DbItem::Artifact(value) => artifacts.push(value),
                DbItem::Environment(value) => environments.push(value),
                DbItem::Check(value) => check = value,
            }
        }
        Ok(DbSystem {
            name,
            database: database.ok_or_else(|| self.error("dbsystem requires a database block"))?,
            migrations,
            artifacts,
            environments,
            check,
            span,
        })
    }

    fn item(&mut self) -> Result<DbItem, ParseError> {
        if self.peek_ident("database") {
            Ok(DbItem::Database(self.database()?))
        } else if self.peek_ident("migration") {
            Ok(DbItem::Migration(self.migration()?))
        } else if self.peek_ident("artifact") {
            Ok(DbItem::Artifact(self.artifact()?))
        } else if self.peek_ident("environment") {
            Ok(DbItem::Environment(self.environment()?))
        } else if self.peek_ident("check") {
            Ok(DbItem::Check(self.check()?))
        } else {
            Err(self.error("expected dbsystem declaration"))
        }
    }

    fn database(&mut self) -> Result<DbDatabase, ParseError> {
        let span = self.bump().span;
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut schema = None;
        let mut tables = Vec::new();
        while !self.eat_symbol("}") {
            if self.eat_ident("schema") {
                if schema.replace(self.expect_int()?).is_some() {
                    return Err(self.error("database schema may be declared at most once"));
                }
                self.eat_symbol(";");
            } else if self.peek_ident("table") {
                tables.push(self.table()?);
            } else {
                return Err(self.error("expected schema or table"));
            }
        }
        Ok(DbDatabase {
            name,
            initial_schema: schema.ok_or_else(|| self.error("database requires schema"))?,
            tables,
            span,
        })
    }

    fn table(&mut self) -> Result<DbTable, ParseError> {
        let span = self.bump().span;
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut columns = Vec::new();
        while !self.eat_symbol("}") {
            columns.push(self.column(&name)?);
        }
        Ok(DbTable {
            name,
            columns,
            span,
        })
    }

    fn column(&mut self, table: &str) -> Result<DbColumn, ParseError> {
        let span = self.peek().span;
        self.expect_ident_value("column")?;
        let name = self.expect_ident()?;
        let db_type = if self.eat_symbol(":") {
            self.expect_ident()?
        } else {
            "Value".to_owned()
        };
        let mut present = true;
        let mut backfilled = true;
        let mut not_null = false;
        while let TokenKind::Ident(attribute) = &self.peek().kind {
            match attribute.as_str() {
                "present" => present = true,
                "absent" => {
                    present = false;
                    backfilled = false;
                    not_null = false;
                }
                "backfilled" => backfilled = true,
                "not_null" => not_null = true,
                "nullable" => not_null = false,
                _ => break,
            }
            self.bump();
        }
        self.eat_symbol(";");
        Ok(DbColumn {
            table: table.to_owned(),
            name,
            db_type,
            present,
            backfilled,
            not_null,
            span,
        })
    }

    fn migration(&mut self) -> Result<DbMigration, ParseError> {
        let span = self.bump().span;
        let name = self.expect_ident()?;
        self.expect_ident_value("from")?;
        let from_schema = self.expect_int()?;
        self.expect_ident_value("to")?;
        let to_schema = self.expect_int()?;
        let annotations = self.annotations();
        self.expect_symbol("{")?;
        let mut ops = Vec::new();
        while !self.eat_symbol("}") {
            ops.push(self.migration_op()?);
        }
        Ok(DbMigration {
            name,
            from_schema,
            to_schema,
            ops,
            annotations,
            span,
        })
    }

    fn migration_op(&mut self) -> Result<DbMigrationOp, ParseError> {
        let span = self.peek().span;
        let op = self.expect_ident()?;
        let op = if op == "not_null" {
            "set_not_null".to_owned()
        } else {
            op
        };
        let (column, nullability, columns, annotations) = match op.as_str() {
            "add" => {
                let column = self.column_ref()?;
                let nullability = if self.peek_ident("nullable") || self.peek_ident("not_null") {
                    Some(self.expect_ident()?)
                } else {
                    Some("nullable".to_owned())
                };
                (column, nullability, Vec::new(), Vec::new())
            }
            "drop" => (self.column_ref()?, None, Vec::new(), self.annotations()),
            "backfill" | "set_not_null" => (self.column_ref()?, None, Vec::new(), Vec::new()),
            "rename" => {
                let source = self.column_ref()?;
                self.expect_ident_value("to")?;
                let target = self.column_ref()?;
                (source, None, vec![target], self.annotations())
            }
            "split" => {
                let source = self.column_ref()?;
                self.expect_ident_value("into")?;
                (source, None, self.column_ref_list()?, self.annotations())
            }
            "merge" => {
                let sources = self.column_ref_list_until("into")?;
                self.expect_ident_value("into")?;
                (self.column_ref()?, None, sources, self.annotations())
            }
            _ => return Err(self.error("unknown migration operation")),
        };
        self.eat_symbol(";");
        Ok(DbMigrationOp {
            op,
            column,
            nullability,
            columns,
            annotations,
            span,
        })
    }

    fn artifact(&mut self) -> Result<DbArtifact, ParseError> {
        let span = self.bump().span;
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut capabilities: BTreeMap<String, Vec<DbColumnRef>> = BTreeMap::new();
        let mut offline_ttls = BTreeMap::new();
        while !self.eat_symbol("}") {
            let capability = self.expect_ident()?;
            let refs = self.column_ref_list()?;
            let ttl = if self.eat_ident("ttl") {
                Some(self.expect_int()?)
            } else {
                None
            };
            if capability == "emits_offline" {
                if let Some(ttl) = ttl {
                    for reference in &refs {
                        offline_ttls.insert(reference.clone(), ttl);
                    }
                }
            }
            capabilities.entry(capability).or_default().extend(refs);
            self.eat_symbol(";");
        }
        Ok(DbArtifact {
            name,
            capabilities,
            offline_ttls,
            span,
        })
    }

    fn environment(&mut self) -> Result<DbEnvironment, ParseError> {
        let span = self.bump().span;
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut schema_window = None;
        let mut artifacts = Vec::new();
        let mut flags = Vec::new();
        while !self.eat_symbol("}") {
            if self.eat_ident("schema") {
                let window = self.int_range()?;
                if schema_window.replace(window).is_some() {
                    return Err(self.error("environment schema may be declared at most once"));
                }
                self.eat_symbol(";");
            } else if self.peek_ident("flag") {
                flags.push(self.flag()?);
            } else {
                artifacts.push(self.environment_artifact()?);
            }
        }
        Ok(DbEnvironment {
            name,
            schema_window: schema_window
                .ok_or_else(|| self.error("environment requires schema"))?,
            artifacts,
            flags,
            span,
        })
    }

    fn flag(&mut self) -> Result<DbFlag, ParseError> {
        let span = self.bump().span;
        let name = self.expect_ident()?;
        self.expect_symbol("{")?;
        let mut variants = vec![self.expect_ident()?];
        while self.eat_symbol(",") {
            if self.peek_symbol("}") {
                break;
            }
            variants.push(self.expect_ident()?);
        }
        self.expect_symbol("}")?;
        let default = if self.eat_ident("default") {
            self.expect_ident()?
        } else {
            variants[0].clone()
        };
        self.eat_symbol(";");
        Ok(DbFlag {
            name,
            variants,
            default,
            span,
        })
    }

    fn environment_artifact(&mut self) -> Result<DbEnvironmentArtifact, ParseError> {
        let span = self.peek().span;
        let role = self.expect_ident()?;
        if !matches!(role.as_str(), "active" | "supported" | "may_exist") {
            return Err(self.error("expected environment artifact role"));
        }
        let artifact = self.expect_ident()?;
        let mut schema_window = None;
        let mut flag_conditions = Vec::new();
        while self.peek_ident("when") {
            let condition_span = self.bump().span;
            if self.eat_ident("schema") {
                schema_window = Some(self.int_range()?);
            } else {
                self.expect_ident_value("flag")?;
                let flag = self.expect_ident()?;
                self.expect_symbol("=")?;
                flag_conditions.push(DbFlagCondition {
                    flag,
                    variant: self.expect_ident()?,
                    span: condition_span,
                });
            }
        }
        self.eat_symbol(";");
        Ok(DbEnvironmentArtifact {
            role,
            artifact,
            schema_window,
            flag_conditions,
            span,
        })
    }

    fn check(&mut self) -> Result<DbCheck, ParseError> {
        let span = self.bump().span;
        self.expect_ident_value("compatibility")?;
        self.expect_symbol("{")?;
        let mut rules = Vec::new();
        while !self.eat_symbol("}") {
            self.expect_ident_value("rule")?;
            rules.push(self.expect_ident()?);
            self.eat_symbol(";");
        }
        Ok(DbCheck {
            rules,
            span: Some(span),
        })
    }

    fn annotations(&mut self) -> Vec<String> {
        let mut values = Vec::new();
        while let TokenKind::Ident(value) = &self.peek().kind {
            if !matches!(
                value.as_str(),
                "destructive" | "irreversible" | "rollbackable" | "lossy" | "lossless"
            ) {
                break;
            }
            values.push(value.clone());
            self.bump();
        }
        values
    }

    fn column_ref_list(&mut self) -> Result<Vec<DbColumnRef>, ParseError> {
        let mut refs = vec![self.column_ref()?];
        while self.eat_symbol(",") {
            refs.push(self.column_ref()?);
        }
        Ok(refs)
    }

    fn column_ref_list_until(&mut self, keyword: &str) -> Result<Vec<DbColumnRef>, ParseError> {
        let mut refs = vec![self.column_ref()?];
        while self.eat_symbol(",") {
            refs.push(self.column_ref()?);
        }
        if !self.peek_ident(keyword) {
            return Err(self.error(&format!("expected '{keyword}'")));
        }
        Ok(refs)
    }

    fn column_ref(&mut self) -> Result<DbColumnRef, ParseError> {
        let table = self.expect_ident()?;
        self.expect_symbol(".")?;
        Ok((table, self.expect_ident()?))
    }

    fn int_range(&mut self) -> Result<(i64, i64), ParseError> {
        let lo = self.expect_int()?;
        self.expect_symbol("..")?;
        Ok((lo, self.expect_int()?))
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn bump(&mut self) -> &Token {
        let index = self.cursor;
        if !matches!(self.tokens[index].kind, TokenKind::Eof) {
            self.cursor += 1;
        }
        &self.tokens[index]
    }

    fn peek_ident(&self, expected: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Ident(value) if value == expected)
    }

    fn peek_symbol(&self, expected: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Symbol(value) if value == expected)
    }

    fn eat_ident(&mut self, expected: &str) -> bool {
        if self.peek_ident(expected) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_symbol(&mut self, expected: &str) -> bool {
        if self.peek_symbol(expected) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        let token = self.bump().clone();
        match token.kind {
            TokenKind::Ident(value) => Ok(value),
            _ => Err(ParseError {
                message: "expected identifier".to_owned(),
                span: token.span,
            }),
        }
    }

    fn expect_ident_value(&mut self, expected: &str) -> Result<(), ParseError> {
        if self.eat_ident(expected) {
            Ok(())
        } else {
            Err(self.error(&format!("expected '{expected}'")))
        }
    }

    fn expect_int(&mut self) -> Result<i64, ParseError> {
        let token = self.bump().clone();
        match token.kind {
            TokenKind::Int(value) => Ok(value),
            _ => Err(ParseError {
                message: "expected integer".to_owned(),
                span: token.span,
            }),
        }
    }

    fn expect_symbol(&mut self, expected: &str) -> Result<(), ParseError> {
        if self.eat_symbol(expected) {
            Ok(())
        } else {
            Err(self.error(&format!("expected '{expected}'")))
        }
    }

    fn error(&self, message: &str) -> ParseError {
        ParseError {
            message: message.to_owned(),
            span: self.peek().span,
        }
    }
}
