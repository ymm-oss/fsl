// SPDX-License-Identifier: Apache-2.0

const TIMING = "<timing>";
const BACKEND_STATISTIC = "<backend-statistic>";
const RUNTIME_IDENTITY = "<runtime-identity>";

function valueShape(value) {
  if (value === null) return "<null>";
  if (Array.isArray(value)) return value.map(valueShape);
  if (typeof value !== "object") return `<${typeof value}>`;
  return Object.fromEntries(
    Object.entries(value).sort(([left], [right]) => left.localeCompare(right))
      .map(([key, nested]) => [key, valueShape(nested)]),
  );
}

function stateShape(value) {
  if (Array.isArray(value)) return value.map(stateShape);
  if (!value || typeof value !== "object") return "<value>";
  return Object.fromEntries(
    Object.entries(value).sort(([left], [right]) => left.localeCompare(right))
      .map(([key, nested]) => [key, stateShape(nested)]),
  );
}

function traceShape(trace) {
  requireCondition(Array.isArray(trace) && trace.length > 0, "invalid trace", trace);
  const entries = trace.map((entry, index) => {
    requireCondition(
      entry && typeof entry === "object" && !Array.isArray(entry)
        && entry.step === index && entry.state && typeof entry.state === "object"
        && !Array.isArray(entry.state),
      "invalid trace entry",
      entry,
    );
    const shaped = {
      keys: Object.keys(entry).sort(),
      state: stateShape(entry.state),
    };
    if (index === 0) {
      requireCondition(entry.action === undefined && entry.changes === undefined, "step zero must be action-free", entry);
    } else {
      requireCondition(
        entry.action && typeof entry.action.name === "string"
          && entry.action.params && typeof entry.action.params === "object"
          && !Array.isArray(entry.action.params)
          && entry.changes && typeof entry.changes === "object"
          && !Array.isArray(entry.changes),
        "invalid trace action",
        entry,
      );
      shaped.action = Object.fromEntries(Object.entries(entry.action)
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([key, nested]) => [key, key === "params" ? valueShape(nested) : nested]));
      shaped.changes = Object.entries(entry.changes).map(([path, change]) => {
        requireCondition(change && typeof change === "object" && !Array.isArray(change), "invalid trace change", change);
        return { path: path.replace(/\[[^\]]+\]/g, "[*]"), shape: valueShape(change) };
      }).sort((left, right) => JSON.stringify(left).localeCompare(JSON.stringify(right)));
    }
    if (entry.blame !== undefined) {
      requireCondition(entry.blame && typeof entry.blame === "object" && !Array.isArray(entry.blame), "invalid trace blame", entry.blame);
      shaped.blame = valueShape(entry.blame);
    }
    return shaped;
  });
  return {
    initial: entries[0],
    steps: entries.slice(1).sort((left, right) => {
      const actionOrder = JSON.stringify(left.action).localeCompare(JSON.stringify(right.action));
      return actionOrder || JSON.stringify(left).localeCompare(JSON.stringify(right));
    }),
  };
}

function normalizeTraces(value, path = "$") {
  if (Array.isArray(value)) {
    return value.map((nested, index) => normalizeTraces(nested, `${path}[${index}]`));
  }
  if (typeof value === "string") {
    return /^\$\.warnings\[\d+\]\.message$/.test(path)
      ? value.replace(/(deadlock reachable at step \d+) \(state: .*\)$/, "$1 (state: <witness>)")
      : value;
  }
  if (!value || typeof value !== "object") return value;
  return Object.fromEntries(Object.entries(value).map(([key, nested]) => {
    const nestedPath = `${path}.${key}`;
    const replayedWitness = nestedPath === "$.trace"
      || nestedPath === "$.deadlock.trace"
      || nestedPath.startsWith("$.reachables.") && nestedPath.endsWith(".witness");
    return [
      key,
      replayedWitness && Array.isArray(nested)
        ? traceShape(nested)
        : normalizeTraces(nested, nestedPath),
    ];
  }));
}

function requireCondition(condition, message, value) {
  if (!condition) throw new Error(`${message}: ${JSON.stringify(value)}`);
}

export function validateEnvelope(envelope) {
  requireCondition(envelope && typeof envelope === "object" && !Array.isArray(envelope), "invalid envelope", envelope);
  const versions = envelope.versions;
  requireCondition(
    JSON.stringify(Object.keys(versions ?? {}).sort()) === JSON.stringify(["core", "solver", "verifier"]),
    "invalid versions",
    versions,
  );
  for (const component of ["core", "verifier"]) {
    requireCondition(
      JSON.stringify(Object.keys(versions[component] ?? {}).sort()) === JSON.stringify(["name", "version"]),
      `invalid ${component} version`,
      versions[component],
    );
  }
  requireCondition(
    JSON.stringify(Object.keys(versions.solver ?? {}).sort()) === JSON.stringify(["backend", "name", "version"]),
    "invalid solver version",
    versions.solver,
  );
  requireCondition(
    versions.core.name === "fsl-core"
      && versions.solver.name === "z3"
      && typeof versions.core.version === "string"
      && typeof versions.verifier.version === "string"
      && typeof versions.solver.version === "string",
    "invalid version identity",
    versions,
  );
  if (envelope.cost === undefined) return;
  const cost = envelope.cost;
  requireCondition(
    JSON.stringify(Object.keys(cost ?? {}).sort()) === JSON.stringify(["elapsed_s", "properties", "solver"])
      && Number.isFinite(cost.elapsed_s) && cost.elapsed_s >= 0,
    "invalid cost",
    cost,
  );
  requireCondition(
    JSON.stringify(Object.keys(cost.solver ?? {}).sort())
      === JSON.stringify(["check_elapsed_s", "checks", "conflicts", "decisions", "memory_mb", "propagations"])
      && Number.isInteger(cost.solver.checks) && cost.solver.checks >= 0
      && Number.isFinite(cost.solver.check_elapsed_s) && cost.solver.check_elapsed_s >= 0,
    "invalid solver cost",
    cost.solver,
  );
  for (const key of ["conflicts", "decisions", "propagations", "memory_mb"]) {
    requireCondition(
      cost.solver[key] === null || (Number.isFinite(cost.solver[key]) && cost.solver[key] >= 0),
      `invalid solver statistic ${key}`,
      cost.solver,
    );
  }
  const identities = cost.properties.map((property) => {
    requireCondition(
      JSON.stringify(Object.keys(property).sort()) === JSON.stringify(["checks", "elapsed_s", "kind", "name"])
        && typeof property.kind === "string" && property.kind
        && typeof property.name === "string" && property.name
        && Number.isInteger(property.checks) && property.checks > 0
        && Number.isFinite(property.elapsed_s) && property.elapsed_s >= 0,
      "invalid property cost",
      property,
    );
    return `${property.kind}\u0000${property.name}`;
  });
  requireCondition(
    JSON.stringify(identities) === JSON.stringify([...identities].sort())
      && cost.properties.reduce((sum, property) => sum + property.checks, 0) === cost.solver.checks,
    "non-deterministic or incomplete property cost",
    cost,
  );
}

export function normalizeEnvelope(envelope) {
  validateEnvelope(envelope);
  const normalized = normalizeTraces(structuredClone(envelope));
  normalized.versions.verifier.name = RUNTIME_IDENTITY;
  normalized.versions.solver.backend = RUNTIME_IDENTITY;
  if (normalized.cost !== undefined) {
    normalized.cost.elapsed_s = TIMING;
    normalized.cost.solver.check_elapsed_s = TIMING;
    for (const key of ["conflicts", "decisions", "propagations", "memory_mb"]) {
      normalized.cost.solver[key] = BACKEND_STATISTIC;
    }
    for (const property of normalized.cost.properties) property.elapsed_s = TIMING;
  }
  return normalized;
}

export function differences(native, wasm, path = "$") {
  if (Object.is(native, wasm)) return [];
  if (Array.isArray(native) && Array.isArray(wasm)) {
    if (native.length !== wasm.length) {
      return [{ path, native: { length: native.length }, wasm: { length: wasm.length } }];
    }
    return native.flatMap((value, index) => differences(value, wasm[index], `${path}[${index}]`));
  }
  if (native && wasm && typeof native === "object" && typeof wasm === "object"
      && !Array.isArray(native) && !Array.isArray(wasm)) {
    const keys = [...new Set([...Object.keys(native), ...Object.keys(wasm)])].sort();
    return keys.flatMap((key) => {
      if (!(key in native) || !(key in wasm)) {
        return [{
          path: `${path}.${key}`,
          native: key in native ? native[key] : { missing: true },
          wasm: key in wasm ? wasm[key] : { missing: true },
        }];
      }
      return differences(native[key], wasm[key], `${path}.${key}`);
    });
  }
  return [{ path, native, wasm }];
}

export function assertNormalizerContract() {
  const base = {
    fsl: "1.0",
    result: "verified",
    loc: { line: 2, column: 3 },
    versions: {
      core: { name: "fsl-core", version: "2.7.0" },
      verifier: { name: "fslc-rust", version: "2.7.0" },
      solver: { name: "z3", version: "Z3 4.16.0.0", backend: "native-z3" },
    },
  };
  const changedLocation = structuredClone(base);
  changedLocation.loc.line = 9;
  requireCondition(
    differences(normalizeEnvelope(base), normalizeEnvelope(changedLocation))[0]?.path === "$.loc.line",
    "location differences must not be normalized",
    changedLocation,
  );
  const missing = structuredClone(base);
  delete missing.loc;
  requireCondition(
    differences(normalizeEnvelope(base), normalizeEnvelope(missing))[0]?.path === "$.loc",
    "missing fields must not equal null or a present value",
    missing,
  );
  const traceEnvelope = {
    ...base,
    trace: [
      { step: 0, state: { selected: 1 } },
      {
        step: 1,
        state: { selected: 2 },
        action: { name: "choose", params: { item: 2 }, loc: { line: 4, column: 3 } },
        changes: { selected: { from: 1, to: 2 } },
      },
    ],
  };
  const alternateWitness = structuredClone(traceEnvelope);
  alternateWitness.trace[0].state.selected = 2;
  alternateWitness.trace[1].state.selected = 1;
  alternateWitness.trace[1].action.params.item = 1;
  alternateWitness.trace[1].changes.selected = { from: 2, to: 1 };
  requireCondition(
    differences(normalizeEnvelope(traceEnvelope), normalizeEnvelope(alternateWitness)).length === 0,
    "concrete values in replayed traces are intentionally non-unique",
    alternateWitness,
  );
  const differentShape = structuredClone(traceEnvelope);
  differentShape.trace[1].action.loc.line = 99;
  requireCondition(
    differences(normalizeEnvelope(traceEnvelope), normalizeEnvelope(differentShape))[0]?.path
      === "$.trace.steps[0].action.loc.line",
    "trace locations must remain exact",
    differentShape,
  );
  const differentStateKey = structuredClone(traceEnvelope);
  differentStateKey.trace[1].state = { wrong: 2 };
  requireCondition(
    differences(normalizeEnvelope(traceEnvelope), normalizeEnvelope(differentStateKey)).some(
      (difference) => difference.path.startsWith("$.trace.steps[0].state"),
    ),
    "trace state keys must remain exact",
    differentStateKey,
  );
  const commutingWitness = {
    ...base,
    trace: [
      { step: 0, state: { selected: null, credit: 0 } },
      {
        step: 1,
        state: { selected: 2, credit: 0 },
        action: { name: "select", params: { item: 2 }, loc: { line: 5, column: 3 } },
        changes: { selected: { from: null, to: 2 } },
      },
      {
        step: 2,
        state: { selected: 2, credit: 1 },
        action: { name: "insert", params: {}, loc: { line: 6, column: 3 } },
        changes: { credit: { from: 0, to: 1 } },
      },
    ],
  };
  const reverseCommutingWitness = {
    ...base,
    trace: [
      { step: 0, state: { selected: null, credit: 0 } },
      {
        step: 1,
        state: { selected: null, credit: 1 },
        action: { name: "insert", params: {}, loc: { line: 6, column: 3 } },
        changes: { credit: { from: 0, to: 1 } },
      },
      {
        step: 2,
        state: { selected: 2, credit: 1 },
        action: { name: "select", params: { item: 2 }, loc: { line: 5, column: 3 } },
        changes: { selected: { from: null, to: 2 } },
      },
    ],
  };
  requireCondition(
    differences(
      normalizeEnvelope(commutingWitness),
      normalizeEnvelope(reverseCommutingWitness),
    ).length === 0,
    "commuting replayed actions are intentionally order-independent",
    reverseCommutingWitness,
  );
  const duplicateWriteNative = {
    ...base,
    result: "error",
    kind: "semantics",
    message: "an action may not assign the same state location more than once",
  };
  const duplicateWriteBefore267 = {
    ...base,
    result: "verified",
    depth: 3,
  };
  requireCondition(
    differences(
      normalizeEnvelope(duplicateWriteNative),
      normalizeEnvelope(duplicateWriteBefore267),
    ).some((difference) => difference.path === "$.result"),
    "the pre-#267 Worker duplicate-write behavior must fail parity",
    duplicateWriteBefore267,
  );
}
