# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Self-contained HTML report rendering for FSL specs."""
from __future__ import annotations

import json
from html import escape
from pathlib import Path


def default_output_name(file: str) -> str:
    return str(Path(file).with_suffix(".html"))


def render_html_report(file: str, source: str, explained: dict, verification: dict) -> str:
    skeleton = explained.get("skeleton") or {}
    spec = explained.get("spec") or verification.get("spec") or Path(file).stem
    depth = explained.get("depth")
    title = f"{spec} - FSL Specification Report"
    status = verification.get("result", "unknown")
    state = skeleton.get("state") or {}
    actions = skeleton.get("actions") or []
    properties = skeleton.get("properties") or []
    auto_checks = skeleton.get("auto_checks") or []
    witnesses = explained.get("witnesses") or []
    counterfactuals = explained.get("counterfactuals") or []
    warnings = verification.get("warnings") or []

    coverage = verification.get("action_coverage") or {}
    covered = sum(1 for ok in coverage.values() if ok)
    coverage_label = f"{covered}/{len(coverage)}" if coverage else "n/a"

    body = "\n".join([
        _hero(spec, file, depth, status, len(state), len(actions), len(properties), coverage_label, warnings),
        _status_section(verification),
        _model_section(state, actions),
        _actions_section(actions, coverage),
        _properties_section(properties, auto_checks),
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
  min-height: 300px;
  display: grid;
  align-items: end;
  gap: 24px;
  padding: 40px;
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
  font-size: 58px;
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
  overflow-wrap: anywhere;
}
th {
  color: var(--muted);
  background: #fafbf8;
  font: 700 12px/1.3 var(--mono);
  text-transform: uppercase;
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
        <a href="#status">Status</a>
        <a href="#model">Model</a>
        <a href="#actions">Actions</a>
        <a href="#properties">Properties</a>
        <a href="#traces">Traces</a>
        <a href="#witnesses">Witnesses</a>
        <a href="#counterfactuals">Counterfactuals</a>
        <a href="#source">Source</a>
      </nav>
    </aside>
"""


def _hero(spec, file, depth, status, states, actions, properties, coverage, warnings) -> str:
    status_class = _status_class(status)
    return f"""
      <header class="hero">
        <div>
          <p class="eyebrow">FSL specification report</p>
          <h1>{escape(spec)}</h1>
          <p class="sub">Review the written model through verification status, action/state influence, concrete traces, and counterfactual evidence.</p>
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


def _metric(label, value) -> str:
    return f'<div class="metric"><span>{escape(label)}</span><strong>{value}</strong></div>'


def _status_section(verification: dict) -> str:
    rows = [
        ("Result", _badge(verification.get("result", "unknown"))),
        ("Completeness", escape(str(verification.get("completeness", "n/a")))),
        ("Checked depth", escape(str(verification.get("checked_to_depth", verification.get("depth", "n/a"))))),
        ("Note", escape(str(verification.get("note", "")))),
        ("Hint", escape(str(verification.get("hint", "")))),
    ]
    if verification.get("implements"):
        rows.append(("Implements", _json_code(verification["implements"])))
    if verification.get("deadlock"):
        rows.append(("Deadlock", _json_code(verification["deadlock"])))
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


def _model_section(state: dict, actions: list) -> str:
    rows = "".join(
        f"<tr><td><code>{escape(name)}</code></td><td>{escape(_type_text(ty))}</td></tr>"
        for name, ty in sorted(state.items())
    )
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
      </section>
"""


def _actions_section(actions: list, coverage: dict) -> str:
    rows = []
    for action in actions:
        name = action.get("name", "")
        params = ", ".join(
            f"{p.get('name')}: {p.get('type')}" for p in action.get("params", [])
        )
        requires = _chip_list(action.get("requires_text"), "require")
        writes = _chip_list(action.get("writes"), "write")
        ensures = _chip_list(action.get("ensures_text"))
        markers = []
        if action.get("fair"):
            markers.append('<span class="chip fair">fair</span>')
        if name in coverage:
            markers.append(_badge("covered" if coverage[name] else "uncovered"))
        params_html = escape(params) if params else '<span class="chip">none</span>'
        rows.append(
            "<tr>"
            f"<td><code>{escape(name)}</code><br>{''.join(markers)}</td>"
            f"<td>{params_html}</td>"
            f"<td>{requires}</td><td>{writes}</td><td>{ensures}</td>"
            f"<td>{_requirement(action.get('requirement'))}</td>"
            "</tr>"
        )
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
            <thead><tr><th>Action</th><th>Params</th><th>Requires</th><th>Writes</th><th>Ensures</th><th>Requirement</th></tr></thead>
            <tbody>{''.join(rows) or '<tr><td colspan="6">No actions.</td></tr>'}</tbody>
          </table>
        </div>
      </section>
"""


def _properties_section(properties: list, auto_checks: list) -> str:
    prop_rows = []
    for prop in properties:
        prop_rows.append(
            "<tr>"
            f"<td>{escape(str(prop.get('kind', '')))}</td>"
            f"<td><code>{escape(str(prop.get('name', '')))}</code></td>"
            f"<td>{escape(str(prop.get('body_text', '')))}</td>"
            f"<td>{_requirement(prop.get('requirement'))}</td>"
            "</tr>"
        )
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
        <div class="grid-2">
          <div class="panel table-wrap">
            <table>
              <thead><tr><th>Kind</th><th>Name</th><th>Body</th><th>Requirement</th></tr></thead>
              <tbody>{''.join(prop_rows) or '<tr><td colspan="4">No user properties.</td></tr>'}</tbody>
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
    trace = verification.get("trace") or _first_reachable_witness(verification)
    if not trace:
        content = '<p class="empty">No counterexample trace was emitted. Reachable witnesses may still appear below.</p>'
    else:
        content = _trace_timeline(trace)
    return f"""
      <section class="section" id="traces">
        <div class="section-head">
          <div>
            <h2>Trace Review</h2>
            <p>Shortest counterexample or representative reachable trace, step by step.</p>
          </div>
        </div>
        {content}
      </section>
"""


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
            rows.append(
                "<tr>"
                f"<td><code>{escape(str(item.get('invariant', '')))}</code></td>"
                f"<td>{weak_text}</td>"
                f"<td>{_requirement(item.get('requirement'))}</td>"
                f"<td>{detail}</td>"
                "</tr>"
            )
        content = f"""
          <div class="panel table-wrap">
            <table>
              <thead><tr><th>Invariant</th><th>Weakening</th><th>Requirement</th><th>Evidence</th></tr></thead>
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
                f'<path d="M 455 {y1} C 360 {y1}, 360 {y2}, 265 {y2}" '
                'fill="none" stroke="#087f7a" stroke-width="1.6" opacity="0.55"/>'
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
        <text class="small" x="48" y="34">STATE</text>
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


def _trace_timeline(trace: list) -> str:
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
        steps.append(
            '<div class="step">'
            f'<div class="step-num">{escape(str(step_no))}</div>'
            '<div class="step-body">'
            f'<strong>{escape(str(action_name))}</strong> {_params(params)}'
            f'<div class="changes">{rendered_changes}</div>'
            f'{_details("State", _json_pre(entry.get("state")))}'
            '</div></div>'
        )
    return f'<div class="timeline">{"".join(steps)}</div>'


def _first_reachable_witness(verification: dict):
    reachables = verification.get("reachables") or {}
    for item in reachables.values():
        if isinstance(item, dict) and item.get("witness"):
            return item["witness"]
    return None


def _type_text(ty) -> str:
    if isinstance(ty, list):
        ty = tuple(ty)
    if not isinstance(ty, tuple) or not ty:
        return str(ty)
    tag = ty[0]
    if tag == "domain":
        return f"{ty[1]}..{ty[2]}"
    if tag == "map":
        return f"Map<{_type_text(ty[1])}, {_type_text(ty[2])}>"
    if tag == "option":
        return f"Option<{_type_text(ty[1])}>"
    if tag == "set":
        return f"Set<{_type_text(ty[1])}>"
    if tag == "seq":
        return f"Seq<{_type_text(ty[1])}, {ty[2]}>"
    if tag in ("enum", "struct", "named", "name"):
        return str(ty[1])
    return str(ty)


def _chip_list(items, kind="") -> str:
    values = [str(item) for item in (items or [])]
    if not values:
        return '<span class="chip">none</span>'
    cls = f"chip {kind}".strip()
    return '<div class="chips">' + "".join(
        f'<span class="{cls}">{escape(value)}</span>' for value in values
    ) + "</div>"


def _requirement(req) -> str:
    if not req:
        return '<span class="chip">none</span>'
    rid = req.get("id")
    text = req.get("text")
    if rid and text:
        return f'<code>{escape(str(rid))}</code><br>{escape(str(text))}'
    if rid:
        return f'<code>{escape(str(rid))}</code>'
    return escape(str(text))


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
