// SPDX-License-Identifier: Apache-2.0

//! A deliberately bounded source interpreter for verify's warning producers.
//! Unlike a grep/list of kinds it resolves actual producer expressions. Unknown
//! expressions fail; comments/test items are removed by the existing extractor.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use proc_macro2::{TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, Lit, Member};

#[path = "product_source.rs"]
mod product_source;

pub type Kinds = BTreeSet<String>;
type Constants = BTreeMap<String, Expr>;

fn ast(source: &str) -> syn::File {
    syn::parse_file(&product_source::product_source(source)).unwrap()
}

fn key(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(path) => Some(path.path.segments.last()?.ident.to_string()),
        Expr::Field(field) => match &field.member {
            Member::Named(name) => Some(format!("{}.{}", key(&field.base)?, name)),
            Member::Unnamed(_) => None,
        },
        Expr::MethodCall(call) if call.args.is_empty() => {
            Some(format!("{}.{}()", key(&call.receiver)?, call.method))
        }
        _ => None,
    }
}

fn values(
    expr: &Expr,
    constants: &Constants,
    bindings: &BTreeMap<String, Kinds>,
) -> Result<Kinds, String> {
    if let Some(name) = key(expr) {
        if let Some(value) = bindings.get(&name) {
            return Ok(value.clone());
        }
        if let Some(value) = constants.get(&name) {
            return values(value, constants, bindings);
        }
        return Err(format!("unresolved warning expression {name}"));
    }
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Str(value) => Ok(BTreeSet::from([value.value()])),
            _ => Err("non-string warning expression".to_owned()),
        },
        Expr::Reference(value) => values(&value.expr, constants, bindings),
        Expr::Paren(value) => values(&value.expr, constants, bindings),
        Expr::Array(array) => array.elems.iter().try_fold(Kinds::new(), |mut out, value| {
            out.extend(values(value, constants, bindings)?);
            Ok(out)
        }),
        Expr::Match(value) => value.arms.iter().try_fold(Kinds::new(), |mut out, arm| {
            out.extend(values(&arm.body, constants, bindings)?);
            Ok(out)
        }),
        Expr::Macro(value) if value.mac.path.is_ident("format") => {
            use syn::parse::Parser;
            let args = syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated
                .parse2(value.mac.tokens.clone())
                .map_err(|error| error.to_string())?;
            if args.len() != 2
                || values(&args[0], constants, bindings)? != Kinds::from(["{}_skipped".to_owned()])
                || key(&args[1]).as_deref() != Some("self.kind")
            {
                return Err("unrecognized computed warning format".to_owned());
            }
            Ok(values(&args[1], constants, bindings)?
                .into_iter()
                .map(|kind| format!("{kind}_skipped"))
                .collect())
        }
        _ => Err("unrecognized computed warning expression".to_owned()),
    }
}

#[derive(Default)]
struct Items {
    source: String,
    owner: Option<String>,
    constants: Constants,
    functions: BTreeMap<String, Vec<syn::Block>>,
}
impl<'ast> Visit<'ast> for Items {
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let owner = match item.self_ty.as_ref() {
            syn::Type::Path(path) => path.path.segments.last().map(|part| part.ident.to_string()),
            _ => None,
        };
        let previous = std::mem::replace(&mut self.owner, owner);
        visit::visit_item_impl(self, item);
        self.owner = previous;
    }
    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        self.constants.insert(
            item.ident.to_string(),
            syn::parse_str(&self.source[item.expr.span().byte_range()]).unwrap(),
        );
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.functions
            .entry(item.sig.ident.to_string())
            .or_default()
            .push(syn::parse_str(&self.source[item.block.span().byte_range()]).unwrap());
        visit::visit_item_fn(self, item);
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if let Some(owner) = &self.owner {
            self.functions
                .entry(format!("{owner}::{}", item.sig.ident))
                .or_default()
                .push(syn::parse_str(&self.source[item.block.span().byte_range()]).unwrap());
        }
        self.functions
            .entry(item.sig.ident.to_string())
            .or_default()
            .push(syn::parse_str(&self.source[item.block.span().byte_range()]).unwrap());
        visit::visit_impl_item_fn(self, item);
    }
}

fn items(source: &str) -> Items {
    let clean = product_source::product_source(source);
    let tree = syn::parse_file(&clean).unwrap();
    let mut out = Items {
        source: clean,
        ..Items::default()
    };
    out.visit_file(&tree);
    out
}

fn function(source: &str, name: &str) -> Result<syn::Block, String> {
    let bodies = items(source)
        .functions
        .remove(name)
        .ok_or_else(|| format!("missing producer {name}"))?;
    if bodies.len() != 1 {
        return Err(format!("ambiguous producer {name}"));
    }
    Ok(bodies.into_iter().next().unwrap())
}

/// Parse JSON's *top-level* token fields, not text or nested diagnostic strings.
fn json_fields(tokens: TokenStream) -> Result<BTreeMap<String, Expr>, String> {
    let mut outer = tokens.into_iter();
    let Some(TokenTree::Group(group)) = outer.next() else {
        return Ok(BTreeMap::new());
    };
    if group.delimiter() != proc_macro2::Delimiter::Brace {
        return Ok(BTreeMap::new());
    }
    let mut parts = Vec::new();
    let mut current = TokenStream::new();
    for token in group.stream() {
        if matches!(&token, TokenTree::Punct(p) if p.as_char() == ',') {
            parts.push(current);
            current = TokenStream::new();
        } else {
            current.extend([token]);
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    let mut fields = BTreeMap::new();
    for part in parts {
        let mut tokens = part.into_iter();
        let Some(TokenTree::Literal(literal)) = tokens.next() else {
            return Err("unrecognized JSON warning key".to_owned());
        };
        let name: syn::LitStr =
            syn::parse_str(&literal.to_string()).map_err(|error| error.to_string())?;
        if !matches!(tokens.next(), Some(TokenTree::Punct(p)) if p.as_char() == ':') {
            return Err("missing JSON colon".to_owned());
        }
        // Only classification expressions matter; JSON nested objects are not Rust Exprs.
        if name.value() == "kind" {
            fields.insert(
                name.value(),
                syn::parse2(tokens.collect()).map_err(|error| format!("computed kind: {error}"))?,
            );
        } else {
            fields.insert(name.value(), syn::parse_quote!(()));
        }
    }
    Ok(fields)
}

#[derive(Default, Debug)]
pub struct Inventory {
    pub named: Kinds,
    pub kindless: usize,
    pub suggestions: usize,
}

struct JsonWarnings<'a> {
    constants: &'a Constants,
    bindings: &'a BTreeMap<String, Kinds>,
    inventory: Inventory,
    error: Option<String>,
}
impl<'ast> Visit<'ast> for JsonWarnings<'_> {
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if !mac.path.is_ident("json")
            && mac
                .path
                .segments
                .iter()
                .map(|part| part.ident.to_string())
                .collect::<Vec<_>>()
                != ["serde_json", "json"]
        {
            return;
        }
        let result = (|| {
            let fields = json_fields(mac.tokens.clone())?;
            // The implicit-initial-value producer also emits a nested edit,
            // not a warning. Recognize its complete shape, never every kind
            // without a message (a valid warning may carry only a kind).
            if fields.keys().map(String::as_str).collect::<BTreeSet<_>>()
                == BTreeSet::from(["kind", "replacement", "span", "machine_applicable"])
                && matches!(
                    values(&fields["kind"], self.constants, self.bindings)?
                        .iter()
                        .next()
                        .map(String::as_str),
                    Some("insert" | "replace")
                )
            {
                self.inventory.suggestions += 1;
                return Ok(());
            }
            if let Some(kind) = fields.get("kind") {
                self.inventory
                    .named
                    .extend(values(kind, self.constants, self.bindings)?);
            } else if fields.contains_key("message") || fields.contains_key("hint") {
                self.inventory.kindless += 1;
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            self.error = Some(error);
        }
    }
}

pub fn json_kinds(source: &str) -> Result<Inventory, String> {
    let declarations = items(source);
    scan_json(&ast(source), &declarations.constants, &BTreeMap::new())
}

fn scan_json(
    node: &syn::File,
    constants: &Constants,
    bindings: &BTreeMap<String, Kinds>,
) -> Result<Inventory, String> {
    let mut scan = JsonWarnings {
        constants,
        bindings,
        inventory: Inventory::default(),
        error: None,
    };
    scan.visit_file(node);
    scan.error.map_or(Ok(scan.inventory), Err)
}

fn scan_block(
    block: &syn::Block,
    constants: &Constants,
    bindings: &BTreeMap<String, Kinds>,
) -> Result<Inventory, String> {
    let file: syn::File = syn::parse_quote!(fn producer() #block);
    scan_json(&file, constants, bindings)
}

struct StructCodes<'a> {
    target: &'a str,
    constants: &'a Constants,
    bindings: &'a BTreeMap<String, Kinds>,
    result: Result<Kinds, String>,
}
impl<'ast> Visit<'ast> for StructCodes<'_> {
    fn visit_expr_struct(&mut self, node: &'ast syn::ExprStruct) {
        if node
            .path
            .segments
            .last()
            .is_some_and(|part| part.ident == self.target)
        {
            let code = node
                .fields
                .iter()
                .find(|field| matches!(&field.member, Member::Named(name) if name == "code"));
            let found = code
                .ok_or_else(|| format!("{} missing code", self.target))
                .and_then(|field| values(&field.expr, self.constants, self.bindings));
            match (&mut self.result, found) {
                (Ok(all), Ok(found)) => all.extend(found),
                (_, Err(error)) => self.result = Err(error),
                _ => (),
            }
        }
        visit::visit_expr_struct(self, node);
    }
}

fn struct_codes(
    source: &str,
    target: &str,
    constants: &Constants,
    bindings: &BTreeMap<String, Kinds>,
) -> Result<Kinds, String> {
    let mut visitor = StructCodes {
        target,
        constants,
        bindings,
        result: Ok(Kinds::new()),
    };
    visitor.visit_file(&ast(source));
    visitor.result
}

struct Tuples {
    kinds: Kinds,
}
impl<'ast> Visit<'ast> for Tuples {
    fn visit_expr_tuple(&mut self, node: &'ast syn::ExprTuple) {
        if let Some(Expr::Lit(value)) = node.elems.first()
            && let Lit::Str(value) = &value.lit
        {
            self.kinds.insert(value.value());
        }
        visit::visit_expr_tuple(self, node);
    }
}

pub fn skipped_inputs(source: &str) -> Result<Kinds, String> {
    struct Inputs {
        kinds: Kinds,
        error: Option<String>,
    }
    impl<'ast> Visit<'ast> for Inputs {
        fn visit_expr_tuple(&mut self, node: &'ast syn::ExprTuple) {
            if let Some(first) = node.elems.first() {
                // The final projection forwards the previously derived `kind`.
                if key(first).as_deref() != Some("kind") {
                    match values(first, &Constants::new(), &BTreeMap::new()) {
                        Ok(found) => self.kinds.extend(found),
                        Err(error) => self.error = Some(error),
                    }
                }
            }
            visit::visit_expr_tuple(self, node);
        }
    }
    let mut inputs = Inputs {
        kinds: Kinds::new(),
        error: None,
    };
    inputs.visit_block(&function(source, "split_cases")?);
    if let Some(error) = inputs.error {
        return Err(error);
    }
    if inputs.kinds.is_empty() {
        return Err("split_cases no longer exposes literal input kinds".to_owned());
    }
    Ok(inputs.kinds)
}

pub fn skipped_warning_kinds(source: &str, inputs: Kinds) -> Result<Kinds, String> {
    Ok(scan_block(
        &function(source, "warning")?,
        &Constants::new(),
        &BTreeMap::from([("self.kind".to_owned(), inputs)]),
    )?
    .named)
}

pub fn solver_kinds(source: &str) -> Result<Kinds, String> {
    let block = function(source, "kind")?;
    let Some(syn::Stmt::Expr(expr, _)) = block.stmts.last() else {
        return Err("missing solver kind expression".to_owned());
    };
    values(expr, &items(source).constants, &BTreeMap::new())
}

struct Calls<'a> {
    name: &'a str,
    kinds: Result<Kinds, String>,
}
impl<'ast> Visit<'ast> for Calls<'_> {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if key(&call.func).as_deref() == Some(self.name) {
            let found = call
                .args
                .first()
                .ok_or_else(|| "warning helper missing kind argument".to_owned())
                .and_then(|arg| values(arg, &Constants::new(), &BTreeMap::new()));
            match (&mut self.kinds, found) {
                (Ok(all), Ok(found)) => all.extend(found),
                (_, Err(error)) => self.kinds = Err(error),
                _ => (),
            }
        }
        visit::visit_expr_call(self, call);
    }
}

pub fn table_keys(source: &str) -> Result<Vec<String>, String> {
    // Permit either (kind, descriptor) entries or descriptor { kind: ... } entries;
    // no public testing API or product table identifier is required.
    struct Tables {
        keys: Vec<String>,
        error: Option<String>,
        in_array: bool,
    }
    impl<'ast> Visit<'ast> for Tables {
        fn visit_expr_array(&mut self, node: &'ast syn::ExprArray) {
            let previous = self.in_array;
            self.in_array = true;
            visit::visit_expr_array(self, node);
            self.in_array = previous;
        }
        fn visit_expr_tuple(&mut self, node: &'ast syn::ExprTuple) {
            if self.in_array
                && node.elems.len() == 2
                && matches!(node.elems.get(1), Some(Expr::Struct(value)) if value.path.segments.last().is_some_and(|p| p.ident == "WarningPresentation"))
            {
                match values(&node.elems[0], &Constants::new(), &BTreeMap::new()) {
                    Ok(found) => self.keys.extend(found),
                    Err(error) => self.error = Some(error),
                }
            }
            visit::visit_expr_tuple(self, node);
        }
        fn visit_expr_struct(&mut self, node: &'ast syn::ExprStruct) {
            if self.in_array
                && node
                    .path
                    .segments
                    .last()
                    .is_some_and(|p| p.ident == "WarningPresentation")
            {
                for field in &node.fields {
                    if matches!(&field.member, Member::Named(name) if name == "kind") {
                        match values(&field.expr, &Constants::new(), &BTreeMap::new()) {
                            Ok(found) => self.keys.extend(found),
                            Err(error) => self.error = Some(error),
                        }
                    }
                }
            }
            visit::visit_expr_struct(self, node);
        }
    }
    let mut visitor = Tables {
        keys: Vec::new(),
        error: None,
        in_array: false,
    };
    visitor.visit_file(&ast(source));
    if let Some(error) = visitor.error {
        return Err(error);
    }
    if visitor.keys.is_empty() {
        return Err("missing WarningPresentation table (main has no kind table yet)".to_owned());
    }
    Ok(visitor.keys)
}

pub fn complete(produced: &Kinds, keys: &[String]) -> Result<(), String> {
    let table: Kinds = keys.iter().cloned().collect();
    if table.len() != keys.len() {
        return Err("duplicate warning table key".to_owned());
    }
    if produced != &table {
        return Err(format!(
            "missing={:?}; stale={:?}",
            produced.difference(&table).collect::<Vec<_>>(),
            table.difference(produced).collect::<Vec<_>>()
        ));
    }
    Ok(())
}

fn forwarded_codes(root: &Path, constants: &Constants) -> Result<(Kinds, Kinds), String> {
    let read = |path: &str| std::fs::read_to_string(root.join("rust").join(path)).unwrap();
    let lexical = struct_codes(
        &read("fsl-syntax/src/lexer.rs"),
        "LexWarning",
        constants,
        &BTreeMap::new(),
    )?;
    let mut next = Kinds::new();
    for parser in [
        "fsl-syntax/src/parser/next_requirements.rs",
        "fsl-syntax/src/parser/next_requirements/rules.rs",
    ] {
        next.extend(struct_codes(
            &read(parser),
            "EditionWarning",
            constants,
            &BTreeMap::new(),
        )?);
    }
    Ok((lexical, next))
}

// Only these value shapes feed the current owner's warning sinks.
fn emitted(value: &Expr) -> Result<(), String> {
    match value {
        Expr::Macro(value)
            if value.mac.path.is_ident("json")
                || value
                    .mac
                    .path
                    .segments
                    .iter()
                    .map(|part| part.ident.to_string())
                    .collect::<Vec<_>>()
                    == ["serde_json", "json"] =>
        {
            Ok(())
        }
        Expr::Macro(value) => Err(format!(
            "unmodeled warning macro {}",
            value.mac.path.segments.last().unwrap().ident
        )),
        // Callee ownership is checked by Delegates, including local initializers.
        Expr::Call(_) | Expr::Continue(_) => Ok(()),
        Expr::Match(value) => value.arms.iter().try_for_each(|arm| emitted(&arm.body)),
        Expr::MethodCall(_) if key(value).as_deref() == Some("solver_vacuity.iter().cloned()") => {
            Ok(())
        }
        _ => Err("unmodeled warning value".to_owned()),
    }
}
// New owners at the warning sink require an explicit census model.
struct Delegates<'ast> {
    locals: BTreeMap<String, &'ast Expr>,
    sink: bool,
    error: Option<String>,
}
impl<'ast> Visit<'ast> for Delegates<'ast> {
    fn visit_local(&mut self, node: &'ast syn::Local) {
        if let syn::Pat::Ident(name) = &node.pat
            && let Some(init) = &node.init
        {
            self.locals.insert(name.ident.to_string(), &init.expr);
        }
        visit::visit_local(self, node);
    }
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let previous = self.sink;
        self.sink |= key(&node.receiver).as_deref() == Some("warnings")
            && matches!(node.method.to_string().as_str(), "push" | "extend");
        if self.sink && !previous {
            for arg in &node.args {
                let value = key(arg)
                    .and_then(|name| self.locals.get(&name))
                    .copied()
                    .unwrap_or(arg);
                if let Err(error) = emitted(value) {
                    self.error = Some(error);
                }
                self.visit_expr(value);
            }
        }
        visit::visit_expr_method_call(self, node);
        self.sink = previous;
    }
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        let writes_warnings = node.args.iter().any(|arg| {
            matches!(arg, Expr::Reference(value) if value.mutability.is_some()
                && key(&value.expr).as_deref() == Some("warnings"))
        });
        if self.sink || writes_warnings {
            let name = key(&node.func).unwrap_or_else(|| "<computed>".to_owned());
            if !matches!(
                name.as_str(),
                "never_enabled_action_warning" | "vacuity_reachability_warning"
            ) {
                self.error = Some(format!("unmodeled warning delegate {name}"));
            }
        }
        visit::visit_expr_call(self, node);
    }
}

fn validate_warning_delegates(owner: &syn::Block) -> Result<(), String> {
    let mut delegates = Delegates {
        locals: BTreeMap::new(),
        sink: false,
        error: None,
    };
    delegates.visit_block(owner);
    delegates.error.map_or(Ok(()), Err)
}

pub fn producers(root: &Path) -> Result<Inventory, String> {
    let runtime = std::fs::read_to_string(root.join("rust/fsl-runtime/src/lib.rs")).unwrap();
    producers_with_runtime(root, &runtime)
}

pub fn producers_with_runtime(root: &Path, runtime: &str) -> Result<Inventory, String> {
    let read = |path: &str| std::fs::read_to_string(root.join("rust").join(path)).unwrap();
    // These are verify's warning-producing modules, not all JSON `kind` users.
    let constant_paths = [
        "fsl-core/src/diagnostics.rs",
        "fsl-core/src/decision_table.rs",
        "fsl-syntax/src/identifier.rs",
        "fsl-syntax/src/parser/next_requirements.rs",
        "fsl-syntax/src/parser/next_requirements/rules.rs",
        "fsl-syntax/src/edition.rs",
    ];
    let mut constants = Constants::new();
    for path in constant_paths {
        constants.extend(items(&read(path)).constants);
    }
    let mut out = Inventory::default();
    let mut add = |found: Inventory| {
        out.named.extend(found.named);
        out.kindless += found.kindless;
        out.suggestions += found.suggestions;
    };
    // Derive forwarded codes once, including info notes in the warnings array.
    let (lexical, next) = forwarded_codes(root, &constants)?;
    for path in [
        "fsl-core/src/diagnostics.rs",
        "fsl-core/src/compose.rs",
        "fsl-core/src/dialect.rs",
        "fsl-core/src/decision_table.rs",
        "fsl-core/src/rule_guards.rs",
        "fslc/src/strict_tags.rs",
    ] {
        let code = if path.ends_with("compose.rs") {
            &lexical
        } else {
            &next
        };
        add(scan_json(
            &ast(&read(path)),
            &constants,
            &BTreeMap::from([("warning.code".to_owned(), code.clone())]),
        )?);
    }
    let mut calls = Calls {
        name: "vacuity_reachability_warning",
        kinds: Ok(Kinds::new()),
    };
    let owner = function(runtime, "verification_warnings")?;
    validate_warning_delegates(&owner)?;
    calls.visit_block(&owner);
    let bindings = BTreeMap::from([("kind".to_owned(), calls.kinds?)]);
    for name in [
        "verification_warnings",
        "never_enabled_action_warning",
        "vacuity_reachability_warning",
    ] {
        add(scan_block(
            &function(runtime, name)?,
            &constants,
            &bindings,
        )?);
    }
    let output = read("fslc/src/verification_output.rs");
    add(scan_block(
        &function(&output, "vacuity_warning")?,
        &constants,
        &BTreeMap::from([(
            "finding.kind()".to_owned(),
            solver_kinds(&read("fsl-verifier/src/vacuity.rs"))?,
        )]),
    )?);
    add(scan_block(
        &function(&output, "warning")?,
        &constants,
        &BTreeMap::from([(
            "self.kind".to_owned(),
            skipped_inputs(&read("fsl-core/src/dialect.rs"))?,
        )]),
    )?);
    add(scan_block(
        &function(
            &read("fslc/src/frontend_output.rs"),
            "implicit_initial_value_warning",
        )?,
        &constants,
        &BTreeMap::new(),
    )?);

    add(migration_inventory(
        &read("fslc/src/migration.rs"),
        &constants,
    )?);
    Ok(out)
}

fn migration_inventory(migration: &str, constants: &Constants) -> Result<Inventory, String> {
    // Admission is a producer-side filter. Next-edition findings are errors;
    // repair-only `parse` warnings are not verify warnings (migration.rs:840).
    struct Admission<'a> {
        constants: &'a Constants,
        allowed: Result<Kinds, String>,
    }
    impl<'ast> Visit<'ast> for Admission<'_> {
        fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
            if node.method == "contains" {
                match (
                    &mut self.allowed,
                    values(&node.receiver, self.constants, &BTreeMap::new()),
                ) {
                    (Ok(all), Ok(found)) => all.extend(found),
                    (_, Err(error)) => self.allowed = Err(error),
                    _ => (),
                }
            }
            visit::visit_expr_method_call(self, node);
        }
    }
    let mut admission = Admission {
        constants,
        allowed: Ok(Kinds::new()),
    };
    admission.visit_block(&function(migration, "admit_source")?);
    let allowed = admission.allowed?;
    if allowed.is_empty() {
        return Err("missing admission code filter".to_owned());
    }
    let mut tuples = Tuples {
        kinds: Kinds::new(),
    };
    tuples.visit_block(&function(migration, "plan_migration")?);
    let bindings = BTreeMap::from([("code".to_owned(), tuples.kinds)]);
    let mut generated = StructCodes {
        target: "MigrationDiagnostic",
        constants,
        bindings: &bindings,
        result: Ok(Kinds::new()),
    };
    for name in [
        "plan_migration",
        "metadata_diagnostics",
        "default_diagnostics",
        "action_map_diagnostic",
    ] {
        generated.visit_block(&function(migration, name)?);
    }
    let generated = generated.result?;
    if !allowed.is_subset(&generated) {
        return Err("admission registry has a code without a migration producer".to_owned());
    }
    // Derive kind=self.code from the actual JSON converter, rather than assuming it.
    scan_block(
        &function(migration, "MigrationDiagnostic::json")?,
        constants,
        &BTreeMap::from([("self.code".to_owned(), allowed)]),
    )
}
