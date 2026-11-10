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

const REPLAY_TRACE_SCHEMA = "https://fsl.dev/schemas/fslc/kernel/replay-trace.v1.schema.json";
export const WITNESS_REPLAY_FIXTURE = "rust/fslc/tests/fixtures/issue_1789_worker_twin_actions.fsl";

// Reachable and deadlock witnesses only: a `$.trace` counterexample stays
// under the structural comparison alone.
function replayableWitnessSites(native, wasm) {
  const sites = [];
  if (Array.isArray(native?.deadlock?.trace) && Array.isArray(wasm?.deadlock?.trace)) {
    sites.push({
      path: "$.deadlock.trace",
      get: (envelope) => envelope.deadlock.trace,
      set: (envelope, trace) => { envelope.deadlock.trace = trace; },
    });
  }
  for (const name of Object.keys(native?.reachables ?? {}).sort()) {
    if (Array.isArray(native.reachables[name]?.witness) && Array.isArray(wasm?.reachables?.[name]?.witness)) {
      sites.push({
        path: `$.reachables.${name}.witness`,
        get: (envelope) => envelope.reachables[name].witness,
        set: (envelope, trace) => { envelope.reachables[name].witness = trace; },
      });
    }
  }
  return sites;
}

function withoutAction(entry) {
  const { action: _action, ...rest } = entry;
  return rest;
}

export function replayTrace(spec, trace) {
  return {
    $schema: REPLAY_TRACE_SCHEMA,
    schema_version: "1.0.0",
    kernel_schema_version: "1.0.0",
    spec,
    initial: trace[0].state,
    events: trace.slice(1).map((entry, index) => ({
      tick: index + 1,
      action: entry.action.generated_name ?? entry.action.name,
      params: entry.action.params,
      state: entry.state,
    })),
  };
}

async function witnessDisagreements(spec, nativeTrace, wasmTrace, oracle) {
  if (nativeTrace.length !== wasmTrace.length) {
    return [`witness lengths differ: native ${nativeTrace.length}, wasm ${wasmTrace.length}`];
  }
  const reasons = [];
  for (let step = 0; step < nativeTrace.length; step += 1) {
    const native = nativeTrace[step];
    const wasm = wasmTrace[step];
    if (differences(withoutAction(native), withoutAction(wasm)).length > 0) {
      reasons.push(`step ${step} differs outside its action (state, changes, or entry fields)`);
      continue;
    }
    if (differences(native.action, wasm.action).length === 0) continue;
    for (const [side, action] of [["native", native.action], ["wasm", wasm.action]]) {
      const keys = JSON.stringify(Object.keys(action).sort());
      if (keys !== JSON.stringify(["loc", "name", "params"])) {
        reasons.push(`step ${step} ${side} action carries fields replay cannot check: ${keys}`);
        continue;
      }
      let locations;
      try {
        locations = await oracle.actionLocations();
      } catch (error) {
        reasons.push(`step ${step}: native action locations unavailable: ${error.message}`);
        continue;
      }
      const expected = locations.get(action.name);
      if (!expected || differences(action.loc, expected).length > 0) {
        reasons.push(`step ${step} ${side} action '${action.name}' loc ${JSON.stringify(action.loc)} `
          + `is not its native source location ${JSON.stringify(expected ?? null)}`);
      }
    }
  }
  if (reasons.length > 0) return reasons;
  for (const [side, trace] of [["native", nativeTrace], ["wasm", wasmTrace]]) {
    const replayed = await oracle.replay(replayTrace(spec, trace));
    if (replayed.status !== 0 || replayed.envelope?.result !== "conformant"
        || replayed.envelope.steps_checked !== trace.length - 1
        || differences(replayed.envelope.final_state, trace.at(-1).state).length > 0) {
      reasons.push(`${side} witness does not replay on the solver-free Monitor: `
        + JSON.stringify(replayed.envelope ?? { status: replayed.status, stderr: replayed.stderr }));
    }
  }
  return reasons;
}

// A reachable or deadlock witness whose structure differs is accepted only when
// both witnesses have the same length and the same entries apart from the
// action, every differing action sits at its native source location, and both
// replay as conformant on the solver-free Monitor. Equal state sequences make
// both witnesses reach the same goal or deadlock state at the same step.
export async function reconcileWitnesses(native, wasm, oracle) {
  const normalizedNative = normalizeEnvelope(native);
  const normalizedWasm = normalizeEnvelope(wasm);
  const unresolved = [];
  const replayed = [];
  for (const site of replayableWitnessSites(native, wasm)) {
    if (differences(site.get(normalizedNative), site.get(normalizedWasm)).length === 0) continue;
    const reasons = await witnessDisagreements(native.spec, site.get(native), site.get(wasm), oracle);
    if (reasons.length === 0) {
      site.set(normalizedWasm, structuredClone(site.get(normalizedNative)));
      replayed.push(site.path);
    } else {
      unresolved.push(...reasons.map((reason) => ({ path: site.path, reason })));
    }
  }
  return { differences: [...differences(normalizedNative, normalizedWasm), ...unresolved], replayed };
}

// #1789: positive and rejecting controls for reconcileWitnesses, replayed by
// the real native CLI on WITNESS_REPLAY_FIXTURE.
export async function assertWitnessReplayContract({ verify, oracleFor }) {
  const native = await verify(WITNESS_REPLAY_FIXTURE);
  const twins = { auto_approve: "approve", approve: "auto_approve" };
  const sites = ["$.deadlock.trace", "$.reachables.Approved.witness"];
  const witnesses = (envelope) => [envelope.deadlock?.trace, envelope.reachables?.Approved?.witness];
  requireCondition(
    native.result === "verified" && witnesses(native).every((trace) => Array.isArray(trace)
      && trace.length === 3 && trace[2].action?.name in twins),
    "fixture witnesses must end in one of the state-identical actions",
    native,
  );
  const locations = await oracleFor(WITNESS_REPLAY_FIXTURE).actionLocations();
  const action = (name) => ({ name, params: {}, loc: locations.get(name) });
  const rewrite = (mutate) => {
    const alternate = structuredClone(native);
    for (const trace of witnesses(alternate)) mutate(trace[2], trace[1]);
    return alternate;
  };
  const reconcile = (alternate) => reconcileWitnesses(native, alternate, oracleFor(WITNESS_REPLAY_FIXTURE));
  const rejectedAt = (result, pattern) => sites.every((site) => result.differences.some(
    (difference) => difference.path === site && pattern.test(difference.reason ?? ""),
  ));

  const swapped = rewrite((entry) => { entry.action = action(twins[entry.action.name]); });
  requireCondition(
    differences(normalizeEnvelope(native), normalizeEnvelope(swapped)).length > 0,
    "the structural comparison alone must still report a state-identical action swap",
    swapped,
  );
  const accepted = await reconcile(swapped);
  requireCondition(
    accepted.differences.length === 0 && JSON.stringify(accepted.replayed) === JSON.stringify(sites),
    "a replay-valid state-identical action swap must be accepted",
    accepted,
  );

  const disabled = rewrite((entry) => { entry.action = action("submit"); });
  const disabledResult = await reconcile(disabled);
  requireCondition(
    disabledResult.replayed.length === 0
      && rejectedAt(disabledResult, /does not replay.*"kind":"requires_failed"/),
    "a witness action that is not enabled at its step must fail parity on its guard",
    disabledResult,
  );

  const generated = rewrite((entry) => {
    entry.action = { ...action(twins[entry.action.name]), origin: { dialect: "spec" } };
  });
  const generatedResult = await reconcile(generated);
  requireCondition(
    generatedResult.replayed.length === 0
      && rejectedAt(generatedResult, /carries fields replay cannot check/),
    "a differing witness action with metadata replay cannot check must fail parity",
    generatedResult,
  );

  const changed = rewrite((entry) => {
    entry.action = action(twins[entry.action.name]);
    entry.changes = { ...entry.changes, declined: { from: false, to: false } };
  });
  const changedResult = await reconcile(changed);
  requireCondition(
    changedResult.replayed.length === 0 && rejectedAt(changedResult, /differs outside its action/),
    "a witness whose changes differ while its states agree must fail parity",
    changedResult,
  );

  const declined = rewrite((entry, previous) => {
    entry.action = action("decline");
    entry.state = { ...previous.state, declined: true };
    entry.changes = { declined: { from: false, to: true } };
  });
  const declinedReplay = await oracleFor(WITNESS_REPLAY_FIXTURE).replay(
    replayTrace(native.spec, declined.reachables.Approved.witness),
  );
  requireCondition(
    declinedReplay.envelope?.result === "conformant",
    "the different-state control must itself replay, so only the state comparison rejects it",
    declinedReplay,
  );
  const declinedResult = await reconcile(declined);
  requireCondition(
    declinedResult.replayed.length === 0 && rejectedAt(declinedResult, /differs outside its action/),
    "a replay-valid witness that reaches a different state must fail parity",
    declinedResult,
  );

  const moved = rewrite((entry) => { entry.action.loc = { ...entry.action.loc, line: 99 }; });
  const movedResult = await reconcile(moved);
  requireCondition(
    movedResult.replayed.length === 0 && rejectedAt(movedResult, /is not its native source location/),
    "a witness action location must equal the native source location of that action",
    movedResult,
  );
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
  // A reachable witness location drift must reach reconcileWitnesses, which
  // accepts it only at the action's native source location
  // (assertWitnessReplayContract).
  const reachableEnvelope = { ...base, reachables: { Chosen: { witness: traceEnvelope.trace } } };
  const movedReachable = structuredClone(reachableEnvelope);
  movedReachable.reachables.Chosen.witness[1].action.loc.line = 99;
  requireCondition(
    differences(normalizeEnvelope(reachableEnvelope), normalizeEnvelope(movedReachable))[0]?.path
      === "$.reachables.Chosen.witness.steps[0].action.loc.line",
    "reachable witness locations must not be normalized",
    movedReachable,
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
