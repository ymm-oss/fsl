// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Lenient parser for fsl-ai project-level evidence declarations (issues
//! #509/#510/#511).
//!
//! The hard-contract `ai_component` and recursive `agent` dialects keep their
//! strict token-based parser (`crate::ai`) because they lower to a kernel or
//! graph check. The `dataset` / `statistical_property` / `ai_migration` /
//! `observed_property` declarations parsed here are external evidence jobs:
//! their `require` clauses are threshold labels, not kernel formulas
//! (`docs/DESIGN-stochastic.md`). This mirrors the frozen reference's
//! `src/fslc/ai_project.py`: a small typed metadata model built with a
//! brace-matching block scanner, not a strict grammar.

use crate::ai::{AiComponent, parse_ai_component};

/// Top-level declaration kinds this parser recognizes as fsl-ai project
/// blocks. A block whose kind is not in this set is ignored (mirrors the
/// frozen reference's `raw_blocks` for `ai_action`/`authority`/`retriever`/
/// `trust_boundary`/`ai_contract`/`evaluator`/`failure_mode`: these are
/// accepted as un-descended block boundaries but carry no evidence-execution
/// semantics in `fslc ai eval`/`regress`/`drift`/`compat`).
const PROJECT_BLOCKS: &[&str] = &[
    "ai_action",
    "ai_component",
    "ai_contract",
    "ai_migration",
    "authority",
    "dataset",
    "evaluator",
    "failure_mode",
    "observed_property",
    "retriever",
    "statistical_property",
    "trust_boundary",
];

/// Nested block kinds that may appear without a name and are only
/// meaningful inside a specific parent (`statistical_property`/
/// `observed_property`'s `slice`, `ai_migration`'s `preserve`/
/// `no_regression`).
const NESTED_BLOCKS: &[&str] = &["slice", "preserve", "no_regression"];

#[derive(Clone, Debug, PartialEq)]
pub struct AiMetricRequirement {
    /// `"min_samples"` | `"ci_lower"` | `"ci_upper"` | `"point_estimate"` |
    /// `"inconclusive"`.
    pub kind: String,
    pub metric: Option<String>,
    pub confidence: Option<f64>,
    pub comparator: Option<String>,
    pub threshold: Option<f64>,
    pub slice: String,
    pub min_samples: Option<u64>,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AiStatisticalProperty {
    pub name: String,
    pub target: Option<String>,
    pub dataset: Option<String>,
    pub evaluator: Option<String>,
    pub confidence: f64,
    pub requirements: Vec<AiMetricRequirement>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AiRegressionRequirement {
    pub metric: String,
    /// `"drop"` | `"increase"`.
    pub direction: String,
    pub comparator: String,
    pub threshold: f64,
    pub dataset: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AiMigration {
    pub name: String,
    pub regression_requirements: Vec<AiRegressionRequirement>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AiObservedRequirement {
    /// `"observed"` | `"drift"` | `"inconclusive"`.
    pub kind: String,
    pub metric: String,
    pub comparator: String,
    pub threshold: f64,
    pub compared_to: Option<String>,
    pub slice: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AiObservedProperty {
    pub name: String,
    pub target: Option<String>,
    pub source: Option<String>,
    pub window: Option<String>,
    pub requirements: Vec<AiObservedRequirement>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AiDataset {
    pub name: String,
    pub source: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AiProject {
    pub name: String,
    pub components: Vec<AiComponent>,
    pub datasets: Vec<AiDataset>,
    pub statistical_properties: Vec<AiStatisticalProperty>,
    pub observed_properties: Vec<AiObservedProperty>,
    pub migrations: Vec<AiMigration>,
}

impl AiProject {
    /// Resolve the source JSONL path declared by `dataset <name> { source
    /// "..."; }`, matching `_records_path`'s dataset-source fallback used
    /// when `fslc ai eval` is invoked without `--records` (`docs/LANGUAGE.md`
    /// documents this exact usage).
    #[must_use]
    pub fn dataset_source(&self, name: &str) -> Option<&str> {
        self.datasets
            .iter()
            .find(|dataset| dataset.name == name)
            .and_then(|dataset| dataset.source.as_deref())
    }

    /// Select the `statistical_property` an `fslc ai eval` invocation
    /// targets: an explicit `--property` name, else the unique property
    /// declared for `--dataset`, else the project's only property.
    ///
    /// # Errors
    ///
    /// Returns a message naming an unknown, missing, or ambiguous selection.
    pub fn select_statistical_property(
        &self,
        property_name: Option<&str>,
        dataset_name: Option<&str>,
    ) -> Result<&AiStatisticalProperty, String> {
        if let Some(property_name) = property_name {
            return self
                .statistical_properties
                .iter()
                .find(|prop| prop.name == property_name)
                .ok_or_else(|| format!("unknown statistical_property '{property_name}'"));
        }
        if let Some(dataset_name) = dataset_name {
            let matching = self
                .statistical_properties
                .iter()
                .filter(|prop| prop.dataset.as_deref() == Some(dataset_name))
                .collect::<Vec<_>>();
            if matching.len() == 1 {
                return Ok(matching[0]);
            }
            if matching.len() > 1 {
                return Err(format!(
                    "multiple statistical_property declarations use dataset '{dataset_name}'; pass --property"
                ));
            }
        }
        match self.statistical_properties.len() {
            1 => Ok(&self.statistical_properties[0]),
            0 => Err("no statistical_property declaration found".to_owned()),
            _ => {
                Err("multiple statistical_property declarations found; pass --property".to_owned())
            }
        }
    }

    /// Select the `ai_migration` an `fslc ai regress` invocation targets.
    ///
    /// # Errors
    ///
    /// Returns a message naming an unknown, missing, or ambiguous selection.
    pub fn select_migration(&self, migration_name: Option<&str>) -> Result<&AiMigration, String> {
        if let Some(migration_name) = migration_name {
            return self
                .migrations
                .iter()
                .find(|migration| migration.name == migration_name)
                .ok_or_else(|| format!("unknown ai_migration '{migration_name}'"));
        }
        match self.migrations.len() {
            1 => Ok(&self.migrations[0]),
            0 => Err("no ai_migration declaration found".to_owned()),
            _ => Err("multiple ai_migration declarations found; pass --migration".to_owned()),
        }
    }

    /// Select the `observed_property` an `fslc ai drift` invocation targets.
    ///
    /// # Errors
    ///
    /// Returns a message naming an unknown, missing, or ambiguous selection.
    pub fn select_observed_property(
        &self,
        property_name: Option<&str>,
    ) -> Result<&AiObservedProperty, String> {
        if let Some(property_name) = property_name {
            return self
                .observed_properties
                .iter()
                .find(|prop| prop.name == property_name)
                .ok_or_else(|| format!("unknown observed_property '{property_name}'"));
        }
        match self.observed_properties.len() {
            1 => Ok(&self.observed_properties[0]),
            0 => Err("no observed_property declaration found".to_owned()),
            _ => Err("multiple observed_property declarations found; pass --property".to_owned()),
        }
    }
}

/// Parse one fsl-ai project source into its typed declarations.
///
/// # Errors
///
/// Returns a message when no recognized top-level block is found, a block is
/// unterminated, or a `statistical_property`/`observed_property`/
/// `ai_migration` name is declared more than once.
pub fn parse_ai_project(source: &str, name: &str) -> Result<AiProject, String> {
    let blocks = top_blocks(source)?;
    if blocks.is_empty() {
        return Err("expected fsl-ai project declarations".to_owned());
    }
    let mut project = AiProject {
        name: name.to_owned(),
        ..AiProject::default()
    };
    for block in &blocks {
        match block.kind.as_str() {
            "ai_component" => {
                project
                    .components
                    .push(parse_ai_component(&block.text).map_err(|error| error.to_string())?);
            }
            "dataset" => project.datasets.push(parse_dataset(block)),
            "statistical_property" => project
                .statistical_properties
                .push(parse_statistical_property(block)?),
            "observed_property" => project
                .observed_properties
                .push(parse_observed_property(block)?),
            "ai_migration" => project.migrations.push(parse_migration(block)?),
            _ => {}
        }
    }
    reject_duplicates(
        project
            .statistical_properties
            .iter()
            .map(|p| p.name.as_str()),
        "statistical_property",
    )?;
    reject_duplicates(
        project.observed_properties.iter().map(|p| p.name.as_str()),
        "observed_property",
    )?;
    reject_duplicates(
        project.migrations.iter().map(|p| p.name.as_str()),
        "ai_migration",
    )?;
    Ok(project)
}

fn reject_duplicates<'a>(names: impl Iterator<Item = &'a str>, label: &str) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for name in names {
        if !seen.insert(name) {
            return Err(format!("duplicate {label} '{name}'"));
        }
    }
    Ok(())
}

// --- brace-matching block scanner (mirrors `_top_blocks` et al.) ----------

struct RawBlock {
    kind: String,
    name: String,
    /// The exact original text of the block, header included (`"kind name {
    /// ... }"`), used to hand `ai_component` blocks to the strict token
    /// parser unmodified.
    text: String,
    body: String,
}

struct BlockHeader {
    start: usize,
    kind: String,
    name: String,
    brace: usize,
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

#[allow(clippy::many_single_char_names)]
fn try_match_header(chars: &[char], start: usize) -> Option<BlockHeader> {
    let n = chars.len();
    let mut p = start;
    while p < n && is_ident_continue(chars[p]) {
        p += 1;
    }
    let kind: String = chars[start..p].iter().collect();
    let mut q = p;
    while q < n && chars[q].is_whitespace() {
        q += 1;
    }
    if q > p && q < n && is_ident_start(chars[q]) {
        let name_start = q;
        let mut r = q;
        while r < n && is_ident_continue(chars[r]) {
            r += 1;
        }
        let mut s = r;
        while s < n && chars[s].is_whitespace() {
            s += 1;
        }
        if s < n && chars[s] == '{' {
            return Some(BlockHeader {
                start,
                kind,
                name: chars[name_start..r].iter().collect(),
                brace: s,
            });
        }
    }
    let mut s = p;
    while s < n && chars[s].is_whitespace() {
        s += 1;
    }
    if s < n && chars[s] == '{' {
        return Some(BlockHeader {
            start,
            kind,
            name: String::new(),
            brace: s,
        });
    }
    None
}

fn find_block_header(chars: &[char], from: usize) -> Option<BlockHeader> {
    let n = chars.len();
    let mut pos = from;
    while pos < n {
        if is_ident_start(chars[pos])
            && (pos == 0 || !is_ident_continue(chars[pos - 1]))
            && let Some(header) = try_match_header(chars, pos)
        {
            return Some(header);
        }
        pos += 1;
    }
    None
}

fn matching_brace(chars: &[char], brace: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (i, &ch) in chars.iter().enumerate().skip(brace) {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
        } else if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

fn brace_depth(chars: &[char]) -> i32 {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for &ch in chars {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
        } else if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
        }
    }
    depth
}

fn top_blocks(source: &str) -> Result<Vec<RawBlock>, String> {
    let chars: Vec<char> = source.chars().collect();
    let mut blocks = Vec::new();
    let mut i = 0usize;
    let n = chars.len();
    while i < n {
        let Some(header) = find_block_header(&chars, i) else {
            break;
        };
        let known = PROJECT_BLOCKS.contains(&header.kind.as_str())
            || NESTED_BLOCKS.contains(&header.kind.as_str());
        if !known || (PROJECT_BLOCKS.contains(&header.kind.as_str()) && header.name.is_empty()) {
            i = header.start + 1;
            continue;
        }
        let Some(end) = matching_brace(&chars, header.brace) else {
            return Err(format!(
                "unterminated {} '{}' block",
                header.kind, header.name
            ));
        };
        if brace_depth(&chars[..header.start]) == 0 {
            blocks.push(RawBlock {
                text: chars[header.start..=end].iter().collect(),
                body: chars[header.brace + 1..end].iter().collect(),
                kind: header.kind,
                name: header.name,
            });
        }
        i = end + 1;
    }
    Ok(blocks)
}

fn strip_comment(line: &str) -> &str {
    let chars: Vec<char> = line.chars().collect();
    let mut in_string = false;
    let mut escaped = false;
    let mut i = 0usize;
    while i + 1 < chars.len() {
        let ch = chars[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == '/' && chars[i + 1] == '/' {
            let byte_offset: usize = chars[..i].iter().map(|c| c.len_utf8()).sum();
            return &line[..byte_offset];
        }
        i += 1;
    }
    line
}

fn strip_semi(line: &str) -> String {
    line.strip_suffix(';')
        .map_or(line, str::trim)
        .trim()
        .to_owned()
}

/// One logical top-level statement per line inside `body`, comments
/// stripped and nested-block lines (including the block's own header line)
/// skipped, mirroring `_top_lines`.
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
fn top_lines(body: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut depth = 0i32;
    for raw in body.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let before = depth;
        depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
        if before == 0 && !line.contains('{') && !line.contains('}') {
            lines.push(strip_semi(line));
        }
    }
    lines
}

fn atom(text: &str) -> String {
    let text = strip_semi(text.trim());
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        text[1..text.len() - 1].to_owned()
    } else {
        text
    }
}

fn metric_name(text: &str) -> String {
    text.trim()
        .strip_prefix("metric.")
        .unwrap_or(text.trim())
        .to_owned()
}

/// The first comparator in `s` (two-character forms take priority), or
/// `None` if `s` has none.
fn find_comparator(s: &str) -> Option<(usize, &'static str)> {
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        if s[i..].starts_with(">=") {
            return Some((i, ">="));
        }
        if s[i..].starts_with("<=") {
            return Some((i, "<="));
        }
        if s[i..].starts_with("==") {
            return Some((i, "=="));
        }
        if bytes[i] == b'>' {
            return Some((i, ">"));
        }
        if bytes[i] == b'<' {
            return Some((i, "<"));
        }
    }
    None
}

fn is_dotted_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if is_ident_start(c) => {}
        _ => return false,
    }
    chars.all(|c| is_ident_continue(c) || c == '.')
}

// --- per-block parsers (mirror `_parse_*` in the frozen reference) --------

fn parse_dataset(block: &RawBlock) -> AiDataset {
    let mut source = None;
    for line in top_lines(&block.body) {
        if let Some(rest) = line.strip_prefix("source ") {
            source = Some(atom(rest));
        }
    }
    AiDataset {
        name: block.name.clone(),
        source,
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn parse_metric_requirement(line: &str, slice_name: &str) -> AiMetricRequirement {
    let source = line.to_owned();
    let expr = line.trim();
    if let Some((idx, op)) = find_comparator(expr) {
        let left = expr[..idx].trim();
        let right = expr[idx + op.len()..].trim();
        if left == "min_samples"
            && let Ok(value) = right.parse::<f64>()
        {
            return AiMetricRequirement {
                kind: "min_samples".to_owned(),
                metric: None,
                confidence: None,
                comparator: Some(op.to_owned()),
                threshold: Some(value),
                slice: slice_name.to_owned(),
                min_samples: Some(value as u64),
                source,
            };
        }
        for (prefix, kind) in [("ci_lower(", "ci_lower"), ("ci_upper(", "ci_upper")] {
            if let Some(inner) = left.strip_prefix(prefix).and_then(|s| s.strip_suffix(')'))
                && let Some((metric_part, confidence_part)) = inner.split_once(',')
                && let Ok(confidence) = confidence_part.trim().parse::<f64>()
                && let Ok(threshold) = right.parse::<f64>()
            {
                return AiMetricRequirement {
                    kind: kind.to_owned(),
                    metric: Some(metric_name(metric_part)),
                    confidence: Some(confidence),
                    comparator: Some(op.to_owned()),
                    threshold: Some(threshold),
                    slice: slice_name.to_owned(),
                    min_samples: None,
                    source,
                };
            }
        }
        if is_dotted_ident(left)
            && let Ok(threshold) = right.parse::<f64>()
        {
            return AiMetricRequirement {
                kind: "point_estimate".to_owned(),
                metric: Some(metric_name(left)),
                confidence: None,
                comparator: Some(op.to_owned()),
                threshold: Some(threshold),
                slice: slice_name.to_owned(),
                min_samples: None,
                source,
            };
        }
    }
    AiMetricRequirement {
        kind: "inconclusive".to_owned(),
        metric: None,
        confidence: None,
        comparator: None,
        threshold: None,
        slice: slice_name.to_owned(),
        min_samples: None,
        source,
    }
}

fn parse_statistical_property(block: &RawBlock) -> Result<AiStatisticalProperty, String> {
    let mut target = None;
    let mut dataset = None;
    let mut evaluator = None;
    let mut confidence = 0.95;
    let mut requirements = Vec::new();
    for line in top_lines(&block.body) {
        if let Some(rest) = line.strip_prefix("target ") {
            target = Some(atom(rest));
        } else if let Some(rest) = line.strip_prefix("dataset ") {
            dataset = Some(atom(rest));
        } else if let Some(rest) = line.strip_prefix("evaluator ") {
            evaluator = Some(atom(rest));
        } else if let Some(rest) = line.strip_prefix("confidence ") {
            confidence = atom(rest).parse().map_err(|_| {
                format!(
                    "statistical_property '{}' has a non-numeric confidence",
                    block.name
                )
            })?;
        } else if let Some(rest) = line.strip_prefix("require ") {
            requirements.push(parse_metric_requirement(rest, "all"));
        }
    }
    for child in top_blocks(&block.body)? {
        if child.kind != "slice" {
            continue;
        }
        for line in top_lines(&child.body) {
            if let Some(rest) = line.strip_prefix("require ") {
                requirements.push(parse_metric_requirement(rest, &child.name));
            }
        }
    }
    Ok(AiStatisticalProperty {
        name: block.name.clone(),
        target,
        dataset,
        evaluator,
        confidence,
        requirements,
    })
}

fn parse_observed_requirement(line: &str, slice_name: &str) -> AiObservedRequirement {
    let expr = line.trim();
    if let Some((idx, op)) = find_comparator(expr) {
        let left = expr[..idx].trim();
        let right = expr[idx + op.len()..].trim();
        if let Some(inner) = left
            .strip_prefix("observed(")
            .and_then(|s| s.strip_suffix(')'))
            && let Ok(threshold) = right.parse::<f64>()
        {
            return AiObservedRequirement {
                kind: "observed".to_owned(),
                metric: metric_name(inner),
                comparator: op.to_owned(),
                threshold,
                compared_to: None,
                slice: slice_name.to_owned(),
                source: line.to_owned(),
            };
        }
        if let Some(inner) = left
            .strip_prefix("drift(")
            .and_then(|s| s.strip_suffix(')'))
        {
            let mut parts = right.splitn(2, "compared_to");
            let threshold_part = parts.next().unwrap_or("").trim();
            let compared_to = parts.next().map(str::trim).filter(|s| !s.is_empty());
            if let (Ok(threshold), Some(compared_to)) = (threshold_part.parse::<f64>(), compared_to)
            {
                return AiObservedRequirement {
                    kind: "drift".to_owned(),
                    metric: metric_name(inner),
                    comparator: op.to_owned(),
                    threshold,
                    compared_to: Some(compared_to.to_owned()),
                    slice: slice_name.to_owned(),
                    source: line.to_owned(),
                };
            }
        }
    }
    AiObservedRequirement {
        kind: "inconclusive".to_owned(),
        metric: "unknown".to_owned(),
        comparator: "==".to_owned(),
        threshold: 0.0,
        compared_to: None,
        slice: slice_name.to_owned(),
        source: line.to_owned(),
    }
}

fn parse_observed_property(block: &RawBlock) -> Result<AiObservedProperty, String> {
    let mut target = None;
    let mut source = None;
    let mut window = None;
    let mut requirements = Vec::new();
    for line in top_lines(&block.body) {
        if let Some(rest) = line.strip_prefix("target ") {
            target = Some(atom(rest));
        } else if let Some(rest) = line.strip_prefix("source ") {
            source = Some(atom(rest));
        } else if let Some(rest) = line.strip_prefix("window ") {
            window = Some(atom(rest));
        } else if let Some(rest) = line.strip_prefix("require ") {
            requirements.push(parse_observed_requirement(rest, "all"));
        }
    }
    for child in top_blocks(&block.body)? {
        if child.kind != "slice" {
            continue;
        }
        for line in top_lines(&child.body) {
            if let Some(rest) = line.strip_prefix("require ") {
                requirements.push(parse_observed_requirement(rest, &child.name));
            }
        }
    }
    Ok(AiObservedProperty {
        name: block.name.clone(),
        target,
        source,
        window,
        requirements,
    })
}

fn parse_regression_requirement(
    line: &str,
    dataset: Option<String>,
) -> Result<AiRegressionRequirement, String> {
    let unsupported = || format!("unsupported no_regression metric clause: {line}");
    let rest = line.strip_prefix("metric ").ok_or_else(unsupported)?;
    let (idx, op) = find_comparator(rest).ok_or_else(unsupported)?;
    let left = rest[..idx].trim();
    let threshold: f64 = rest[idx + op.len()..]
        .trim()
        .parse()
        .map_err(|_| unsupported())?;
    let mut left_parts = left.split_whitespace();
    let metric = left_parts.next().ok_or_else(unsupported)?;
    let direction = left_parts.next().ok_or_else(unsupported)?;
    if left_parts.next().is_some() || !matches!(direction, "drop" | "increase") {
        return Err(unsupported());
    }
    Ok(AiRegressionRequirement {
        metric: metric_name(metric),
        direction: direction.to_owned(),
        comparator: op.to_owned(),
        threshold,
        dataset,
    })
}

fn parse_migration(block: &RawBlock) -> Result<AiMigration, String> {
    let mut regression_requirements = Vec::new();
    for child in top_blocks(&block.body)? {
        if child.kind != "preserve" {
            continue;
        }
        for grandchild in top_blocks(&child.body)? {
            if grandchild.kind != "no_regression" {
                continue;
            }
            let mut dataset = None;
            for line in top_lines(&grandchild.body) {
                if let Some(rest) = line.strip_prefix("dataset ") {
                    dataset = Some(atom(rest));
                } else if line.starts_with("metric ") {
                    regression_requirements
                        .push(parse_regression_requirement(&line, dataset.clone())?);
                }
            }
        }
    }
    Ok(AiMigration {
        name: block.name.clone(),
        regression_requirements,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_statistical_property_with_slice_gate() {
        let source = r"
statistical_property LooseQuality {
  target SupportAnswerAgent
  dataset SupportEvalV3
  evaluator SupportAnswerJudge
  confidence 0.95

  require ci_lower(metric.accuracy, 0.95) >= 0.45

  slice JapaneseRefundTickets {
    require min_samples >= 5
    require ci_lower(metric.accuracy, 0.95) >= 0.35
  }
}
";
        let project = parse_ai_project(source, "P").expect("parse");
        assert_eq!(project.statistical_properties.len(), 1);
        let prop = &project.statistical_properties[0];
        assert_eq!(prop.dataset.as_deref(), Some("SupportEvalV3"));
        assert_eq!(prop.evaluator.as_deref(), Some("SupportAnswerJudge"));
        assert_eq!(prop.requirements.len(), 3);
        assert_eq!(prop.requirements[0].slice, "all");
        assert_eq!(prop.requirements[0].kind, "ci_lower");
        assert_eq!(prop.requirements[0].threshold, Some(0.45));
        assert_eq!(prop.requirements[1].slice, "JapaneseRefundTickets");
        assert_eq!(prop.requirements[1].kind, "min_samples");
        assert_eq!(prop.requirements[1].min_samples, Some(5));
        assert_eq!(prop.requirements[2].kind, "ci_lower");
        assert_eq!(prop.requirements[2].threshold, Some(0.35));
    }

    #[test]
    fn parses_migration_no_regression_clauses() {
        let source = r"
ai_migration PromptV7ToV8 {
  preserve {
    no_regression {
      dataset SupportEvalV3
      metric accuracy drop <= 0.05
      metric hallucination_rate increase <= 0.02
    }
  }
}
";
        let project = parse_ai_project(source, "P").expect("parse");
        let migration = project.select_migration(None).expect("selection");
        assert_eq!(migration.regression_requirements.len(), 2);
        assert_eq!(migration.regression_requirements[0].metric, "accuracy");
        assert_eq!(migration.regression_requirements[0].direction, "drop");
        assert!((migration.regression_requirements[0].threshold - 0.05).abs() < f64::EPSILON);
        assert_eq!(
            migration.regression_requirements[1].metric,
            "hallucination_rate"
        );
        assert_eq!(migration.regression_requirements[1].direction, "increase");
    }

    #[test]
    fn parses_observed_property_with_drift_clause() {
        let source = r"
observed_property SupportAgentOperationalQuality {
  target SupportAnswerAgent
  source production_logs
  window last_7_days

  require observed(metric.hallucination_rate) <= 0.30
  require drift(metric.refusal_rate) <= 0.10 compared_to previous_7_days
}
";
        let project = parse_ai_project(source, "P").expect("parse");
        let prop = project.select_observed_property(None).expect("selection");
        assert_eq!(prop.requirements.len(), 2);
        assert_eq!(prop.requirements[0].kind, "observed");
        assert_eq!(prop.requirements[1].kind, "drift");
        assert_eq!(
            prop.requirements[1].compared_to.as_deref(),
            Some("previous_7_days")
        );
    }

    #[test]
    fn rejects_source_with_no_recognized_block() {
        let error = parse_ai_project("domain D {}", "P").unwrap_err();
        assert!(error.contains("expected fsl-ai project declarations"));
    }

    #[test]
    fn dataset_source_resolves_records_fallback() {
        let source = r#"
dataset SupportEvalV3 {
  source "examples/ai/support_eval_v3.jsonl"
}
statistical_property Loose {
  target X
  dataset SupportEvalV3
  require ci_lower(metric.accuracy, 0.95) >= 0.45
}
"#;
        let project = parse_ai_project(source, "P").expect("parse");
        assert_eq!(
            project.dataset_source("SupportEvalV3"),
            Some("examples/ai/support_eval_v3.jsonl")
        );
    }
}
