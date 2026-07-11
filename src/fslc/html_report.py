# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Self-contained HTML report rendering for FSL specs."""
from __future__ import annotations

import json
from html import escape
from pathlib import Path

from .assurance import assurance_label, classify_element, classify_result


def default_output_name(file: str) -> str:
    return str(Path(file).with_suffix(".html"))


def render_html_report(file: str, source: str, explained: dict, verification: dict) -> str:
    skeleton = explained.get("skeleton") or {}
    spec = explained.get("spec") or verification.get("spec") or Path(file).stem
    depth = explained.get("depth")
    title = f"{spec} - FSL Specification Report"
    status = verification.get("result", "unknown")
    state = skeleton.get("state") or {}
    enums = skeleton.get("enums") or {}
    domains = skeleton.get("domains") or []
    kpis = skeleton.get("kpis") or []
    stage_flows = skeleton.get("stage_flows") or []
    kind = skeleton.get("spec_kind")

    all_actions = skeleton.get("actions") or []
    actions = [a for a in all_actions if not a.get("generated")]
    generated_actions = [a for a in all_actions if a.get("generated")]

    all_properties = skeleton.get("properties") or []
    properties = [p for p in all_properties if not p.get("generated")]
    generated_properties = [p for p in all_properties if p.get("generated")]

    auto_checks = list(skeleton.get("auto_checks") or [])
    auto_checks.extend(_generated_action_check(a) for a in generated_actions)
    auto_checks.extend(_generated_property_check(p) for p in generated_properties)

    witnesses = explained.get("witnesses") or []
    counterfactuals = explained.get("counterfactuals") or []
    warnings = verification.get("warnings") or []

    coverage = verification.get("action_coverage") or {}
    covered = sum(1 for ok in coverage.values() if ok)
    coverage_label = f"{covered}/{len(coverage)}" if coverage else "n/a"

    subtitle = _hero_subtitle(len(state), len(actions), len(properties), domains, kpis)

    body = "\n".join([
        _hero(spec, file, depth, status, len(state), len(actions), len(properties), coverage_label, warnings, subtitle, kind),
        _model_section(state, actions, enums, domains, kpis, stage_flows),
        _undecided_section(skeleton.get("undecided") or []),
        _actions_section(actions, coverage),
        _properties_section(properties, auto_checks, verification),
        _status_section(verification),
        _refinement_section(verification),
        _trace_section(verification),
        _witness_section(witnesses),
        _counterfactual_section(counterfactuals),
        _source_section(source),
        _raw_data_section(explained, verification),
    ])

    return "\n".join([
        "<!doctype html>",
        '<html lang="en">',
        "<head>",
        '  <meta charset="utf-8">',
        '  <meta name="viewport" content="width=device-width, initial-scale=1">',
        f"  <title>{escape(title)}</title>",
        "  <style>",
        _css(),
        "  </style>",
        "</head>",
        "<body>",
        '  <div class="shell">',
        _sidebar(spec),
        '    <main class="main" id="top">',
        body,
        "    </main>",
        "  </div>",
        "</body>",
        "</html>",
    ])


def _css() -> str:
    return r"""
:root {
  --bg: #f6f7f3;
  --ink: #171a16;
  --muted: #62685f;
  --line: #d9ddd2;
  --surface: #ffffff;
  --surface-2: #eef2e9;
  --teal: #087f7a;
  --teal-2: #d8f0eb;
  --amber: #b7791f;
  --amber-2: #fff2d2;
  --green: #237a3b;
  --green-2: #dff3e3;
  --red: #b83232;
  --red-2: #ffe1de;
  --violet: #6554c0;
  --violet-2: #ebe7ff;
  --mono: ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace;
  --sans: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}
* { box-sizing: border-box; }
html { scroll-behavior: smooth; }
@media (prefers-reduced-motion: reduce) { html { scroll-behavior: auto; } }
body {
  margin: 0;
  background:
    linear-gradient(180deg, rgba(8, 127, 122, 0.08), transparent 360px),
    var(--bg);
  color: var(--ink);
  font-family: var(--sans);
  font-size: 16px;
  line-height: 1.55;
  letter-spacing: 0;
}
a { color: inherit; }
.shell {
  display: grid;
  grid-template-columns: 236px minmax(0, 1fr);
  min-height: 100vh;
}
.side {
  position: sticky;
  top: 0;
  height: 100vh;
  padding: 24px 20px;
  border-right: 1px solid var(--line);
  background: rgba(246, 247, 243, 0.92);
  backdrop-filter: blur(14px);
}
.mark {
  display: flex;
  align-items: center;
  gap: 10px;
  font-weight: 760;
  font-size: 14px;
  margin-bottom: 24px;
}
.mark span:last-child { overflow-wrap: anywhere; }
.mark-dot {
  width: 24px;
  height: 24px;
  border-radius: 7px;
  background: linear-gradient(135deg, var(--teal), #63b27e);
}
.side nav {
  display: grid;
  gap: 6px;
}
.side a {
  min-height: 36px;
  display: flex;
  align-items: center;
  padding: 8px 10px;
  border-radius: 8px;
  color: var(--muted);
  text-decoration: none;
  font-size: 14px;
}
.side a:hover, .side a:focus-visible {
  background: var(--surface-2);
  color: var(--ink);
  outline: none;
}
.main {
  min-width: 0;
  padding: 32px;
}
.hero {
  display: grid;
  align-items: end;
  gap: 18px;
  padding: 26px 32px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background:
    linear-gradient(135deg, rgba(255,255,255,0.96), rgba(255,255,255,0.72)),
    repeating-linear-gradient(90deg, rgba(8,127,122,0.12) 0, rgba(8,127,122,0.12) 1px, transparent 1px, transparent 38px);
}
.eyebrow {
  margin: 0 0 10px;
  color: var(--teal);
  font: 700 12px/1.2 var(--mono);
  text-transform: uppercase;
}
h1, h2, h3 {
  margin: 0;
  letter-spacing: 0;
  line-height: 1.14;
}
h1 {
  max-width: 940px;
  font-size: 42px;
  overflow-wrap: anywhere;
}
h2 { font-size: 26px; }
h3 { font-size: 16px; }
.sub {
  max-width: 760px;
  margin: 16px 0 0;
  color: var(--muted);
  font-size: 17px;
}
.meta-grid {
  display: grid;
  grid-template-columns: repeat(6, minmax(0, 1fr));
  gap: 10px;
}
.metric {
  min-height: 86px;
  padding: 14px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(255,255,255,0.78);
}
.metric span {
  display: block;
  color: var(--muted);
  font: 700 11px/1.2 var(--mono);
  text-transform: uppercase;
}
.metric strong {
  display: block;
  margin-top: 9px;
  font-size: 22px;
  overflow-wrap: anywhere;
}
.section {
  margin-top: 24px;
  padding: 28px 0 0;
  border-top: 1px solid var(--line);
}
.section-head {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 18px;
}
.section-head p {
  margin: 7px 0 0;
  color: var(--muted);
}
.badge {
  display: inline-flex;
  align-items: center;
  min-height: 26px;
  padding: 4px 9px;
  border-radius: 999px;
  border: 1px solid transparent;
  font: 700 12px/1 var(--mono);
  white-space: nowrap;
}
.badge.ok { color: var(--green); background: var(--green-2); border-color: #b9dfbf; }
.badge.warn { color: var(--amber); background: var(--amber-2); border-color: #efd39a; }
.badge.bad { color: var(--red); background: var(--red-2); border-color: #f1b2ac; }
.badge.info { color: var(--teal); background: var(--teal-2); border-color: #afdcd7; }
.badge.neutral { color: #4b5563; background: #eef0ec; border-color: #d7dbd2; }
.grid-2 {
  display: grid;
  grid-template-columns: minmax(0, 0.9fr) minmax(0, 1.1fr);
  gap: 18px;
}
.stack {
  display: grid;
  gap: 18px;
}
.panel {
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--surface);
  overflow: hidden;
}
.panel-pad { padding: 18px; }
.table-wrap { overflow-x: auto; }
table {
  width: 100%;
  border-collapse: collapse;
  font-size: 14px;
}
th, td {
  padding: 12px 14px;
  border-bottom: 1px solid var(--line);
  text-align: left;
  vertical-align: top;
  overflow-wrap: break-word;
}
th {
  color: var(--muted);
  background: #fafbf8;
  font: 700 12px/1.3 var(--mono);
  text-transform: uppercase;
  white-space: nowrap;
}
tr:last-child td { border-bottom: 0; }
code, pre {
  font-family: var(--mono);
  letter-spacing: 0;
}
code {
  padding: 2px 5px;
  border-radius: 5px;
  background: #eef0ec;
  font-size: 0.92em;
}
pre {
  margin: 0;
  overflow-x: auto;
  white-space: pre;
  font-size: 12.5px;
  line-height: 1.55;
}
.code-block {
  border: 1px solid var(--line);
  border-radius: 8px;
  background: #10130f;
  color: #eef5e8;
}
.code-block pre { padding: 16px; }
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.chip {
  display: inline-flex;
  align-items: center;
  max-width: 100%;
  min-height: 28px;
  padding: 4px 8px;
  border-radius: 7px;
  background: #eef0ec;
  color: #363b34;
  font: 700 12px/1.2 var(--mono);
  overflow-wrap: anywhere;
}
.chip.write { background: var(--teal-2); color: var(--teal); }
.chip.require { background: var(--amber-2); color: var(--amber); }
.chip.fair { background: var(--violet-2); color: var(--violet); }
.flow-svg {
  width: 100%;
  min-height: 320px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: linear-gradient(180deg, #ffffff, #f8faf5);
}
.flow-svg text {
  fill: var(--ink);
  font-family: var(--sans);
  font-size: 13px;
}
.flow-svg .small { fill: var(--muted); font-size: 11px; font-family: var(--mono); }
.timeline {
  display: grid;
  gap: 10px;
}
.step {
  display: grid;
  grid-template-columns: 44px minmax(0, 1fr);
  gap: 12px;
  align-items: start;
}
.step-num {
  width: 36px;
  height: 36px;
  display: grid;
  place-items: center;
  border-radius: 8px;
  background: var(--teal);
  color: white;
  font: 800 13px/1 var(--mono);
}
.step-body {
  min-width: 0;
  padding: 10px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: #fbfcf8;
}
.step-body strong { font-family: var(--mono); font-size: 13px; }
.changes {
  margin-top: 8px;
  display: grid;
  gap: 5px;
}
.change {
  color: var(--muted);
  font: 12px/1.45 var(--mono);
}
.relation-graphs {
  margin-top: 14px;
  display: grid;
  gap: 12px;
}
.edge-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 10px;
}
.edge {
  display: inline-flex;
  align-items: center;
  min-height: 28px;
  padding: 4px 8px;
  border-radius: 7px;
  background: var(--teal-2);
  color: var(--teal);
  font: 700 12px/1.2 var(--mono);
}
.callout {
  margin-bottom: 16px;
  padding: 12px 14px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--surface);
  font-size: 14px;
}
.callout.bad { border-color: #f1b2ac; background: var(--red-2); color: #7f2620; }
.callout.info { border-color: #afdcd7; background: var(--teal-2); color: #0a5d59; }
.callout-sub { margin-top: 6px; font: 12px/1.45 var(--mono); }
.step.bad .step-num { background: var(--red); }
.step.bad .step-body { border-color: #f1b2ac; background: var(--red-2); }
.cards {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
}
.mini-card {
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--surface);
  padding: 16px;
}
.mini-card h3 {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  margin-bottom: 12px;
}
details {
  border: 1px solid var(--line);
  border-radius: 8px;
  background: #fbfcf8;
}
summary {
  min-height: 42px;
  padding: 10px 12px;
  cursor: pointer;
  color: var(--muted);
  font: 700 13px/1.35 var(--mono);
}
details > div { padding: 0 12px 12px; }
.source-table td:first-child {
  width: 1%;
  color: #858b80;
  text-align: right;
  user-select: none;
  font-family: var(--mono);
}
.source-table td:last-child {
  font-family: var(--mono);
  white-space: pre;
}
.empty {
  padding: 18px;
  border: 1px dashed var(--line);
  border-radius: 8px;
  color: var(--muted);
  background: #fbfcf8;
}
.req-caption {
  margin-top: 4px;
  color: var(--muted);
  font-size: 12.5px;
  font-weight: 400;
  font-family: var(--sans);
  overflow-wrap: anywhere;
}
.panel-pad ul {
  margin: 0;
  padding-left: 20px;
}
.panel-pad h3 {
  margin-bottom: 8px;
}
@media (max-width: 960px) {
  .shell { grid-template-columns: 1fr; }
  .side {
    position: static;
    height: auto;
    border-right: 0;
    border-bottom: 1px solid var(--line);
  }
  .side nav { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .main { padding: 18px; }
  .hero { padding: 26px; }
  h1 { font-size: 40px; }
  .meta-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .grid-2, .cards { grid-template-columns: 1fr; }
}
@media (max-width: 560px) {
  .side nav { grid-template-columns: 1fr; }
  .meta-grid { grid-template-columns: 1fr; }
  .section { padding-top: 22px; }
  h1 { font-size: 34px; }
}
"""


def _sidebar(spec: str) -> str:
    return f"""
    <aside class="side" aria-label="Report navigation">
      <div class="mark"><span class="mark-dot" aria-hidden="true"></span><span>{escape(spec)}</span></div>
      <nav>
        <a href="#model">Model</a>
        <a href="#undecided">Undecided</a>
        <a href="#actions">Actions</a>
        <a href="#properties">Properties</a>
        <a href="#status">Status</a>
        <a href="#traces">Traces</a>
        <a href="#witnesses">Witnesses</a>
        <a href="#counterfactuals">Counterfactuals</a>
        <a href="#source">Source</a>
      </nav>
    </aside>
"""


def _hero_subtitle(states: int, actions: int, properties: int, domains: list, kpis: list) -> str:
    prop_word = "property" if properties == 1 else "properties"
    action_word = "action" if actions == 1 else "actions"
    state_word = "state variable" if states == 1 else "state variables"
    text = f"A model of {states} {state_word}, {actions} {action_word}, and {properties} {prop_word}."
    extras = []
    if domains:
        extras.append(f"{len(domains)} entity/domain declaration(s)")
    if kpis:
        extras.append(f"{len(kpis)} KPI(s)")
    if extras:
        text += " Tracks " + " and ".join(extras) + "."
    return text


def _generated_action_check(action: dict) -> dict:
    gen = action.get("generated")
    text = "generated by verifier"
    if isinstance(gen, dict) and gen.get("kind") == "time_tick":
        text = "generated by verifier (time-tick action)"
    return {"kind": "generated_action", "name": action.get("name"), "target": action.get("name"), "text": text}


def _generated_property_check(prop: dict) -> dict:
    return {
        "kind": prop.get("kind") or "generated",
        "name": prop.get("name"),
        "target": prop.get("name"),
        "text": prop.get("body_text") or "generated by verifier",
    }


def _hero(spec, file, depth, status, states, actions, properties, coverage, warnings, subtitle, kind=None) -> str:
    status_class = _status_class(status)
    return f"""
      <header class="hero">
        <div>
          <p class="eyebrow">FSL specification report</p>
          <h1>{escape(spec)}{_kind_badge(kind)}</h1>
          <p class="sub">{escape(subtitle)}</p>
        </div>
        <div class="meta-grid" aria-label="Report summary">
          {_metric("Result", f'<span class="badge {status_class}">{escape(str(status))}</span>')}
          {_metric("Depth", escape(str(depth)))}
          {_metric("States", escape(str(states)))}
          {_metric("Actions", escape(str(actions)))}
          {_metric("Properties", escape(str(properties)))}
          {_metric("Coverage", escape(str(coverage)))}
        </div>
        <div class="chips">
          <span class="chip">{escape(file)}</span>
          <span class="chip">{len(warnings)} warning(s)</span>
        </div>
      </header>
"""


def _kind_badge(kind) -> str:
    """A neutral pill next to the spec title classifying the whole spec (e.g. `ui`)."""
    if not kind:
        return ""
    label = escape(str(kind.get("id", "")))
    text = kind.get("text")
    title = f' title="{escape(str(text))}"' if text else ""
    return f' <span class="badge neutral kind"{title}>{label}</span>'


def _metric(label, value) -> str:
    return f'<div class="metric"><span>{escape(label)}</span><strong>{value}</strong></div>'


def _status_section(verification: dict) -> str:
    assurance = assurance_label(
        classify_result(verification),
        depth=verification.get("checked_to_depth", verification.get("depth")),
    )
    rows = [
        ("Result", _badge(verification.get("result", "unknown"))),
        ("Assurance", escape(assurance)),
        ("Completeness", escape(str(verification.get("completeness", "n/a")))),
        ("Checked depth", escape(str(verification.get("checked_to_depth", verification.get("depth", "n/a"))))),
        ("Note", escape(str(verification.get("note", "")))),
        ("Hint", escape(str(verification.get("hint", "")))),
    ]
    if verification.get("implements"):
        rows.append(("Implements", _json_code(verification["implements"])))
    if verification.get("deadlock"):
        rows.append(("Deadlock", _json_code(verification["deadlock"])))
    rows.extend(_violation_rows(verification))
    warning_html = _warnings(verification.get("warnings") or [])
    return f"""
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
              <tbody>{''.join(f'<tr><th>{escape(k)}</th><td>{v}</td></tr>' for k, v in rows if v)}</tbody>
            </table>
          </div>
          {warning_html}
        </div>
      </section>
"""


def _refinement_violation(verification: dict):
    if verification.get("result") == "refinement_failed":
        return verification
    impl = verification.get("implements")
    if isinstance(impl, dict) and impl.get("result") == "refinement_failed":
        return impl.get("violation") or impl
    return None


def _refinement_section(verification: dict) -> str:
    violation = _refinement_violation(verification)
    if not violation:
        return ""
    impl_action = violation.get("impl_action") or violation.get("action") or {}
    abs_action = violation.get("abs_action") or violation.get("abstract_action") or {}
    edge = ""
    if impl_action or abs_action:
        impl_name = impl_action.get("name") if isinstance(impl_action, dict) else impl_action
        abs_name = abs_action.get("name") if isinstance(abs_action, dict) else abs_action
        edge = (
            '<div class="callout bad">'
            f"<strong>Action correspondence</strong> "
            f"<code>{escape(str(impl_name or 'impl step'))}</code> -&gt; "
            f"<code>{escape(str(abs_name or 'abstract step'))}</code>"
            "</div>"
        )
    impl_payload = _drop_empty({
        "impl": violation.get("impl"),
        "action": impl_action,
        "state": violation.get("impl_state"),
        "trace": violation.get("impl_trace"),
    })
    abs_payload = _drop_empty({
        "abs": violation.get("abs"),
        "action": abs_action,
        "alpha_before": violation.get("alpha_before"),
        "alpha_after_expected": violation.get("alpha_after_expected"),
        "alpha_after_actual": violation.get("alpha_after_actual"),
        "mismatch": violation.get("mismatch"),
    })
    return f"""
      <section class="section" id="refinement">
        <div class="section-head">
          <div>
            <h2>Refinement Evidence</h2>
            <p>Side-by-side implementation and abstract states for the refinement failure.</p>
          </div>
        </div>
        {edge}
        <div class="grid-2">
          <div class="panel panel-pad">
            <h3>Implementation Side</h3>
            {_json_pre(impl_payload)}
          </div>
          <div class="panel panel-pad">
            <h3>Abstract Side</h3>
            {_json_pre(abs_payload)}
          </div>
        </div>
      </section>
"""


def _warnings(warnings) -> str:
    if not warnings:
        return '<div class="panel panel-pad"><h3>Warnings</h3><p class="empty">No warnings reported.</p></div>'
    items = []
    for warning in warnings:
        kind = warning.get("kind", "warning") if isinstance(warning, dict) else "warning"
        msg = warning.get("message", warning) if isinstance(warning, dict) else warning
        items.append(f'<tr><td>{escape(str(kind))}</td><td>{escape(str(msg))}</td></tr>')
    return f"""
      <div class="panel table-wrap">
        <table>
          <thead><tr><th>Kind</th><th>Message</th></tr></thead>
          <tbody>{''.join(items)}</tbody>
        </table>
      </div>
"""


_VIOLATION_KIND_LABELS = {
    "type_bound": "type bound",
    "invariant": "invariant",
    "leadsTo": "response (leadsTo)",
    "leadsTo_rank": "response ranking (leadsTo)",
    "ensures": "postcondition (ensures)",
    "trans": "transition guard",
    "partial_op": "partial-operation guard",
    "deadlock": "deadlock",
}

_VIOLATION_FIELDS = [
    ("violation_kind", "Violation kind"),
    ("invariant", "Violated property"),
    ("violated_at_step", "Violated at step"),
    ("pending_since", "Pending since step"),
    ("deadline", "Deadline"),
    ("within", "Within"),
]


def _violated_name(verification: dict) -> str:
    name = str(verification.get("invariant") or "")
    if name.startswith("_bounds_"):
        return name[len("_bounds_"):]
    return name


def _bindings_inline(binds) -> str:
    if isinstance(binds, dict):
        binds = [binds]
    parts = []
    for b in binds or []:
        if isinstance(b, dict):
            parts.append(", ".join(f"{k}={v}" for k, v in b.items()))
        else:
            parts.append(str(b))
    return f"<code>{escape('; '.join(parts))}</code>"


def _violation_rows(verification: dict) -> list:
    if not verification.get("violation_kind"):
        return []
    rows = []
    for key, label in _VIOLATION_FIELDS:
        val = verification.get(key)
        if val is None:
            continue
        if key == "violation_kind":
            val = _VIOLATION_KIND_LABELS.get(val, val)
        elif key == "invariant":
            val = _violated_name(verification)
        rows.append((label, f"<code>{escape(str(val))}</code>"))
    binds = verification.get("violating_bindings") or verification.get("bindings")
    if binds:
        rows.append(("Violating binding", _bindings_inline(binds)))
    return rows


def _model_section(state: dict, actions: list, enums: dict, domains: list, kpis: list, stage_flows: list) -> str:
    rows = "".join(
        f"<tr><td><code>{escape(name)}</code></td><td>{escape(_type_text(ty, enums))}</td></tr>"
        for name, ty in sorted(state.items())
    )
    extra = "".join(filter(None, [
        _domains_panel(domains),
        _kpis_panel(kpis),
        _stage_flow_panel(stage_flows),
    ]))
    return f"""
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
              <tbody>{rows or '<tr><td colspan="2">No state variables.</td></tr>'}</tbody>
            </table>
          </div>
          <div>{_influence_svg(list(sorted(state)), actions)}</div>
        </div>
        {extra}
      </section>
"""


def _undecided_section(entries: list) -> str:
    if entries:
        rows = "".join(
            "<tr>"
            f"<td>{escape(str(item.get('kind', '')))} <code>{escape(str(item.get('name', '')))}</code></td>"
            f"<td>{escape(str(item.get('text', '')))}</td>"
            f"<td>{escape(', '.join(item.get('requirements') or []) or 'spec-wide')}</td>"
            "<td>metadata only; findings remain visible</td>"
            "</tr>"
            for item in entries
        )
    else:
        rows = '<tr><td colspan="4">No intentional undecision markers.</td></tr>'
    return f"""
      <section class="section" id="undecided">
        <div class="section-head"><div>
          <h2>Undecided Items</h2>
          <p>Intentional decision deferrals. These markers do not change verification semantics.</p>
        </div></div>
        <div class="panel table-wrap"><table>
          <thead><tr><th>Declaration</th><th>Open decision</th><th>Requirements</th><th>Semantics</th></tr></thead>
          <tbody>{rows}</tbody>
        </table></div>
      </section>
"""


def _domains_panel(domains: list) -> str:
    if not domains:
        return ""
    items = "".join(f"<li>{escape(d)}</li>" for d in domains)
    return f'<div class="panel panel-pad"><h3>Entities &amp; Domains</h3><ul>{items}</ul></div>'


def _kpis_panel(kpis: list) -> str:
    if not kpis:
        return ""
    rows = "".join(
        "<tr>"
        f"<td><code>{escape(str(k.get('name', '')))}</code></td>"
        f"<td>count of {escape(str(k.get('entity', '')))} in {escape(str(k.get('stage', '')))}</td>"
        "</tr>"
        for k in kpis
    )
    return f"""
      <div class="panel table-wrap">
        <table>
          <thead><tr><th>KPI</th><th>Definition</th></tr></thead>
          <tbody>{rows}</tbody>
        </table>
      </div>
"""


def _stage_flow_panel(stage_flows: list) -> str:
    if not stage_flows:
        return ""
    blocks = []
    for flow in stage_flows:
        transitions = flow.get("transitions", [])
        has_actor = any(t.get("actor") for t in transitions)
        stage_chips = "".join(f'<span class="chip">{escape(s)}</span>' for s in flow.get("stages", []))
        tr_rows = "".join(
            "<tr>"
            f"<td>{escape(str(t.get('from', '')))}</td><td>{escape(str(t.get('to', '')))}</td>"
            f"<td><code>{escape(str(t.get('action', '')))}</code></td>"
            + (f"<td>{escape(str(t.get('actor', '')))}</td>" if has_actor else "")
            + "</tr>"
            for t in transitions
        )
        actor_header = "<th>Actor</th>" if has_actor else ""
        blocks.append(f"""
          <div class="panel table-wrap">
            <div class="panel-pad">
              <h3>{escape(str(flow.get('state', '')))}: {escape(str(flow.get('type', '')))} stages</h3>
              <div class="chips">{stage_chips}</div>
            </div>
            <table>
              <thead><tr><th>From</th><th>To</th><th>Action</th>{actor_header}</tr></thead>
              <tbody>{tr_rows}</tbody>
            </table>
          </div>
""")
    return "".join(blocks)


def _actions_section(actions: list, coverage: dict) -> str:
    has_actor = any(action.get("actor") for action in actions)
    rows = []
    for action in actions:
        name = action.get("name", "")
        params = ", ".join(
            f"{p.get('name')}: {p.get('type')}" for p in action.get("params", [])
        )
        requires = _chip_list(_strip_lead(action.get("requires_text"), "requires "), "require")
        writes = _chip_list(action.get("writes"), "write")
        ensures_items = _strip_lead(action.get("ensures_text"), "ensures ")
        ensures = _chip_list(ensures_items) if ensures_items else ""
        markers = []
        if action.get("fair"):
            markers.append('<span class="chip fair">fair</span>')
        if name in coverage:
            markers.append(_badge("covered" if coverage[name] else "uncovered"))
        params_html = escape(params) if params else '<span class="chip">none</span>'
        requirement = action.get("requirement")
        if has_actor and _is_actor_only_requirement(requirement):
            requirement = None  # the Actor column already owns "by <actor>"
        name_cell = f"<code>{escape(name)}</code>{_requirement_caption(requirement)}"
        actor_cell = f"<td>{escape(str(action.get('actor') or ''))}</td>" if has_actor else ""
        rows.append(
            "<tr>"
            f"<td>{name_cell}<br>{''.join(markers)}</td>"
            f"<td>{params_html}</td>"
            f"{actor_cell}"
            f"<td>{requires}</td><td>{writes}</td><td>{ensures}</td>"
            "</tr>"
        )
    actor_header = "<th>Actor</th>" if has_actor else ""
    colspan = 6 if has_actor else 5
    return f"""
      <section class="section" id="actions">
        <div class="section-head">
          <div>
            <h2>Actions</h2>
            <p>Preconditions, writes, postconditions, and coverage.</p>
          </div>
        </div>
        <div class="panel table-wrap">
          <table>
            <thead><tr><th>Action</th><th>Params</th>{actor_header}<th>Requires</th><th>Writes</th><th>Ensures</th></tr></thead>
            <tbody>{''.join(rows) or f'<tr><td colspan="{colspan}">No actions.</td></tr>'}</tbody>
          </table>
        </div>
      </section>
"""


_PROPERTY_KIND_TO_GROUP = {
    "invariant": "invariants",
    "trans": "transitions",
    "leadsTo": "leadstos",
    "reachable": "reachables",
}


def _properties_section(properties: list, auto_checks: list, verification: dict | None = None) -> str:
    has_deadline = any(p.get("within") is not None for p in properties)
    prop_rows = []
    for prop in properties:
        within = prop.get("within")
        within_cell = (
            f'<td><span class="chip fair">within {escape(str(within))}</span></td>'
            if within is not None else "<td></td>"
        ) if has_deadline else ""
        name_cell = f"<code>{escape(str(prop.get('name', '')))}</code>{_requirement_caption(prop.get('requirement'))}"
        group = _PROPERTY_KIND_TO_GROUP.get(prop.get("kind"))
        assurance_cell = (
            f"<td>{escape(assurance_label(classify_element(group, prop.get('name'), verification), depth=verification.get('checked_to_depth', verification.get('depth'))))}</td>"
            if verification is not None and group else "<td></td>"
        )
        prop_rows.append(
            "<tr>"
            f"<td>{escape(str(prop.get('kind', '')))}</td>"
            f"<td>{name_cell}</td>"
            f"{within_cell}"
            f"{assurance_cell}"
            f"<td>{escape(str(prop.get('body_text', '')))}</td>"
            "</tr>"
        )
    deadline_header = "<th>Deadline</th>" if has_deadline else ""
    assurance_header = "<th>Assurance</th>" if verification is not None else ""
    prop_colspan = 3 + (1 if has_deadline else 0) + (1 if verification is not None else 0)
    check_rows = []
    for check in auto_checks:
        label = check.get("target") or check.get("action") or check.get("name")
        check_rows.append(
            "<tr>"
            f"<td>{escape(str(check.get('kind', '')))}</td>"
            f"<td><code>{escape(str(label or ''))}</code></td>"
            f"<td>{escape(str(check.get('text', 'implicit bounded-domain check')))}</td>"
            "</tr>"
        )
    return f"""
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
              <thead><tr><th>Kind</th><th>Name</th>{deadline_header}{assurance_header}<th>Body</th></tr></thead>
              <tbody>{''.join(prop_rows) or f'<tr><td colspan="{prop_colspan}">No user properties.</td></tr>'}</tbody>
            </table>
          </div>
          <div class="panel table-wrap">
            <table>
              <thead><tr><th>Check</th><th>Target</th><th>Source</th></tr></thead>
              <tbody>{''.join(check_rows) or '<tr><td colspan="3">No automatic checks.</td></tr>'}</tbody>
            </table>
          </div>
        </div>
      </section>
"""


def _trace_section(verification: dict) -> str:
    is_counterexample = bool(verification.get("trace"))
    trace = verification.get("trace") or _first_reachable_witness(verification)
    banner = _trace_banner(verification, is_counterexample, trace)
    if not trace:
        content = '<p class="empty">No counterexample trace was emitted. Reachable witnesses may still appear below.</p>'
    else:
        violated_step = verification.get("violated_at_step") if is_counterexample else None
        content = _trace_timeline(trace, violated_step) + _relation_graphs(trace)
    return f"""
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
"""


def _trace_banner(verification: dict, is_counterexample: bool, trace) -> str:
    if is_counterexample:
        kind = _VIOLATION_KIND_LABELS.get(
            verification.get("violation_kind"), verification.get("violation_kind") or "a property"
        )
        name = _violated_name(verification)
        name_html = f" <code>{escape(name)}</code>" if name else ""
        if verification.get("violated_at_step") is not None:
            where = f" at step {escape(str(verification['violated_at_step']))}"
        elif verification.get("pending_since") is not None:
            where = f", pending since step {escape(str(verification['pending_since']))}"
        else:
            where = ""
        binds = verification.get("violating_bindings") or verification.get("bindings")
        bind_html = f'<div class="callout-sub">binding {_bindings_inline(binds)}</div>' if binds else ""
        return (
            '<div class="callout bad">'
            f"<strong>Counterexample</strong> — violates {escape(kind)}{name_html}{where}. "
            "The highlighted step below is where it first breaks."
            f"{bind_html}"
            "</div>"
        )
    if trace:
        return (
            '<div class="callout info">'
            "<strong>Representative reachable trace</strong> — no violation; "
            "a concrete path the model can take."
            "</div>"
        )
    return ""


def _witness_section(witnesses: list) -> str:
    if not witnesses:
        content = '<p class="empty">No witnesses emitted at this depth.</p>'
    else:
        cards = []
        for witness in witnesses:
            steps = witness.get("narration") or []
            step_html = "".join(
                f'<div class="step"><div class="step-num">{i}</div><div class="step-body"><strong>{escape(step)}</strong></div></div>'
                for i, step in enumerate(steps, start=1)
            )
            states = {
                "initial_state": witness.get("initial_state"),
                "expected_states": witness.get("expected_states"),
            }
            step_content = step_html or '<p class="empty">No steps.</p>'
            cards.append(
                '<article class="mini-card">'
                f'<h3><span>{escape(str(witness.get("name", "")))}</span>{_badge(witness.get("kind", ""))}</h3>'
                f'<p><code>{escape(str(witness.get("target", "")))}</code></p>'
                f'<div class="timeline">{step_content}</div>'
                f'{_details("State snapshots", _json_pre(states))}'
                '</article>'
            )
        content = f'<div class="cards">{"".join(cards)}</div>'
    return f"""
      <section class="section" id="witnesses">
        <div class="section-head">
          <div>
            <h2>Witnesses</h2>
            <p>Concrete examples that exercise reachable targets and action coverage.</p>
          </div>
        </div>
        {content}
      </section>
"""


def _counterfactual_section(counterfactuals: list) -> str:
    if not counterfactuals:
        content = '<p class="empty">No counterfactuals were generated for this spec.</p>'
    else:
        rows = []
        for item in counterfactuals:
            weakening = item.get("weakening") or {}
            weak_text = "none"
            if weakening:
                weak_text = (
                    f"{escape(str(weakening.get('op', '')))}: "
                    f"{escape(str(weakening.get('target', '')))}"
                    f"<br><code>{escape(str(weakening.get('source_text', '')))}</code>"
                )
            trace = item.get("trace")
            detail = _details("Trace / violation", _json_pre({
                "trace": trace,
                "violation": item.get("violation"),
                "note": item.get("note"),
            }))
            name_cell = f"<code>{escape(str(item.get('invariant', '')))}</code>{_requirement_caption(item.get('requirement'))}"
            rows.append(
                "<tr>"
                f"<td>{name_cell}</td>"
                f"<td>{weak_text}</td>"
                f"<td>{detail}</td>"
                "</tr>"
            )
        content = f"""
          <div class="panel table-wrap">
            <table>
              <thead><tr><th>Invariant</th><th>Weakening</th><th>Evidence</th></tr></thead>
              <tbody>{''.join(rows)}</tbody>
            </table>
          </div>
"""
    return f"""
      <section class="section" id="counterfactuals">
        <div class="section-head">
          <div>
            <h2>Counterfactuals</h2>
            <p>What would break if a rule or transition detail were removed.</p>
          </div>
        </div>
        {content}
      </section>
"""


def _source_section(source: str) -> str:
    rows = []
    for idx, line in enumerate(source.splitlines(), start=1):
        rows.append(f"<tr><td>{idx}</td><td>{escape(line)}</td></tr>")
    return f"""
      <section class="section" id="source">
        <div class="section-head">
          <div>
            <h2>Source</h2>
            <p>Original FSL with stable line numbers for review comments.</p>
          </div>
        </div>
        <div class="panel table-wrap">
          <table class="source-table">
            <tbody>{''.join(rows)}</tbody>
          </table>
        </div>
      </section>
"""


def _raw_data_section(explained: dict, verification: dict) -> str:
    return f"""
      <section class="section" id="raw-data">
        <div class="section-head">
          <div>
            <h2>Raw Data</h2>
            <p>Original JSON payloads used to build this report.</p>
          </div>
        </div>
        {_details("explain JSON", _json_pre(explained))}
        {_details("verify JSON", _json_pre(verification))}
      </section>
"""


def _influence_svg(states: list, actions: list) -> str:
    width = 720
    row = 54
    height = max(320, 90 + row * max(len(states), len(actions), 1))
    state_y = _node_positions(len(states), height)
    action_y = _node_positions(len(actions), height)
    state_index = {name: i for i, name in enumerate(states)}
    edges = []
    for ai, action in enumerate(actions):
        for write in action.get("writes", []):
            if write not in state_index:
                continue
            y1 = action_y[ai]
            y2 = state_y[state_index[write]]
            edges.append(
                f'<path d="M 455 {y1} C 360 {y1}, 360 {y2}, 268 {y2}" '
                'fill="none" stroke="#087f7a" stroke-width="1.6" opacity="0.55" '
                'marker-end="url(#flow-arrow)"/>'
            )
    state_nodes = []
    for name, y in zip(states, state_y):
        state_nodes.append(
            f'<rect x="46" y="{y - 17}" width="220" height="34" rx="8" fill="#d8f0eb" stroke="#afdcd7"/>'
            f'<text x="62" y="{y + 5}">{escape(name)}</text>'
        )
    action_nodes = []
    for action, y in zip(actions, action_y):
        label = str(action.get("name", ""))
        action_nodes.append(
            f'<rect x="455" y="{y - 17}" width="220" height="34" rx="8" fill="#fff2d2" stroke="#efd39a"/>'
            f'<text x="471" y="{y + 5}">{escape(_shorten(label, 27))}</text>'
        )
    return f"""
      <svg class="flow-svg" viewBox="0 0 {width} {height}" role="img" aria-label="Action to state write graph">
        <defs>
          <marker id="flow-arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="8" markerHeight="8" orient="auto">
            <path d="M 0 0 L 10 5 L 0 10 z" fill="#087f7a"/>
          </marker>
        </defs>
        <text class="small" x="48" y="34">STATE &#8592; writes</text>
        <text class="small" x="457" y="34">ACTIONS</text>
        {''.join(edges)}
        {''.join(state_nodes)}
        {''.join(action_nodes)}
      </svg>
"""


def _node_positions(count: int, height: int) -> list:
    if count <= 0:
        return []
    if count == 1:
        return [height // 2]
    top = 72
    bottom = height - 42
    gap = (bottom - top) / (count - 1)
    return [round(top + gap * i, 1) for i in range(count)]


def _trace_timeline(trace: list, violated_step=None) -> str:
    steps = []
    for entry in trace:
        step_no = entry.get("step", len(steps))
        action = entry.get("action") or {}
        action_name = action.get("name", "initial")
        params = action.get("params") or {}
        changes = entry.get("changes") or {}
        rendered_changes = "".join(
            f'<div class="change">{escape(str(k))}: {escape(str(v.get("from")))} -> {escape(str(v.get("to")))}</div>'
            for k, v in changes.items() if isinstance(v, dict)
        )
        if not rendered_changes and step_no == 0:
            rendered_changes = '<div class="change">initial state</div>'
        is_bad = violated_step is not None and step_no == violated_step
        badge = '<span class="badge bad">violation</span>' if is_bad else ""
        steps.append(
            f'<div class="step{" bad" if is_bad else ""}">'
            f'<div class="step-num">{escape(str(step_no))}</div>'
            '<div class="step-body">'
            f'<strong>{escape(str(action_name))}</strong> {_params(params)} {badge}'
            f'<div class="changes">{rendered_changes}</div>'
            f'{_details("State", _json_pre(entry.get("state")))}'
            '</div></div>'
        )
    return f'<div class="timeline">{"".join(steps)}</div>'


def _relation_graphs(trace: list) -> str:
    panels = []
    for entry in trace:
        step = entry.get("step")
        state = entry.get("state") or {}
        for name, value in state.items():
            if not _looks_like_relation(value):
                continue
            edges = "".join(
                f'<span class="edge">{escape(str(src))} -&gt; {escape(str(dst))}</span>'
                for src, dst in value
            )
            panels.append(
                '<div class="mini-card">'
                f'<h3><span>{escape(str(name))}</span><span class="badge info">step {escape(str(step))}</span></h3>'
                f'<div class="edge-list">{edges}</div>'
                '</div>'
            )
    if not panels:
        return ""
    return f"""
      <div class="relation-graphs">
        <h3>Relation Graphs</h3>
        <div class="cards">{''.join(panels)}</div>
      </div>
"""


def _looks_like_relation(value) -> bool:
    return (
        isinstance(value, list)
        and bool(value)
        and all(isinstance(item, list) and len(item) == 2 for item in value)
    )


def _first_reachable_witness(verification: dict):
    reachables = verification.get("reachables") or {}
    for item in reachables.values():
        if isinstance(item, dict) and item.get("witness"):
            return item["witness"]
    return None


def _type_text(ty, enums=None) -> str:
    if isinstance(ty, list):
        ty = tuple(ty)
    if not isinstance(ty, tuple) or not ty:
        return str(ty)
    tag = ty[0]
    if tag == "bool":
        return "Bool"
    if tag == "int":
        return "Int"
    if tag == "domain":
        return f"{ty[1]}..{ty[2]}"
    if tag == "map":
        return f"Map<{_type_text(ty[1], enums)}, {_type_text(ty[2], enums)}>"
    if tag == "option":
        return f"Option<{_type_text(ty[1], enums)}>"
    if tag == "set":
        return f"Set<{_type_text(ty[1], enums)}>"
    if tag == "seq":
        return f"Seq<{_type_text(ty[1], enums)}, {ty[2]}>"
    if tag == "relation":
        return f"relation {_type_text(ty[1], enums)} -> {_type_text(ty[2], enums)}"
    if tag == "enum":
        name = str(ty[1])
        members = (enums or {}).get(name)
        return f"{name} {{{', '.join(members)}}}" if members else name
    if tag in ("struct", "named", "name"):
        return str(ty[1])
    return str(ty)


def _strip_lead(items, prefix) -> list:
    """Drop a redundant leading keyword (the column header already names it)."""
    return [
        item[len(prefix):] if isinstance(item, str) and item.startswith(prefix) else item
        for item in (items or [])
    ]


def _chip_list(items, kind="") -> str:
    values = [str(item) for item in (items or [])]
    if not values:
        return '<span class="chip">none</span>'
    cls = f"chip {kind}".strip()
    return '<div class="chips">' + "".join(
        f'<span class="{cls}">{escape(value)}</span>' for value in values
    ) + "</div>"


def _is_actor_only_requirement(req) -> bool:
    """True when a requirement's whole content is a business-dialect `by
    <actor>` annotation (no distinct id/prose beyond that) — i.e. it's exactly
    what the Actor column already displays, so captioning it too is redundant."""
    if not req:
        return False
    text = req.get("text")
    return isinstance(text, str) and text.startswith("by ")


def _requirement_caption(req) -> str:
    """Render `{id, text}` meta as an inline caption under a declaration's own
    name — the primary human-language label — rather than a trailing column;
    when there's no meta at all, this renders nothing (no "none" filler)."""
    if not req:
        return ""
    rid = req.get("id")
    text = req.get("text")
    if rid and text:
        label = f"{rid}: {text}"
    elif rid:
        label = str(rid)
    else:
        label = str(text)
    return f'<div class="req-caption">{escape(label)}</div>'


def _params(params: dict) -> str:
    if not params:
        return ""
    rendered = ", ".join(f"{k}={v}" for k, v in params.items())
    return f"<code>{escape(rendered)}</code>"


def _details(label: str, inner: str) -> str:
    return f"<details><summary>{escape(label)}</summary><div>{inner}</div></details>"


def _json_pre(data) -> str:
    return f'<div class="code-block"><pre>{escape(json.dumps(data, indent=2, ensure_ascii=False, sort_keys=True))}</pre></div>'


def _json_code(data) -> str:
    return f"<code>{escape(json.dumps(data, ensure_ascii=False, sort_keys=True))}</code>"


def _drop_empty(data: dict) -> dict:
    return {k: v for k, v in data.items() if v is not None and v != {} and v != []}


def _badge(value) -> str:
    text = str(value)
    return f'<span class="badge {_status_class(text)}">{escape(text)}</span>'


def _status_class(status: str) -> str:
    status = str(status)
    if status in {"verified", "proved", "ok", "covered", "refines", "generated"}:
        return "ok"
    if status in {"violated", "reachable_failed", "nonconformant", "refinement_failed", "uncovered"}:
        return "bad"
    if status in {"warning", "unknown_cti"}:
        return "warn"
    return "info"


def _shorten(text: str, limit: int) -> str:
    if len(text) <= limit:
        return text
    return text[: max(0, limit - 1)] + "..."
