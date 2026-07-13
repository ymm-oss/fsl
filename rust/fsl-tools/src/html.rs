// SPDX-License-Identifier: Apache-2.0

#![allow(
    clippy::cast_precision_loss,
    clippy::format_collect,
    clippy::too_many_arguments
)]

//! Self-contained HTML report rendering from native FSL JSON envelopes.

use std::fmt::Write as _;

use serde_json::{Value, json};

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn text(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn status_class(status: &str) -> &'static str {
    match status {
        "verified" | "proved" | "ok" | "covered" | "refines" | "generated" => "ok",
        "violated" | "reachable_failed" | "nonconformant" | "refinement_failed" | "uncovered" => {
            "bad"
        }
        "warning" | "unknown_cti" => "warn",
        _ => "info",
    }
}
fn badge(value: &Value) -> String {
    let value = text(value);
    format!(
        "<span class=\"badge {}\">{}</span>",
        status_class(&value),
        escape(&value)
    )
}
fn metric(label: &str, value: &str) -> String {
    format!(
        "<div class=\"metric\"><span>{}</span><strong>{value}</strong></div>",
        escape(label)
    )
}
fn details(label: &str, inner: &str) -> String {
    format!(
        "<details><summary>{}</summary><div>{inner}</div></details>",
        escape(label)
    )
}
fn sorted_json(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(sorted_json).collect()),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), sorted_json(value)))
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        other => other.clone(),
    }
}
fn json_pre(value: &Value) -> String {
    format!(
        "<div class=\"code-block\"><pre>{}</pre></div>",
        escape(&serde_json::to_string_pretty(&sorted_json(value)).unwrap_or_default())
    )
}

fn json_code(value: &Value) -> String {
    fn render(value: &Value) -> String {
        match value {
            Value::Array(values) => format!(
                "[{}]",
                values.iter().map(render).collect::<Vec<_>>().join(", ")
            ),
            Value::Object(values) => {
                let mut values = values.iter().collect::<Vec<_>>();
                values.sort_by_key(|(key, _)| key.as_str());
                format!(
                    "{{{}}}",
                    values
                        .into_iter()
                        .map(|(key, value)| format!(
                            "{}: {}",
                            serde_json::to_string(key).unwrap_or_default(),
                            render(value)
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            other => serde_json::to_string(other).unwrap_or_default(),
        }
    }
    format!("<code>{}</code>", escape(&render(value)))
}

fn sidebar(spec: &str) -> String {
    format!(
        r##"
    <aside class="side" aria-label="Report navigation">
      <div class="mark"><span class="mark-dot" aria-hidden="true"></span><span>{}</span></div>
      <nav>
        <a href="#model">Model</a>
        <a href="#actions">Actions</a>
        <a href="#properties">Properties</a>
        <a href="#status">Status</a>
        <a href="#traces">Traces</a>
        <a href="#witnesses">Witnesses</a>
        <a href="#counterfactuals">Counterfactuals</a>
        <a href="#source">Source</a>
      </nav>
    </aside>
"##,
        escape(spec)
    )
}

fn hero(
    spec: &str,
    file: &str,
    depth: &Value,
    status: &str,
    state: usize,
    actions: usize,
    properties: usize,
    coverage: &str,
    warnings: usize,
    domains: usize,
    kpis: usize,
) -> String {
    let state_word = if state == 1 {
        "state variable"
    } else {
        "state variables"
    };
    let action_word = if actions == 1 { "action" } else { "actions" };
    let property_word = if properties == 1 {
        "property"
    } else {
        "properties"
    };
    let mut subtitle = format!(
        "A model of {state} {state_word}, {actions} {action_word}, and {properties} {property_word}."
    );
    let mut extras = Vec::new();
    if domains > 0 {
        extras.push(format!("{domains} entity/domain declaration(s)"));
    }
    if kpis > 0 {
        extras.push(format!("{kpis} KPI(s)"));
    }
    if !extras.is_empty() {
        let _ = write!(subtitle, " Tracks {}.", extras.join(" and "));
    }
    format!(
        r#"
      <header class="hero">
        <div>
          <p class="eyebrow">FSL specification report</p>
          <h1>{}</h1>
          <p class="sub">{}</p>
        </div>
        <div class="meta-grid" aria-label="Report summary">
          {}
          {}
          {}
          {}
          {}
          {}
        </div>
        <div class="chips">
          <span class="chip">{}</span>
          <span class="chip">{warnings} warning(s)</span>
        </div>
      </header>
"#,
        escape(spec),
        escape(&subtitle),
        metric(
            "Result",
            &format!(
                "<span class=\"badge {}\">{}</span>",
                status_class(status),
                escape(status)
            )
        ),
        metric("Depth", &escape(&text(depth))),
        metric("States", &state.to_string()),
        metric("Actions", &actions.to_string()),
        metric("Properties", &properties.to_string()),
        metric("Coverage", &escape(coverage)),
        escape(file)
    )
}

fn type_text(value: &Value, enums: &Value) -> String {
    let Some(values) = value.as_array() else {
        return text(value);
    };
    let tag = values.first().and_then(Value::as_str).unwrap_or_default();
    match tag {
        "bool" => "Bool".to_owned(),
        "int" => "Int".to_owned(),
        "domain" => format!("{}..{}", text(&values[1]), text(&values[2])),
        "map" => format!(
            "Map<{}, {}>",
            type_text(&values[1], enums),
            type_text(&values[2], enums)
        ),
        "option" => format!("Option<{}>", type_text(&values[1], enums)),
        "set" => format!("Set<{}>", type_text(&values[1], enums)),
        "seq" => format!(
            "Seq<{}, {}>",
            type_text(&values[1], enums),
            text(&values[2])
        ),
        "relation" => format!(
            "relation {} -> {}",
            type_text(&values[1], enums),
            type_text(&values[2], enums)
        ),
        "enum" => {
            let name = text(&values[1]);
            enums
                .get(&name)
                .and_then(Value::as_array)
                .map_or(name.clone(), |members| {
                    format!(
                        "{name} {{{}}}",
                        members.iter().map(text).collect::<Vec<_>>().join(", ")
                    )
                })
        }
        "struct" | "named" | "name" => text(&values[1]),
        _ => text(value),
    }
}

fn positions(count: usize, height: usize) -> Vec<String> {
    if count == 0 {
        return Vec::new();
    }
    if count == 1 {
        return vec![(height / 2).to_string()];
    }
    let gap = (height as f64 - 42.0 - 72.0) / (count - 1) as f64;
    (0..count)
        .map(|index| format!("{:.1}", (72.0 + gap * index as f64) * 10.0_f64.powi(0)))
        .collect()
}

fn influence_svg(states: &[String], actions: &[Value]) -> String {
    let width = 720;
    let height = 320_usize.max(90 + 54 * states.len().max(actions.len()).max(1));
    let sy = positions(states.len(), height);
    let ay = positions(actions.len(), height);
    let mut edges = String::new();
    for (index, action) in actions.iter().enumerate() {
        for write in action["writes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if let Some(state_index) = states.iter().position(|state| state == write) {
                let _ = write!(
                    edges,
                    "<path d=\"M 455 {} C 360 {}, 360 {}, 268 {}\" fill=\"none\" stroke=\"#087f7a\" stroke-width=\"1.6\" opacity=\"0.55\" marker-end=\"url(#flow-arrow)\"/>",
                    ay[index], ay[index], sy[state_index], sy[state_index]
                );
            }
        }
    }
    let state_nodes=states.iter().zip(&sy).map(|(name,y)|format!("<rect x=\"46\" y=\"{}\" width=\"220\" height=\"34\" rx=\"8\" fill=\"#d8f0eb\" stroke=\"#afdcd7\"/><text x=\"62\" y=\"{}\">{}</text>",number_minus(y,17.0),number_plus(y,5.0),escape(name))).collect::<String>();
    let action_nodes=actions.iter().zip(&ay).map(|(action,y)|{let label=action["name"].as_str().unwrap_or_default();format!("<rect x=\"455\" y=\"{}\" width=\"220\" height=\"34\" rx=\"8\" fill=\"#fff2d2\" stroke=\"#efd39a\"/><text x=\"471\" y=\"{}\">{}</text>",number_minus(y,17.0),number_plus(y,5.0),escape(&shorten(label,27))) }).collect::<String>();
    format!(
        r##"
      <svg class="flow-svg" viewBox="0 0 {width} {height}" role="img" aria-label="Action to state write graph">
        <defs>
          <marker id="flow-arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="8" markerHeight="8" orient="auto">
            <path d="M 0 0 L 10 5 L 0 10 z" fill="#087f7a"/>
          </marker>
        </defs>
        <text class="small" x="48" y="34">STATE &#8592; writes</text>
        <text class="small" x="457" y="34">ACTIONS</text>
        {edges}
        {state_nodes}
        {action_nodes}
      </svg>
"##
    )
}
fn number_minus(value: &str, delta: f64) -> String {
    let result = value.parse::<f64>().unwrap_or_default() - delta;
    if value.contains('.') {
        format_number(result)
    } else {
        format!("{result:.0}")
    }
}
fn number_plus(value: &str, delta: f64) -> String {
    let result = value.parse::<f64>().unwrap_or_default() + delta;
    if value.contains('.') {
        format_number(result)
    } else {
        format!("{result:.0}")
    }
}
fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.1}")
    } else {
        value.to_string()
    }
}
fn shorten(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        value.to_owned()
    } else {
        value.chars().take(limit - 1).collect::<String>() + "..."
    }
}

fn stage_flow_panel(stage_flows: &[Value]) -> String {
    stage_flows
        .iter()
        .map(|flow| {
            let transitions = flow["transitions"].as_array().cloned().unwrap_or_default();
            let has_actor = transitions.iter().any(|transition| {
                transition
                    .get("actor")
                    .and_then(Value::as_str)
                    .is_some_and(|actor| !actor.is_empty())
            });
            let stages = flow["stages"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|stage| format!("<span class=\"chip\">{}</span>", escape(&text(stage))))
                .collect::<String>();
            let rows = transitions
                .iter()
                .map(|transition| {
                    let actor = if has_actor {
                        format!("<td>{}</td>", escape(&text(&transition["actor"])))
                    } else {
                        String::new()
                    };
                    format!(
                        "<tr><td>{}</td><td>{}</td><td><code>{}</code></td>{actor}</tr>",
                        escape(&text(&transition["from"])),
                        escape(&text(&transition["to"])),
                        escape(&text(&transition["action"])),
                    )
                })
                .collect::<String>();
            let actor_header = if has_actor { "<th>Actor</th>" } else { "" };
            format!(
                r#"
          <div class="panel table-wrap">
            <div class="panel-pad">
              <h3>{}: {} stages</h3>
              <div class="chips">{stages}</div>
            </div>
            <table>
              <thead><tr><th>From</th><th>To</th><th>Action</th>{actor_header}</tr></thead>
              <tbody>{rows}</tbody>
            </table>
          </div>
"#,
                escape(&text(&flow["state"])),
                escape(&text(&flow["type"]))
            )
        })
        .collect()
}

fn model_section(
    state: &Value,
    actions: &[Value],
    enums: &Value,
    domains: &[Value],
    stage_flows: &[Value],
) -> String {
    let mut names = state
        .as_object()
        .map(|state| state.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    names.sort();
    let rows = names
        .iter()
        .map(|name| {
            format!(
                "<tr><td><code>{}</code></td><td>{}</td></tr>",
                escape(name),
                escape(&type_text(&state[name], enums))
            )
        })
        .collect::<String>();
    let domain_panel = if domains.is_empty() {
        String::new()
    } else {
        format!(
            "<div class=\"panel panel-pad\"><h3>Entities &amp; Domains</h3><ul>{}</ul></div>",
            domains
                .iter()
                .map(|value| format!("<li>{}</li>", escape(&text(value))))
                .collect::<String>()
        )
    };
    let extras = format!("{domain_panel}{}", stage_flow_panel(stage_flows));
    format!(
        r#"
      <section class="section" id="model">
        <div class="section-head">
          <div>
            <h2>State Model</h2>
            <p>Declared state and the actions that write to it.</p>
          </div>
        </div>
        <div class="grid-2">
          <div class="panel table-wrap">
            <table>
              <thead><tr><th>State</th><th>Type</th></tr></thead>
              <tbody>{}</tbody>
            </table>
          </div>
          <div>{}</div>
        </div>
        {extras}
      </section>
"#,
        if rows.is_empty() {
            "<tr><td colspan=\"2\">No state variables.</td></tr>"
        } else {
            &rows
        },
        influence_svg(&names, actions)
    )
}

fn chip_list(values: &Value, kind: &str) -> String {
    let values = values.as_array().cloned().unwrap_or_default();
    if values.is_empty() {
        return "<span class=\"chip\">none</span>".to_owned();
    }
    format!(
        "<div class=\"chips\">{}</div>",
        values
            .iter()
            .map(|value| format!(
                "<span class=\"{}\">{}</span>",
                if kind.is_empty() {
                    "chip".to_owned()
                } else {
                    format!("chip {kind}")
                },
                escape(&text(value))
            ))
            .collect::<String>()
    )
}

fn actions_section(actions: &[Value], coverage: &Value) -> String {
    let mut rows = String::new();
    for action in actions {
        let name = action["name"].as_str().unwrap_or_default();
        let params = action["params"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|param| format!("{}: {}", text(&param["name"]), text(&param["type"])))
            .collect::<Vec<_>>()
            .join(", ");
        let requires = json!(
            action["requires_text"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|value| value
                    .as_str()
                    .unwrap_or_default()
                    .strip_prefix("requires ")
                    .unwrap_or(value.as_str().unwrap_or_default()))
                .collect::<Vec<_>>()
        );
        let ensures = json!(
            action["ensures_text"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|value| value
                    .as_str()
                    .unwrap_or_default()
                    .strip_prefix("ensures ")
                    .unwrap_or(value.as_str().unwrap_or_default()))
                .collect::<Vec<_>>()
        );
        let mut markers = String::new();
        if action["fair"] == true {
            markers.push_str("<span class=\"chip fair\">fair</span>");
        }
        if let Some(value) = coverage.get(name) {
            markers.push_str(&badge(&json!(if value == true {
                "covered"
            } else {
                "uncovered"
            })));
        }
        let ensures_html = if ensures.as_array().is_some_and(Vec::is_empty) {
            String::new()
        } else {
            chip_list(&ensures, "")
        };
        let _ = write!(
            rows,
            "<tr><td><code>{}</code>{}<br>{markers}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            escape(name),
            requirement_caption(&action["requirement"]),
            if params.is_empty() {
                "<span class=\"chip\">none</span>".to_owned()
            } else {
                escape(&params)
            },
            chip_list(&requires, "require"),
            chip_list(&action["writes"], "write"),
            ensures_html
        );
    }
    format!(
        r#"
      <section class="section" id="actions">
        <div class="section-head">
          <div>
            <h2>Actions</h2>
            <p>Preconditions, writes, postconditions, and coverage.</p>
          </div>
        </div>
        <div class="panel table-wrap">
          <table>
            <thead><tr><th>Action</th><th>Params</th><th>Requires</th><th>Writes</th><th>Ensures</th></tr></thead>
            <tbody>{}</tbody>
          </table>
        </div>
      </section>
"#,
        if rows.is_empty() {
            "<tr><td colspan=\"5\">No actions.</td></tr>"
        } else {
            &rows
        }
    )
}

fn assurance(verification: &Value) -> String {
    if verification["completeness"] == "unbounded" {
        "proved(induction)".to_owned()
    } else if verification["completeness"] == "bounded" {
        format!(
            "bounded(BMC depth {})",
            text(
                verification
                    .get("checked_to_depth")
                    .unwrap_or(&verification["depth"])
            )
        )
    } else {
        "not_run".to_owned()
    }
}

fn properties_section(properties: &[Value], checks: &[Value], verification: &Value) -> String {
    let mut rows = String::new();
    for property in properties {
        let _ = write!(
            rows,
            "<tr><td>{}</td><td><code>{}</code></td><td>{}</td><td>{}</td></tr>",
            escape(&text(&property["kind"])),
            escape(&text(&property["name"])),
            escape(&assurance(verification)),
            escape(&text(&property["body_text"]))
        );
    }
    let mut check_rows = String::new();
    for check in checks {
        let label = check
            .get("target")
            .or_else(|| check.get("action"))
            .or_else(|| check.get("name"))
            .unwrap_or(&Value::Null);
        let source = check
            .get("text")
            .map_or_else(|| "implicit bounded-domain check".to_owned(), text);
        let _ = write!(
            check_rows,
            "<tr><td>{}</td><td><code>{}</code></td><td>{}</td></tr>",
            escape(&text(&check["kind"])),
            escape(&text(label)),
            escape(&source)
        );
    }
    format!(
        r#"
      <section class="section" id="properties">
        <div class="section-head">
          <div>
            <h2>Properties And Automatic Checks</h2>
            <p>Human-authored properties plus checks inserted by the verifier.</p>
          </div>
        </div>
        <div class="stack">
          <div class="panel table-wrap">
            <table>
              <thead><tr><th>Kind</th><th>Name</th><th>Assurance</th><th>Body</th></tr></thead>
              <tbody>{}</tbody>
            </table>
          </div>
          <div class="panel table-wrap">
            <table>
              <thead><tr><th>Check</th><th>Target</th><th>Source</th></tr></thead>
              <tbody>{}</tbody>
            </table>
          </div>
        </div>
      </section>
"#,
        if rows.is_empty() {
            "<tr><td colspan=\"4\">No user properties.</td></tr>"
        } else {
            &rows
        },
        if check_rows.is_empty() {
            "<tr><td colspan=\"3\">No automatic checks.</td></tr>"
        } else {
            &check_rows
        }
    )
}

fn status_section(verification: &Value) -> String {
    let mut fields = vec![
        ("Result", badge(&verification["result"])),
        ("Assurance", escape(&assurance(verification))),
        (
            "Completeness",
            escape(&text(
                verification.get("completeness").unwrap_or(&json!("n/a")),
            )),
        ),
        (
            "Checked depth",
            escape(&text(
                verification
                    .get("checked_to_depth")
                    .or_else(|| verification.get("depth"))
                    .unwrap_or(&json!("n/a")),
            )),
        ),
        (
            "Note",
            escape(&text(verification.get("note").unwrap_or(&json!("")))),
        ),
        (
            "Hint",
            escape(&text(verification.get("hint").unwrap_or(&json!("")))),
        ),
    ];
    if let Some(implements) = verification.get("implements") {
        fields.push(("Implements", json_code(implements)));
    }
    if let Some(deadlock) = verification.get("deadlock") {
        fields.push(("Deadlock", json_code(deadlock)));
    }
    let rows = fields
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| format!("<tr><th>{}</th><td>{value}</td></tr>", escape(key)))
        .collect::<String>();
    let warnings = verification["warnings"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let warning_html = if warnings.is_empty() {
        "<div class=\"panel panel-pad\"><h3>Warnings</h3><p class=\"empty\">No warnings reported.</p></div>".to_owned()
    } else {
        format!(
            "<div class=\"panel table-wrap\"><table><thead><tr><th>Kind</th><th>Message</th></tr></thead><tbody>{}</tbody></table></div>",
            warnings
                .iter()
                .map(|warning| format!(
                    "<tr><td>{}</td><td>{}</td></tr>",
                    escape(
                        warning
                            .get("kind")
                            .and_then(Value::as_str)
                            .unwrap_or("warning")
                    ),
                    escape(&text(warning.get("message").unwrap_or(warning)))
                ))
                .collect::<String>()
        )
    };
    format!(
        r#"
      <section class="section" id="status">
        <div class="section-head">
          <div>
            <h2>Verification Status</h2>
            <p>Bounded or proof results from <code>fslc verify</code>.</p>
          </div>
        </div>
        <div class="grid-2">
          <div class="panel table-wrap">
            <table>
              <tbody>{rows}</tbody>
            </table>
          </div>
          {warning_html}
        </div>
      </section>
"#
    )
}

fn undecided_section(items: &[Value]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let rows = items
        .iter()
        .map(|item| {
            let ids = item["requirement_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "<tr><td><code>{}</code></td><td>{}</td><td>{}</td></tr>",
                escape(item["declaration"].as_str().unwrap_or_default()),
                escape(item["reason"].as_str().unwrap_or_default()),
                if ids.is_empty() {
                    "—".to_owned()
                } else {
                    escape(&ids)
                },
            )
        })
        .collect::<String>();
    format!(
        r#"
      <section class="section" id="undecided">
        <div class="section-head"><div>
          <h2>Intentional Undecided Decisions</h2>
          <p><code>undecided:</code> declarations are review metadata and are not verification conditions.</p>
        </div></div>
        <div class="panel table-wrap"><table>
          <thead><tr><th>Declaration</th><th>Reason</th><th>Affected requirement IDs</th></tr></thead>
          <tbody>{rows}</tbody>
        </table></div>
      </section>
"#
    )
}

fn params(params: &Value) -> String {
    let Some(params) = params.as_object() else {
        return String::new();
    };
    if params.is_empty() {
        return String::new();
    }
    let rendered = params
        .iter()
        .map(|(name, value)| format!("{name}={}", text(value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("<code>{}</code>", escape(&rendered))
}

fn trace_timeline(trace: &[Value], violated_step: Option<u64>) -> String {
    let steps = trace
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let step = entry["step"].as_u64().unwrap_or(index as u64);
            let action = entry.get("action").and_then(Value::as_object);
            let action_name = action
                .and_then(|action| action.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("initial");
            let rendered_changes = entry["changes"]
                .as_object()
                .map(|changes| {
                    changes
                        .iter()
                        .filter_map(|(name, change)| {
                            let change = change.as_object()?;
                            Some(format!(
                                "<div class=\"change\">{}: {} -> {}</div>",
                                escape(name),
                                escape(&text(change.get("from").unwrap_or(&Value::Null))),
                                escape(&text(change.get("to").unwrap_or(&Value::Null)))
                            ))
                        })
                        .collect::<String>()
                })
                .unwrap_or_default();
            let rendered_changes = if rendered_changes.is_empty() && step == 0 {
                "<div class=\"change\">initial state</div>".to_owned()
            } else {
                rendered_changes
            };
            let is_bad = violated_step == Some(step);
            let bad_class = if is_bad { " bad" } else { "" };
            let bad_badge = if is_bad {
                "<span class=\"badge bad\">violation</span>"
            } else {
                ""
            };
            format!(
                "<div class=\"step{bad_class}\"><div class=\"step-num\">{step}</div><div class=\"step-body\"><strong>{}</strong> {} {bad_badge}<div class=\"changes\">{rendered_changes}</div>{}</div></div>",
                escape(action_name),
                params(action.and_then(|action| action.get("params")).unwrap_or(&Value::Null)),
                details("State", &json_pre(&entry["state"])),
            )
        })
        .collect::<String>();
    format!("<div class=\"timeline\">{steps}</div>")
}

fn relation_graphs(trace: &[Value]) -> String {
    let mut panels = String::new();
    for entry in trace {
        let Some(state) = entry["state"].as_object() else {
            continue;
        };
        for (name, value) in state {
            let Some(edges) = value.as_array().filter(|edges| {
                !edges.is_empty()
                    && edges
                        .iter()
                        .all(|edge| edge.as_array().is_some_and(|edge| edge.len() == 2))
            }) else {
                continue;
            };
            let edges = edges
                .iter()
                .filter_map(Value::as_array)
                .map(|edge| {
                    format!(
                        "<span class=\"edge\">{} -&gt; {}</span>",
                        escape(&text(&edge[0])),
                        escape(&text(&edge[1]))
                    )
                })
                .collect::<String>();
            let _ = write!(
                panels,
                "<div class=\"mini-card\"><h3><span>{}</span><span class=\"badge info\">step {}</span></h3><div class=\"edge-list\">{edges}</div></div>",
                escape(name),
                escape(&text(&entry["step"]))
            );
        }
    }
    if panels.is_empty() {
        String::new()
    } else {
        format!(
            "\n      <div class=\"relation-graphs\">\n        <h3>Relation Graphs</h3>\n        <div class=\"cards\">{panels}</div>\n      </div>\n"
        )
    }
}

fn first_reachable_witness(verification: &Value) -> Option<&Vec<Value>> {
    verification["reachables"]
        .as_object()?
        .values()
        .find_map(|item| item.get("witness")?.as_array())
}

fn trace_section(verification: &Value) -> String {
    let counterexample = verification["trace"].as_array();
    let trace = counterexample.or_else(|| first_reachable_witness(verification));
    let banner = if counterexample.is_some() {
        let name = verification["invariant"].as_str().unwrap_or_default();
        let name = if name.is_empty() {
            String::new()
        } else {
            format!(" <code>{}</code>", escape(name))
        };
        let where_text = verification["violated_at_step"]
            .as_u64()
            .map_or_else(String::new, |step| format!(" at step {step}"));
        format!(
            "<div class=\"callout bad\"><strong>Counterexample</strong> — violates {}{name}{where_text}. The highlighted step below is where it first breaks.</div>",
            escape(
                verification["violation_kind"]
                    .as_str()
                    .unwrap_or("a property")
            )
        )
    } else if trace.is_some() {
        "<div class=\"callout info\"><strong>Representative reachable trace</strong> — no violation; a concrete path the model can take.</div>".to_owned()
    } else {
        String::new()
    };
    let content = trace.map_or_else(
        || "<p class=\"empty\">No counterexample trace was emitted. Reachable witnesses may still appear below.</p>".to_owned(),
        |trace| {
            format!(
                "{}{}",
                trace_timeline(trace, counterexample.and_then(|_| verification["violated_at_step"].as_u64())),
                relation_graphs(trace)
            )
        },
    );
    format!(
        r#"
      <section class="section" id="traces">
        <div class="section-head">
          <div>
            <h2>Trace Review</h2>
            <p>Shortest counterexample or representative reachable trace, step by step.</p>
          </div>
        </div>
        {banner}
        {content}
      </section>
"#
    )
}
fn witness_section(witnesses: &[Value]) -> String {
    let content = if witnesses.is_empty() {
        "<p class=\"empty\">No witnesses emitted at this depth.</p>".to_owned()
    } else {
        let cards = witnesses.iter().map(|witness| {
            let steps = witness["narration"].as_array().cloned().unwrap_or_default();
            let steps = if steps.is_empty() {
                "<p class=\"empty\">No steps.</p>".to_owned()
            } else {
                steps.iter().enumerate().map(|(index, step)| format!("<div class=\"step\"><div class=\"step-num\">{}</div><div class=\"step-body\"><strong>{}</strong></div></div>", index + 1, escape(&text(step)))).collect::<String>()
            };
            let states = json!({
                "initial_state": witness.get("initial_state").cloned().unwrap_or(Value::Null),
                "expected_states": witness.get("expected_states").cloned().unwrap_or(Value::Null),
            });
            format!("<article class=\"mini-card\"><h3><span>{}</span>{}</h3><p><code>{}</code></p><div class=\"timeline\">{steps}</div>{}</article>", escape(&text(&witness["name"])), badge(&witness["kind"]), escape(&text(&witness["target"])), details("State snapshots", &json_pre(&states)))
        }).collect::<String>();
        format!("<div class=\"cards\">{cards}</div>")
    };
    format!(
        r#"
      <section class="section" id="witnesses">
        <div class="section-head">
          <div>
            <h2>Witnesses</h2>
            <p>Concrete examples that exercise reachable targets and action coverage.</p>
          </div>
        </div>
        {content}
      </section>
"#
    )
}
fn requirement_caption(requirement: &Value) -> String {
    let Some(requirement) = requirement.as_object() else {
        return String::new();
    };
    let id = requirement.get("id").and_then(Value::as_str);
    let text = requirement.get("text").and_then(Value::as_str);
    let label = match (id, text) {
        (Some(id), Some(text)) => format!("{id}: {text}"),
        (Some(id), None) => id.to_owned(),
        (None, Some(text)) => text.to_owned(),
        (None, None) => return String::new(),
    };
    format!("<div class=\"req-caption\">{}</div>", escape(&label))
}

fn counterfactual_section(counterfactuals: &[Value]) -> String {
    let content = if counterfactuals.is_empty() {
        "<p class=\"empty\">No counterfactuals were generated for this spec.</p>".to_owned()
    } else {
        let rows = counterfactuals
            .iter()
            .map(|item| {
                let weakening = item["weakening"].as_object();
                let weak_text = weakening.map_or_else(
                    || "none".to_owned(),
                    |weakening| {
                        format!(
                            "{}: {}<br><code>{}</code>",
                            escape(&text(weakening.get("op").unwrap_or(&Value::Null))),
                            escape(&text(weakening.get("target").unwrap_or(&Value::Null))),
                            escape(&text(weakening.get("source_text").unwrap_or(&Value::Null)))
                        )
                    },
                );
                let evidence = json!({
                    "trace": item.get("trace").cloned().unwrap_or(Value::Null),
                    "violation": item.get("violation").cloned().unwrap_or(Value::Null),
                    "note": item.get("note").cloned().unwrap_or(Value::Null),
                });
                format!(
                    "<tr><td><code>{}</code>{}</td><td>{weak_text}</td><td>{}</td></tr>",
                    escape(&text(&item["invariant"])),
                    requirement_caption(&item["requirement"]),
                    details("Trace / violation", &json_pre(&evidence))
                )
            })
            .collect::<String>();
        format!(
            "\n          <div class=\"panel table-wrap\">\n            <table>\n              <thead><tr><th>Invariant</th><th>Weakening</th><th>Evidence</th></tr></thead>\n              <tbody>{rows}</tbody>\n            </table>\n          </div>\n"
        )
    };
    format!(
        r#"
      <section class="section" id="counterfactuals">
        <div class="section-head">
          <div>
            <h2>Counterfactuals</h2>
            <p>What would break if a rule or transition detail were removed.</p>
          </div>
        </div>
        {content}
      </section>
"#
    )
}
fn source_section(source: &str) -> String {
    let rows = source
        .lines()
        .enumerate()
        .map(|(index, line)| format!("<tr><td>{}</td><td>{}</td></tr>", index + 1, escape(line)))
        .collect::<String>();
    format!(
        r#"
      <section class="section" id="source">
        <div class="section-head">
          <div>
            <h2>Source</h2>
            <p>Original FSL with stable line numbers for review comments.</p>
          </div>
        </div>
        <div class="panel table-wrap">
          <table class="source-table">
            <tbody>{rows}</tbody>
          </table>
        </div>
      </section>
"#
    )
}
fn raw_section(explained: &Value, verification: &Value) -> String {
    format!(
        r#"
      <section class="section" id="raw-data">
        <div class="section-head">
          <div>
            <h2>Raw Data</h2>
            <p>Original JSON payloads used to build this report.</p>
          </div>
        </div>
        {}
        {}
      </section>
"#,
        details("explain JSON", &json_pre(explained)),
        details("verify JSON", &json_pre(verification))
    )
}

/// Render a self-contained HTML report from native explain and verify payloads.
#[must_use]
pub fn render_html_report(
    file: &str,
    source: &str,
    explained: &Value,
    verification: &Value,
    undecided: &[Value],
) -> String {
    let skeleton = &explained["skeleton"];
    let spec = explained["spec"]
        .as_str()
        .or_else(|| verification["spec"].as_str())
        .unwrap_or("spec");
    let state = skeleton["state"]
        .as_object()
        .map_or(0, serde_json::Map::len);
    let actions = skeleton["actions"].as_array().cloned().unwrap_or_default();
    let properties = skeleton["properties"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let checks = skeleton["auto_checks"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let domains = skeleton["domains"].as_array().cloned().unwrap_or_default();
    let coverage = &verification["action_coverage"];
    let covered = coverage.as_object().map_or(0, |values| {
        values
            .values()
            .filter(|value| value.as_bool() == Some(true))
            .count()
    });
    let coverage_label = coverage.as_object().map_or_else(
        || "n/a".to_owned(),
        |values| format!("{covered}/{}", values.len()),
    );
    let status = verification["result"].as_str().unwrap_or("unknown");
    let warnings = verification["warnings"].as_array().map_or(0, Vec::len);
    let mut sections = vec![
        hero(
            spec,
            file,
            &explained["depth"],
            status,
            state,
            actions.len(),
            properties.len(),
            &coverage_label,
            warnings,
            domains.len(),
            0,
        ),
        model_section(
            &skeleton["state"],
            &actions,
            &skeleton["enums"],
            &domains,
            skeleton["stage_flows"]
                .as_array()
                .map_or(&[], Vec::as_slice),
        ),
        actions_section(&actions, coverage),
        properties_section(&properties, &checks, verification),
    ];
    if !undecided.is_empty() {
        sections.push(undecided_section(undecided));
    }
    sections.extend([
        status_section(verification),
        String::new(),
        trace_section(verification),
        witness_section(explained["witnesses"].as_array().map_or(&[], Vec::as_slice)),
        counterfactual_section(
            explained["counterfactuals"]
                .as_array()
                .map_or(&[], Vec::as_slice),
        ),
        source_section(source),
        raw_section(explained, verification),
    ]);
    let body = sections.join("\n");
    [
        "<!doctype html>".to_owned(),
        "<html lang=\"en\">".to_owned(),
        "<head>".to_owned(),
        "  <meta charset=\"utf-8\">".to_owned(),
        "  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">".to_owned(),
        format!(
            "  <title>{} - FSL Specification Report</title>",
            escape(spec)
        ),
        "  <style>".to_owned(),
        format!("\n{}", include_str!("report.css")),
        "  </style>".to_owned(),
        "</head>".to_owned(),
        "<body>".to_owned(),
        "  <div class=\"shell\">".to_owned(),
        sidebar(spec),
        "    <main class=\"main\" id=\"top\">".to_owned(),
        body,
        "    </main>".to_owned(),
        "  </div>".to_owned(),
        "</body>".to_owned(),
        "</html>".to_owned(),
    ]
    .join("\n")
}
