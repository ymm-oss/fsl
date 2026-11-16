// SPDX-License-Identifier: Apache-2.0
//
// Spike (#1908): translate an `fslc kernel` (Public Kernel v1) JSON document
// into an abstract Dafny module. The module holds the transition system
// (state, init, guards, updates) as executable functions and declares, without
// bodies, the proof obligations "init establishes the invariants" and "every
// action preserves them". A separate proof module refines it and supplies the
// lemma bodies; Dafny decides whether they hold.
//
// Fails closed: any Kernel construct this spike does not translate is an error.
//
// usage: node gen.mjs <kernel.json> [--widen T1,T2] > Spec.gen.dfy

import { readFileSync } from "node:fs";

class Unsupported extends Error {}
const fail = (msg) => {
  throw new Unsupported(msg);
};

const KEYWORDS = new Set([
  "seq", "set", "map", "multiset", "in", "var", "if", "then", "else", "match",
  "case", "function", "method", "lemma", "predicate", "datatype", "type", "int",
  "nat", "bool", "real", "string", "old", "fresh", "forall", "exists", "returns",
  "requires", "ensures", "decreases", "reads", "modifies", "assert", "assume",
  "module", "import", "include", "class", "const", "ghost", "null", "this",
  "true", "false", "new", "print", "return", "while", "invariant", "label",
  "calc", "reveal", "opaque", "static", "Config", "State", "Option", "Some",
  "None", "Wf", "Inv", "Init", "c", "st", "st2",
]);

function ident(name) {
  if (!/^[A-Za-z][A-Za-z0-9_]*$/.test(name)) fail(`identifier ${JSON.stringify(name)}`);
  if (KEYWORDS.has(name)) fail(`identifier ${name} collides with a Dafny or generated name`);
  return name;
}

export function translate(kernel, widen = []) {
  if (kernel.schema_version !== "1.0.0") fail(`kernel schema ${kernel.schema_version}`);
  const sem = kernel.semantics;
  const expectSem = {
    assignment: "simultaneous",
    reads: "pre_state",
    requires_false: "not_enabled",
    failure_state: "rollback",
    old: "pre_state",
    integer_division: "euclidean",
  };
  for (const [k, v] of Object.entries(expectSem)) {
    if (sem[k] !== v) fail(`semantics.${k} = ${sem[k]}`);
  }

  // ---- types -------------------------------------------------------------
  const types = new Map();
  for (const t of kernel.types) {
    const d = t.definition;
    if (d.kind !== "domain") fail(`type ${t.name}: definition kind ${d.kind}`);
    types.set(t.name, { name: ident(t.name), lo: d.lo, hi: d.hi, widened: false });
  }
  for (const w of widen) {
    const t = types.get(w);
    if (!t) fail(`--widen ${w}: no such domain type`);
    if (t.lo !== 0) fail(`--widen ${w}: lower bound ${t.lo} (only 0..hi widens)`);
    t.widened = true;
  }
  const widened = [...types.values()].filter((t) => t.widened);

  const consts = new Map();
  for (const k of kernel.constants) {
    if (k.type.kind !== "int") fail(`constant ${k.name}: type ${k.type.kind}`);
    consts.set(k.name, ident(k.name));
  }

  const domainOf = (ty) => {
    if (ty.kind !== "named") fail(`expected a domain type, got ${ty.kind}`);
    const t = types.get(ty.name);
    if (!t) fail(`unknown type ${ty.name}`);
    return t;
  };
  const lower = (t) => (t.widened ? "0" : String(t.lo));
  const size = (t) => (t.widened ? `c.N_${t.name}` : String(t.hi - t.lo + 1));
  // Range test written inline so Dafny can bound compiled quantifiers.
  const range = (t, x) =>
    t.widened ? `0 <= ${x} < c.N_${t.name}` : `${t.lo} <= ${x} <= ${t.hi}`;
  const offset = (t, x) => (lower(t) === "0" ? x : `(${x}) - ${lower(t)}`);

  const dty = (ty) => {
    switch (ty.kind) {
      case "named":
        domainOf(ty);
        return "int";
      case "int":
        return "int";
      case "bool":
        return "bool";
      case "option":
        return `Option<${dty(ty.item)}>`;
      case "map":
        domainOf(ty.key);
        return `seq<${dty(ty.value)}>`;
      default:
        return fail(`type kind ${ty.kind}`);
    }
  };

  // Well-formedness: shape of every map and the implicit type bounds of every value.
  let wfVar = 0;
  const bound = (ty, e) => {
    switch (ty.kind) {
      case "named":
        return range(domainOf(ty), e);
      case "int":
      case "bool":
        return "true";
      case "option":
        return `(${e}.Some? ==> (${bound(ty.item, `${e}.value`)}))`;
      case "map": {
        const k = `k${wfVar++}`;
        return `(|${e}| == ${size(domainOf(ty.key))} && forall ${k} :: 0 <= ${k} < |${e}| ==> (${bound(ty.value, `${e}[${k}]`)}))`;
      }
      default:
        return fail(`type kind ${ty.kind}`);
    }
  };

  const stateVars = new Map();
  for (const v of kernel.state) stateVars.set(v.name, { name: ident(v.name), type: v.type });

  // ---- expressions -------------------------------------------------------
  const helpers = [];
  let fresh = 0;

  // env: Map<FSL name, {dafny, type}>; ctx.st = state expression; ctx.old = pre-state expression
  function expr(e, env, ctx) {
    switch (e.kind) {
      case "num":
        return String(e.value);
      case "bool":
        return e.value ? "true" : "false";
      case "var": {
        if (env.has(e.name)) return env.get(e.name).dafny;
        if (consts.has(e.name)) return consts.get(e.name);
        if (stateVars.has(e.name)) return `${ctx.st}.${stateVars.get(e.name).name}`;
        return fail(`unbound name ${e.name}`);
      }
      case "index": {
        const ct = e.collection.type;
        if (ct.kind !== "map") fail(`index into ${ct.kind}`);
        return `${expr(e.collection, env, ctx)}[${offset(domainOf(ct.key), expr(e.index, env, ctx))}]`;
      }
      case "some":
        return `Some(${expr(e.operand, env, ctx)})`;
      case "none":
        return "None";
      case "old":
        if (!ctx.old) fail("old() outside ensures");
        return expr(e.operand, env, { ...ctx, st: ctx.old, old: null });
      case "is": {
        if (e.pattern.kind === "none") return `${expr(e.operand, env, ctx)}.None?`;
        if (e.pattern.kind !== "some") fail(`pattern ${e.pattern.kind}`);
        // A binding is visible only to the right operand of an enclosing `and`
        // (handled there) or to later guards (handled by the action).
        if (e.pattern.binding) fail(`'is some(${e.pattern.binding})' binding outside a conjunction`);
        return `${expr(e.operand, env, ctx)}.Some?`;
      }
      case "binary":
        return binary(e, env, ctx);
      case "unary":
        if (e.operator === "not") return `!(${expr(e.operand, env, ctx)})`;
        if (e.operator === "-") return `-(${expr(e.operand, env, ctx)})`;
        return fail(`unary ${e.operator}`);
      case "forall":
      case "exists": {
        const t = domainOf(e.binder.type);
        const x = `${ident(e.binder.name)}_${fresh++}`;
        const env2 = new Map(env).set(e.binder.name, { dafny: x, type: e.binder.type });
        const body = expr(e.body, env2, ctx);
        return e.kind === "forall"
          ? `(forall ${x}: int :: ${range(t, x)} ==> (${body}))`
          : `(exists ${x}: int :: ${range(t, x)} && (${body}))`;
      }
      case "count":
        return count(e, env, ctx);
      default:
        return fail(`expression kind ${e.kind}`);
    }
  }

  const OPS = {
    "==": "==", "!=": "!=", "<": "<", "<=": "<=", ">": ">", ">=": ">=",
    "+": "+", "-": "-", "*": "*", "/": "/", "%": "%",
    and: "&&", or: "||", implies: "==>",
  };
  function binary(e, env, ctx) {
    const op = OPS[e.operator];
    if (!op) fail(`binary operator ${e.operator}`);
    if (e.operator === "/" || e.operator === "%") fail("division needs a partial-operation obligation (not in this spike)");
    // `X is some(v) and B`: v is bound in B.
    if (e.operator === "and" && e.left.kind === "is" && e.left.pattern.binding) {
      const x = expr(e.left.operand, env, ctx);
      const v = `${ident(e.left.pattern.binding)}_${fresh++}`;
      const env2 = new Map(env).set(e.left.pattern.binding, { dafny: v, type: e.left.operand.type.item });
      return `(${x}.Some? && (var ${v} := ${x}.value; ${expr(e.right, env2, ctx)}))`;
    }
    return `(${expr(e.left, env, ctx)} ${op} ${expr(e.right, env, ctx)})`;
  }

  // count(x: T where P) becomes a recursive helper over the range of T, closed
  // over the free variables of P. Recursion (not set cardinality) so it compiles
  // to a loop and its proofs go by induction.
  function count(e, env, ctx) {
    if (ctx.old) fail("count inside ensures (not in this spike)");
    const t = domainOf({ kind: "named", name: e.domain });
    // Close over only the free variables the condition uses, renamed p0..pn,
    // so structurally equal counts share one helper.
    const used = new Set();
    const walk = (x) => {
      if (Array.isArray(x)) x.forEach(walk);
      else if (x && typeof x === "object") {
        if (x.kind === "var" && env.has(x.name) && x.name !== e.binding) used.add(x.name);
        Object.values(x).forEach(walk);
      }
    };
    walk(e.condition ?? fail("count without condition"));
    const free = [...env.entries()].filter(([k]) => used.has(k));
    const env2 = new Map(free.map(([k, v], n) => [k, { dafny: `p${n}`, type: v.type }]));
    env2.set(e.binding, { dafny: "i", type: { kind: "named", name: e.domain } });
    const saved = fresh;
    fresh = 0;
    const body = expr(e.condition, env2, { st: "st" });
    fresh = saved;
    const hi = t.widened ? `c.N_${t.name}` : String(t.hi + 1);
    const params = free.map(([, v], n) => `, p${n}: ${dty(v.type)}`).join("");
    const key = `${t.name}|${params}|${body}`;
    let h = helpers.find((x) => x.key === key);
    if (!h) {
      const name = `Count${helpers.length}`;
      const rec = free.map((_, n) => `, p${n}`).join("");
      h = {
        key,
        name,
        text:
          `  function ${name}(c: Config, st: State${params}, i: int): nat\n` +
          `    requires Wf(c, st) && ${lower(t)} <= i\n` +
          `    decreases ${hi} - i\n` +
          `  {\n` +
          `    if i >= ${hi} then 0\n` +
          `    else (if ${body} then 1 else 0) + ${name}(c, st${rec}, i + 1)\n` +
          `  }`,
      };
      helpers.push(h);
    }
    const args = free.map(([, v]) => `, ${v.dafny}`).join("");
    return `${h.name}(c, ${ctx.st}${args}, ${lower(t)})`;
  }

  // ---- init --------------------------------------------------------------
  const initFields = new Map();
  for (const s of kernel.init.statements) {
    if (s.kind === "assign" && s.target.kind === "var") {
      if (initFields.has(s.target.name)) fail(`init assigns ${s.target.name} twice`);
      initFields.set(s.target.name, expr(s.value, new Map(), { st: "INIT_HAS_NO_STATE" }));
    } else if (s.kind === "forall" && s.statements.length === 1) {
      const a = s.statements[0];
      if (a.kind !== "assign" || a.target.kind !== "index")
        fail("init forall body must be one indexed assignment");
      if (a.target.index.kind !== "var" || a.target.index.name !== s.binder.name)
        fail("init forall must assign at the bound index");
      const name = a.target.name;
      if (initFields.has(name)) fail(`init assigns ${name} twice`);
      const t = domainOf(s.binder.type);
      const k = `k_${fresh++}`;
      const key = lower(t) === "0" ? k : `${k} + ${lower(t)}`;
      const env = new Map([[s.binder.name, { dafny: `(${key})`, type: s.binder.type }]]);
      initFields.set(name, `seq(${size(t)}, ${k} => ${expr(a.value, env, { st: "INIT_HAS_NO_STATE" })})`);
    } else {
      fail(`init statement ${s.kind}`);
    }
  }
  for (const v of stateVars.keys()) if (!initFields.has(v)) fail(`init does not assign ${v}`);
  for (const text of initFields.values()) if (text.includes("INIT_HAS_NO_STATE")) fail("init reads state");

  // ---- properties --------------------------------------------------------
  const props = kernel.properties;
  for (const [k, v] of Object.entries(props)) {
    if (["invariants", "reachables"].includes(k)) continue;
    if (Array.isArray(v) && v.length) fail(`properties.${k} (not in this spike)`);
  }
  const invariants = props.invariants.map((p) => ({
    name: `Inv_${ident(p.name)}`,
    body: expr(p.expression, new Map(), { st: "st" }),
  }));

  // ---- actions -----------------------------------------------------------
  const actions = kernel.actions.map((a) => {
    if (a.lets.length) fail(`${a.name}: let (not in this spike)`);
    if (a.partial_operations.length) fail(`${a.name}: partial operations (not in this spike)`);
    if (a.update_semantics !== "simultaneous") fail(`${a.name}: update semantics ${a.update_semantics}`);
    const name = ident(a.name);
    const params = a.parameters.map((p) => {
      if (!p.type || p.type.kind !== "named") fail(`${a.name}.${p.name}: parameter type`);
      return { fsl: p.name, dafny: ident(p.name), type: p.type, dom: domainOf(p.type) };
    });
    const sig = params.map((p) => `${p.dafny}: int`).join(", ");
    const args = params.map((p) => p.dafny).join(", ");
    const inRange = params.map((p) => range(p.dom, p.dafny));
    let env = new Map(params.map((p) => [p.fsl, { dafny: p.dafny, type: p.type }]));

    // Guards in order; an `is some(x)` guard binds x for the guards after it and for the updates.
    const bindings = []; // [dafny name, expression] in order
    const guardText = [];
    for (const g of a.guards) {
      if (g.kind !== "requires") fail(`${a.name}: guard kind ${g.kind}`);
      const ge = g.expression;
      if (ge.kind === "is" && ge.pattern.kind === "some" && ge.pattern.binding) {
        const x = expr(ge.operand, env, { st: "st" });
        const v = ident(ge.pattern.binding);
        guardText.push({ cond: `${x}.Some?`, bind: [v, `${x}.value`] });
        env = new Map(env).set(ge.pattern.binding, { dafny: v, type: ge.operand.type.item });
        bindings.push([v, `${x}.value`]);
      } else {
        guardText.push({ cond: expr(ge, env, { st: "st" }) });
      }
    }
    // Fold right so each binding scopes over the remaining guards.
    let enabled = "true";
    for (let k = guardText.length - 1; k >= 0; k--) {
      const g = guardText[k];
      enabled = g.bind ? `${g.cond} && (var ${g.bind[0]} := ${g.bind[1]}; ${enabled})` : `${g.cond} && ${enabled}`;
    }

    const byVar = new Map();
    for (const u of a.updates) {
      if (u.kind !== "assign") fail(`${a.name}: update ${u.kind}`);
      if (!["var", "index"].includes(u.target.kind)) fail(`${a.name}: update target ${u.target.kind}`);
      const tv = u.target.name;
      if (!stateVars.has(tv)) fail(`${a.name}: update target ${tv}`);
      if (byVar.has(tv)) fail(`${a.name}: two updates to ${tv} (not in this spike)`);
      const value = expr(u.value, env, { st: "st" });
      if (u.target.kind === "var") byVar.set(tv, value);
      else {
        const key = offset(domainOf(stateVars.get(tv).type.key), expr(u.target.index, env, { st: "st" }));
        byVar.set(tv, `st.${stateVars.get(tv).name}[${key} := ${value}]`);
      }
    }
    const upd = [...byVar.entries()].map(([v, x]) => `${stateVars.get(v).name} := ${x}`).join(", ");
    const lets = bindings.map(([v, x]) => `var ${v} := ${x}; `).join("");
    const apply = `${lets}st.(${upd})`;

    const ensures = a.ensures.map((x) => {
      const ens = expr(x, env, { st: "st2", old: "st" });
      return bindings.length ? `(${bindings.map(([v, b]) => `var ${v} := ${b}; `).join("")}${ens})` : ens;
    });
    return { name, sig, args, inRange, enabled, apply, ensures };
  });

  // ---- emit --------------------------------------------------------------
  const mod = ident(kernel.spec.name);
  const out = [];
  out.push(`// GENERATED by rust/spikes/dafny-core/gen.mjs from ${kernel.spec.source.file}. Do not edit.`);
  out.push(`// widened: ${widened.map((t) => t.name).join(", ") || "(none)"}`);
  out.push(`abstract module ${mod}Spec {`);
  out.push(`  datatype Option<T> = None | Some(value: T)`);
  out.push(`  datatype Config = Config(${widened.map((t) => `N_${t.name}: nat`).join(", ")})`);
  for (const [, d] of consts) {
    const k = kernel.constants.find((x) => x.name === d);
    out.push(`  const ${d}: int := ${k.value}`);
  }
  out.push(`  datatype State = State(${[...stateVars.values()].map((v) => `${v.name}: ${dty(v.type)}`).join(", ")})`);
  out.push("");
  out.push(`  predicate Wf(c: Config, st: State) {`);
  out.push(`    true`);
  for (const v of stateVars.values()) out.push(`    && ${bound(v.type, `st.${v.name}`)}`);
  out.push(`  }`);
  out.push("");
  out.push(`  function Init(c: Config): State {`);
  out.push(`    State(${[...stateVars.keys()].map((v) => initFields.get(v)).join(", ")})`);
  out.push(`  }`);
  out.push("");
  for (const inv of invariants) {
    out.push(`  predicate ${inv.name}(c: Config, st: State) requires Wf(c, st) {`);
    out.push(`    ${inv.body}`);
    out.push(`  }`);
  }
  out.push(`  predicate Inv(c: Config, st: State) {`);
  out.push(`    Wf(c, st)${invariants.map((i) => ` && ${i.name}(c, st)`).join("")}`);
  out.push(`  }`);
  out.push("");
  for (const a of actions) {
    const pre = ["Wf(c, st)", ...a.inRange].join(" && ");
    const sig = `c: Config, st: State${a.sig ? ", " + a.sig : ""}`;
    const call = `c, st${a.args ? ", " + a.args : ""}`;
    out.push(`  predicate ${a.name}_Enabled(${sig})`);
    out.push(`    requires ${pre}`);
    out.push(`  {`);
    out.push(`    ${a.enabled}`);
    out.push(`  }`);
    out.push(`  function ${a.name}_Apply(${sig}): State`);
    out.push(`    requires ${pre} && ${a.name}_Enabled(${call})`);
    out.push(`  {`);
    out.push(`    ${a.apply}`);
    out.push(`  }`);
    // Total entry point for callers outside Dafny: it checks every precondition
    // itself, so a TypeScript call site cannot reach unproved behaviour.
    out.push(`  function ${a.name}_Step(${sig}): Option<State> {`);
    out.push(`    if ${pre} && ${a.name}_Enabled(${call}) then Some(${a.name}_Apply(${call})) else None`);
    out.push(`  }`);
    out.push(`  lemma ${a.name}_StepPreserves(${sig})`);
    out.push(`    requires Inv(c, st)`);
    out.push(`    ensures var r := ${a.name}_Step(${call}); r.Some? ==> Inv(c, r.value)`);
    out.push(`  {`);
    out.push(`    if ${pre} && ${a.name}_Enabled(${call}) { ${a.name}_Preserves(${call}); }`);
    out.push(`  }`);
    out.push("");
  }
  for (const h of helpers) out.push(h.text, "");
  // Accessors for the JS harness: Dafny erases a one-field datatype, so the
  // harness never touches the State representation directly.
  const vars = [...stateVars.values()];
  out.push(`  function Mk(${vars.map((v) => `${v.name}: ${dty(v.type)}`).join(", ")}): State { State(${vars.map((v) => v.name).join(", ")}) }`);
  for (const v of vars) out.push(`  function Get_${v.name}(st: State): ${dty(v.type)} { st.${v.name} }`);
  out.push("");
  out.push(`  // Proof obligations. Bodies come from the refining proof module.`);
  out.push(`  lemma InitEstablishes(c: Config) ensures Inv(c, Init(c))`);
  for (const a of actions) {
    const sig = `c: Config, st: State${a.sig ? ", " + a.sig : ""}`;
    const call = `c, st${a.args ? ", " + a.args : ""}`;
    out.push(`  lemma ${a.name}_Preserves(${sig})`);
    out.push(`    requires ${["Inv(c, st)", ...a.inRange, `${a.name}_Enabled(${call})`].join(" && ")}`);
    out.push(`    ensures Inv(c, ${a.name}_Apply(${call}))`);
    for (const e of a.ensures) out.push(`    ensures var st2 := ${a.name}_Apply(${call}); ${e}`);
  }
  out.push(`}`);
  return out.join("\n") + "\n";
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const args = process.argv.slice(2);
  const file = args.find((a) => !a.startsWith("--"));
  const wi = args.indexOf("--widen");
  const widen = wi >= 0 ? args[wi + 1].split(",").filter(Boolean) : [];
  try {
    process.stdout.write(translate(JSON.parse(readFileSync(file, "utf8")), widen));
  } catch (err) {
    if (err instanceof Unsupported) {
      process.stderr.write(`unsupported: ${err.message}\n`);
      process.exit(2);
    }
    throw err;
  }
}
