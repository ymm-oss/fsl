// SPDX-License-Identifier: Apache-2.0

//! Alternate-language conformance-test emitters.

use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;

fn inline(value: &Value) -> String {
    match value {
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(inline).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!(
                    "{}: {}",
                    serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_owned()),
                    inline(value)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned()),
    }
}

fn array(values: &[Value]) -> String {
    if values.is_empty() {
        return "[]".to_owned();
    }
    format!(
        "[\n{},\n]",
        values
            .iter()
            .map(|value| format!("  {}", inline(value)))
            .collect::<Vec<_>>()
            .join(",\n")
    )
}

fn vitest_scenario(scenario: &Value) -> String {
    let name = scenario["name"].as_str().unwrap_or_default();
    let mut lines = vec![
        format!(
            "scenario({}, () => {{",
            inline(&Value::String(format!("scenario: {name}")))
        ),
        "  adapter.reset();".to_owned(),
    ];
    let steps = scenario["steps"].as_array().cloned().unwrap_or_default();
    let states = scenario["expected_states"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let empty = Value::Object(serde_json::Map::new());
    for (index, step) in steps.iter().enumerate() {
        lines.push(format!(
            "  adapter.step({}, {});",
            inline(&step["action"]),
            inline(&step["params"])
        ));
        lines.push(format!(
            "  assertPartial(adapter.observe(), {});",
            inline(states.get(index).unwrap_or(&empty))
        ));
    }
    if scenario["kind"].as_str() == Some("forbidden") && scenario["forbidden_step"].is_object() {
        let step = &scenario["forbidden_step"];
        lines.push(format!(
            "  const result = adapter.step({}, {});",
            inline(&step["action"]),
            inline(&step["params"])
        ));
        lines.push(format!(
            "  assertRejected(result, {});",
            inline(&scenario["rejected_by"])
        ));
    }
    lines.push("});".to_owned());
    lines.join("\n")
}

/// Emit a standalone Vitest conformance scaffold.
#[must_use]
pub fn emit_vitest(source_name: &str, scenarios: &[Value], walk: &Value) -> String {
    let mut parts = vec![include_str!("testgen_vitest.txt").replace("__SOURCE__", source_name)];
    parts.extend(scenarios.iter().map(vitest_scenario));
    let steps = walk["steps"].as_array().cloned().unwrap_or_default();
    parts.push(
        [
            "interface WalkStep {".to_owned(),
            "  action: string;".to_owned(),
            "  params: Record<string, unknown>;".to_owned(),
            "  expected: Record<string, unknown>;".to_owned(),
            "}".to_owned(),
            String::new(),
            format!(
                "const RANDOM_WALK_INITIAL: Record<string, unknown> = {};",
                inline(&walk["initial"])
            ),
            format!("const RANDOM_WALK: WalkStep[] = {};", array(&steps)),
            String::new(),
            "scenario(\"random-walk conformance (baked oracle trace)\", () => {".to_owned(),
            "  adapter.reset();".to_owned(),
            "  assertPartial(adapter.observe(), RANDOM_WALK_INITIAL);".to_owned(),
            "  for (const step of RANDOM_WALK) {".to_owned(),
            "    adapter.step(step.action, step.params);".to_owned(),
            "    assertPartial(adapter.observe(), step.expected);".to_owned(),
            "  }".to_owned(),
            "});".to_owned(),
        ]
        .join("\n"),
    );
    parts.join("\n\n") + "\n"
}

#[derive(Clone, Copy)]
enum Target {
    Swift,
    Kotlin,
    Dart,
    Php,
}

fn quoted(value: &str, target: Target) -> String {
    let quote = if matches!(target, Target::Dart | Target::Php) {
        '\''
    } else {
        '"'
    };
    let mut result = String::from(quote);
    for character in value.chars() {
        match (target, character) {
            (Target::Php, '\\') => result.push_str("\\\\"),
            (Target::Php | Target::Dart, '\'') => result.push_str("\\'"),
            (Target::Php, _) => result.push(character),
            (_, '"') if quote == '"' => result.push_str("\\\""),
            (_, '\\') => result.push_str("\\\\"),
            (Target::Kotlin | Target::Dart, '$') => result.push_str("\\$"),
            (_, '\n') => result.push_str("\\n"),
            (_, '\t') => result.push_str("\\t"),
            (_, '\r') => result.push_str("\\r"),
            (Target::Swift, '\0') => result.push_str("\\0"),
            (Target::Swift | Target::Dart, value) if value < ' ' => {
                let _ = write!(result, "\\u{{{:x}}}", u32::from(value));
            }
            (Target::Kotlin, value) if value < ' ' => {
                let _ = write!(result, "\\u{:04x}", u32::from(value));
            }
            (_, value) => result.push(value),
        }
    }
    result.push(quote);
    result
}

fn literal(value: &Value, target: Target) -> String {
    match value {
        Value::Null => match target {
            Target::Swift => "FSLNull.instance".to_owned(),
            _ => "null".to_owned(),
        },
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => quoted(value, target),
        Value::Array(values) if values.is_empty() => match target {
            Target::Swift | Target::Php => "[]".to_owned(),
            Target::Kotlin => "listOf<Any?>()".to_owned(),
            Target::Dart => "<dynamic>[]".to_owned(),
        },
        Value::Array(values) => {
            let items = values
                .iter()
                .map(|value| literal(value, target))
                .collect::<Vec<_>>()
                .join(", ");
            match target {
                Target::Kotlin => format!("listOf({items})"),
                _ => format!("[{items}]"),
            }
        }
        Value::Object(values) if values.is_empty() => match target {
            Target::Swift => "[String: Any]()".to_owned(),
            Target::Kotlin => "mapOf<String, Any?>()".to_owned(),
            Target::Dart => "<String, dynamic>{}".to_owned(),
            Target::Php => "[]".to_owned(),
        },
        Value::Object(values) => {
            let items = values
                .iter()
                .map(|(key, value)| match target {
                    Target::Swift | Target::Dart => {
                        format!("{}: {}", quoted(key, target), literal(value, target))
                    }
                    Target::Kotlin => {
                        format!("{} to {}", quoted(key, target), literal(value, target))
                    }
                    Target::Php => format!("{} => {}", quoted(key, target), literal(value, target)),
                })
                .collect::<Vec<_>>()
                .join(", ");
            match target {
                Target::Kotlin => format!("mapOf({items})"),
                Target::Dart => format!("{{{items}}}"),
                _ => format!("[{items}]"),
            }
        }
    }
}

fn safe_ident(value: &str) -> String {
    let mut result = String::new();
    let mut underscore = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            result.push(character);
            underscore = false;
        } else if !underscore {
            result.push('_');
            underscore = true;
        }
    }
    let result = result.trim_matches('_');
    if result.is_empty() {
        "scenario".to_owned()
    } else {
        result.to_owned()
    }
}

fn unique_ident(value: &str, seen: &mut BTreeMap<String, usize>, prefix: &str) -> String {
    let safe = safe_ident(value);
    let count = seen.entry(safe.clone()).or_default();
    *count += 1;
    if *count == 1 {
        format!("{prefix}{safe}")
    } else {
        format!("{prefix}{safe}_{count}")
    }
}

fn scenario_parts(scenario: &Value) -> (&str, Vec<Value>, Vec<Value>) {
    (
        scenario["name"].as_str().unwrap_or("scenario"),
        scenario["steps"].as_array().cloned().unwrap_or_default(),
        scenario["expected_states"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    )
}

fn append_forbidden(lines: &mut Vec<String>, scenario: &Value, target: Target, indent: &str) {
    if scenario["kind"].as_str() != Some("forbidden") || !scenario["forbidden_step"].is_object() {
        return;
    }
    let step = &scenario["forbidden_step"];
    let rejected = scenario
        .get("rejected_by")
        .filter(|value| !value.is_null())
        .map_or_else(|| "null".to_owned(), |value| literal(value, target));
    let action = literal(&step["action"], target);
    let params = literal(&step["params"], target);
    match target {
        Target::Swift => {
            lines.push(format!("{indent}let result = a.step({action}, {params})"));
            lines.push(format!("{indent}assertRejected(result, {rejected})"));
        }
        Target::Kotlin => {
            lines.push(format!("{indent}val result = a.step({action}, {params})"));
            lines.push(format!("{indent}assertRejected(result, {rejected})"));
        }
        Target::Dart => {
            lines.push(format!(
                "{indent}final result = a.step({action}, {params});"
            ));
            lines.push(format!("{indent}assertRejected(result, {rejected});"));
        }
        Target::Php => {
            lines.push(format!("{indent}$result = $a->step({action}, {params});"));
            lines.push(format!(
                "{indent}$this->assertRejected($result, {rejected});"
            ));
        }
    }
}

/// Emit a standalone Swift Testing conformance scaffold.
#[must_use]
pub fn emit_swift(source_name: &str, scenarios: &[Value], walk: &Value) -> String {
    let mut parts = vec![include_str!("testgen_swift.txt").replace("__SOURCE__", source_name)];
    let mut seen = BTreeMap::new();
    for scenario in scenarios {
        let (name, steps, states) = scenario_parts(scenario);
        let function = unique_ident(name, &mut seen, "scenario_");
        let mut lines = vec![
            format!("@Test(.enabled(if: isAdapterWired())) func {function}() throws {{"),
            "    let a = try makeAdapter()".to_owned(),
            "    a.reset()".to_owned(),
        ];
        for (index, step) in steps.iter().enumerate() {
            lines.push(format!(
                "    _ = a.step({}, {})",
                literal(&step["action"], Target::Swift),
                literal(&step["params"], Target::Swift)
            ));
            lines.push(format!(
                "    assertPartial(a.observe(), {})",
                literal(states.get(index).unwrap_or(&Value::Null), Target::Swift)
            ));
        }
        append_forbidden(&mut lines, scenario, Target::Swift, "    ");
        lines.push("}".to_owned());
        parts.push(lines.join("\n"));
    }
    let rows = walk["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|step| {
            format!(
                "        (action: {}, params: {}, expected: {})",
                literal(&step["action"], Target::Swift),
                literal(&step["params"], Target::Swift),
                literal(&step["expected"], Target::Swift)
            )
        })
        .collect::<Vec<_>>();
    let walk_literal = if rows.is_empty() {
        "[]".to_owned()
    } else {
        format!("[\n{},\n    ]", rows.join(",\n"))
    };
    parts.push([
        "@Test(.enabled(if: isAdapterWired())) func randomWalkConformance() throws {".to_owned(),
        "    // random-walk conformance (baked oracle trace)".to_owned(),
        "    let a = try makeAdapter()".to_owned(), "    a.reset()".to_owned(),
        format!("    let initial: [String: Any] = {}", literal(&walk["initial"], Target::Swift)),
        "    assertPartial(a.observe(), initial)".to_owned(),
        format!("    let walk: [(action: String, params: [String: Any], expected: [String: Any])] = {walk_literal}"),
        "    for step in walk {".to_owned(), "        _ = a.step(step.action, step.params)".to_owned(),
        "        assertPartial(a.observe(), step.expected)".to_owned(), "    }".to_owned(), "}".to_owned(),
    ].join("\n"));
    parts.join("\n\n") + "\n"
}

/// Emit a standalone kotlin.test conformance scaffold.
#[must_use]
pub fn emit_kotlin(
    source_name: &str,
    spec_name: &str,
    scenarios: &[Value],
    walk: &Value,
) -> String {
    let preamble = include_str!("testgen_kotlin.txt").replace("__SOURCE__", source_name);
    let mut lines = vec![format!("class {spec_name}ConformanceTest {{")];
    let mut seen = BTreeMap::new();
    for scenario in scenarios {
        let (name, steps, states) = scenario_parts(scenario);
        lines.push(String::new());
        lines.push(format!(
            "    @Test fun {}() {{",
            unique_ident(name, &mut seen, "scenario_")
        ));
        lines.push("        val a = makeAdapter() ?: return".to_owned());
        lines.push("        a.reset()".to_owned());
        for (index, step) in steps.iter().enumerate() {
            lines.push(format!(
                "        a.step({}, {})",
                literal(&step["action"], Target::Kotlin),
                literal(&step["params"], Target::Kotlin)
            ));
            lines.push(format!(
                "        assertPartial(a.observe(), {})",
                literal(states.get(index).unwrap_or(&Value::Null), Target::Kotlin)
            ));
        }
        append_forbidden(&mut lines, scenario, Target::Kotlin, "        ");
        lines.push("    }".to_owned());
    }
    let rows = walk["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|step| {
            format!(
                "            Triple({}, {}, {})",
                literal(&step["action"], Target::Kotlin),
                literal(&step["params"], Target::Kotlin),
                literal(&step["expected"], Target::Kotlin)
            )
        })
        .collect::<Vec<_>>();
    let walk_literal = if rows.is_empty() {
        "listOf<Triple<String, Map<String, Any?>, Map<String, Any?>>>()".to_owned()
    } else {
        format!("listOf(\n{},\n        )", rows.join(",\n"))
    };
    lines.extend([
        String::new(),
        "    @Test fun randomWalkConformance() {".to_owned(),
        "        // random-walk conformance (baked oracle trace)".to_owned(),
        "        val a = makeAdapter() ?: return".to_owned(),
        "        a.reset()".to_owned(),
        format!(
            "        val initial: Map<String, Any?> = {}",
            literal(&walk["initial"], Target::Kotlin)
        ),
        "        assertPartial(a.observe(), initial)".to_owned(),
        format!(
            "        val walk: List<Triple<String, Map<String, Any?>, Map<String, Any?>>> = {walk_literal}"
        ),
        "        for ((action, params, expected) in walk) {".to_owned(),
        "            a.step(action, params)".to_owned(),
        "            assertPartial(a.observe(), expected)".to_owned(),
        "        }".to_owned(),
        "    }".to_owned(),
        "}".to_owned(),
    ]);
    format!("{preamble}\n\n{}\n", lines.join("\n"))
}

/// Emit a standalone package:test Dart conformance scaffold.
#[must_use]
pub fn emit_dart(source_name: &str, scenarios: &[Value], walk: &Value) -> String {
    let preamble = include_str!("testgen_dart.txt").replace("__SOURCE__", source_name);
    let mut lines = vec![
        "void main() {".to_owned(),
        "  final wired = _adapterWired();".to_owned(),
    ];
    for scenario in scenarios {
        let (name, steps, states) = scenario_parts(scenario);
        lines.push(String::new());
        lines.push(format!(
            "  test({}, () {{",
            quoted(&format!("scenario: {name}"), Target::Dart)
        ));
        lines.push("    final a = makeAdapter();".to_owned());
        lines.push("    a.reset();".to_owned());
        for (index, step) in steps.iter().enumerate() {
            lines.push(format!(
                "    a.step({}, {});",
                literal(&step["action"], Target::Dart),
                literal(&step["params"], Target::Dart)
            ));
            lines.push(format!(
                "    assertPartial(a.observe(), {});",
                literal(states.get(index).unwrap_or(&Value::Null), Target::Dart)
            ));
        }
        append_forbidden(&mut lines, scenario, Target::Dart, "    ");
        lines.push("  }, skip: wired ? null : 'Adapter not wired');".to_owned());
    }
    let rows = walk["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|step| {
            format!(
                "      {{'action': {}, 'params': {}, 'expected': {}}}",
                literal(&step["action"], Target::Dart),
                literal(&step["params"], Target::Dart),
                literal(&step["expected"], Target::Dart)
            )
        })
        .collect::<Vec<_>>();
    let walk_literal = if rows.is_empty() {
        "<Map<String, dynamic>>[]".to_owned()
    } else {
        format!("<Map<String, dynamic>>[\n{},\n    ]", rows.join(",\n"))
    };
    lines.extend([
        String::new(),
        "  test('random-walk conformance (baked oracle trace)', () {".to_owned(),
        "    final a = makeAdapter();".to_owned(),
        "    a.reset();".to_owned(),
        format!(
            "    final initial = {};",
            literal(&walk["initial"], Target::Dart)
        ),
        "    assertPartial(a.observe(), Map<String, dynamic>.from(initial));".to_owned(),
        format!("    final walk = {walk_literal};"),
        "    for (final step in walk) {".to_owned(),
        "      a.step(step['action'] as String, step['params'] as Map<String, dynamic>);"
            .to_owned(),
        "      assertPartial(a.observe(), step['expected'] as Map<String, dynamic>);".to_owned(),
        "    }".to_owned(),
        "  }, skip: wired ? null : 'Adapter not wired');".to_owned(),
        "}".to_owned(),
    ]);
    format!("{preamble}\n\n{}\n", lines.join("\n"))
}

/// Emit a standalone `PHPUnit` conformance scaffold.
#[must_use]
pub fn emit_phpunit(
    source_name: &str,
    spec_name: &str,
    scenarios: &[Value],
    walk: &Value,
) -> String {
    let preamble = include_str!("testgen_php.txt").replace("__SOURCE__", source_name);
    let mut lines = vec![
        format!("final class {spec_name}ConformanceTest extends TestCase"),
        "{".to_owned(),
        include_str!("testgen_php_helpers.txt")
            .trim_end()
            .to_owned(),
    ];
    let mut seen = BTreeMap::new();
    for scenario in scenarios {
        let (name, steps, states) = scenario_parts(scenario);
        lines.push(String::new());
        lines.push(format!(
            "    public function {}(): void",
            unique_ident(name, &mut seen, "testScenario_")
        ));
        lines.push("    {".to_owned());
        lines.push("        $a = $this->adapter;".to_owned());
        lines.push("        $a->reset();".to_owned());
        for (index, step) in steps.iter().enumerate() {
            lines.push(format!(
                "        $a->step({}, {});",
                literal(&step["action"], Target::Php),
                literal(&step["params"], Target::Php)
            ));
            lines.push(format!(
                "        $this->assertPartial({}, $a->observe());",
                literal(states.get(index).unwrap_or(&Value::Null), Target::Php)
            ));
        }
        append_forbidden(&mut lines, scenario, Target::Php, "        ");
        lines.push("    }".to_owned());
    }
    let rows = walk["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|step| {
            format!(
                "        ['action' => {}, 'params' => {}, 'expected' => {}]",
                literal(&step["action"], Target::Php),
                literal(&step["params"], Target::Php),
                literal(&step["expected"], Target::Php)
            )
        })
        .collect::<Vec<_>>();
    let walk_literal = if rows.is_empty() {
        "[]".to_owned()
    } else {
        format!("[\n{},\n    ]", rows.join(",\n"))
    };
    lines.extend([
        String::new(),
        format!(
            "    private const INITIAL = {};",
            literal(&walk["initial"], Target::Php)
        ),
        format!("    private const WALK = {walk_literal};"),
        String::new(),
        "    public function testRandomWalkConformance(): void".to_owned(),
        "    {".to_owned(),
        "        // random-walk conformance (baked oracle trace)".to_owned(),
        "        $a = $this->adapter;".to_owned(),
        "        $a->reset();".to_owned(),
        "        $this->assertPartial(self::INITIAL, $a->observe());".to_owned(),
        "        foreach (self::WALK as $step) {".to_owned(),
        "            $a->step($step['action'], $step['params']);".to_owned(),
        "            $this->assertPartial($step['expected'], $a->observe());".to_owned(),
        "        }".to_owned(),
        "    }".to_owned(),
        "}".to_owned(),
    ]);
    format!("{preamble}\n\n{}\n", lines.join("\n"))
}
