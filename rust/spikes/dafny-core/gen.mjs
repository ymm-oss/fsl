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
// Types declared `symmetric` are widened to a size parameter; --widen adds more.
//
// usage: node gen.mjs <kernel.json> [--widen T1,T2] [--liveness-by-fslc] [--name N] > Spec.gen.dfy

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

// Dafny keywords: a struct field or enum member may not be one (they are
// always qualified or accessed as members, so generated names do not clash).
const RESERVED = new Set([...KEYWORDS].filter((k) => !["Config", "State", "Option", "Some", "None", "Wf", "Inv", "Init", "c", "st", "st2"].includes(k)));
function member(name) {
  if (!/^[A-Za-z][A-Za-z0-9_]*$/.test(name)) fail(`identifier ${JSON.stringify(name)}`);
  if (RESERVED.has(name)) fail(`identifier ${name} is a Dafny keyword`);
  return name;
}

function ident(name) {
  if (!/^[A-Za-z][A-Za-z0-9_]*$/.test(name)) fail(`identifier ${JSON.stringify(name)}`);
  if (KEYWORDS.has(name)) fail(`identifier ${name} collides with a Dafny or generated name`);
  return name;
}

// The types proved for every size: each `symmetric type` of the spec, plus the
// ones named with --widen (for specs that do not mark their identity types).
export function widenedTypes(kernel, widen = []) {
  const names = kernel.types.filter((t) => t.symmetric).map((t) => t.name);
  for (const w of widen) if (!names.includes(w)) names.push(w);
  return kernel.types.map((t) => t.name).filter((n) => names.includes(n)).concat(names.filter((n) => !kernel.types.some((t) => t.name === n)));
}

// opts.livenessByFslc: accept `leadsTo` properties without translating them.
// Dafny proves only the safety obligations; the caller (core.mjs) must have
// fslc verify the liveness ones, and the generated header names them.
export function translate(kernel, widen = [], opts = {}) {
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
  const types = new Map(); // domain types
  const enums = new Map(); // name -> { name, members }
  const structs = new Map(); // name -> { name, fields: [{ name, type }] }
  const members = new Map(); // enum member -> enum name
  for (const t of kernel.types) {
    const d = t.definition;
    if (d.kind === "domain") types.set(t.name, { name: ident(t.name), lo: d.lo, hi: d.hi, widened: false });
    else if (d.kind === "enum") {
      enums.set(t.name, { name: ident(t.name), members: d.members.map(member) });
      for (const m of d.members) {
        if (members.has(m)) fail(`enum member ${m} declared twice`);
        members.set(m, t.name);
      }
    } else if (d.kind === "struct") {
      structs.set(t.name, { name: ident(t.name), fields: d.fields.map((f) => ({ name: member(f.name), type: f.type })) });
    } else fail(`type ${t.name}: definition kind ${d.kind}`);
  }
  // A named type is a domain (an int range), an enum, or a struct.
  const namedKind = (ty) => {
    if (ty.kind !== "named") return null;
    if (types.has(ty.name)) return "domain";
    if (enums.has(ty.name)) return "enum";
    if (structs.has(ty.name)) return "struct";
    return fail(`unknown type ${ty.name}`);
  };
  for (const w of widenedTypes(kernel, widen)) {
    const t = types.get(w);
    if (!t) fail(`--widen ${w}: no such domain type`);
    if (t.lo !== 0) fail(`widen ${w}: lower bound ${t.lo} (only 0..hi widens)`);
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
        if (namedKind(ty) === "domain") return "int";
        return (enums.get(ty.name) ?? structs.get(ty.name)).name;
      case "int":
        return "int";
      case "bool":
        return "bool";
      case "option":
        return `Option<${dty(ty.item)}>`;
      case "map":
        domainOf(ty.key);
        return `seq<${dty(ty.value)}>`;
      case "seq":
        return `seq<${dty(ty.item)}>`;
      case "set":
        return `set<${dty(ty.item)}>`;
      case "domain": // an inline range type, e.g. a `forall i in 0..3` binder
        return "int";
      default:
        return fail(`type kind ${ty.kind}`);
    }
  };

  // Well-formedness: shape of every map and the implicit type bounds of every value.
  let wfVar = 0;
  const bound = (ty, e) => {
    switch (ty.kind) {
      case "named": {
        const kind = namedKind(ty);
        if (kind === "domain") return range(domainOf(ty), e);
        if (kind === "enum") return "true";
        const parts = structs.get(ty.name).fields.map((f) => bound(f.type, `${e}.${f.name}`)).filter((x) => x !== "true");
        return parts.length ? parts.map((x) => `(${x})`).join(" && ") : "true";
      }
      case "int":
      case "bool":
        return "true";
      case "option":
        return `(${e}.Some? ==> (${bound(ty.item, `${e}.value`)}))`;
      case "map": {
        const k = `k${wfVar++}`;
        const inner = bound(ty.value, `${e}[${k}]`);
        // A value type without bounds (Bool, Int) needs no quantifier, and an
        // `==> true` one has no trigger, which Dafny warns about.
        if (inner === "true") return `(|${e}| == ${size(domainOf(ty.key))})`;
        return `(|${e}| == ${size(domainOf(ty.key))} && forall ${k} :: 0 <= ${k} < |${e}| ==> (${inner}))`;
      }
      case "seq": {
        // A bounded FIFO: at most `capacity` items, each within its type.
        if (!Number.isInteger(ty.capacity)) fail("Seq without a static capacity");
        const k = `k${wfVar++}`;
        const inner = bound(ty.item, `${e}[${k}]`);
        if (inner === "true") return `(|${e}| <= ${ty.capacity})`;
        return `(|${e}| <= ${ty.capacity} && forall ${k} :: 0 <= ${k} < |${e}| ==> (${inner}))`;
      }
      case "set": {
        const x = `x${wfVar++}`;
        const inner = bound(ty.item, x);
        return inner === "true" ? "true" : `(forall ${x} :: ${x} in ${e} ==> (${inner}))`;
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
        if (members.has(e.name)) return `${enums.get(members.get(e.name)).name}.${e.name}`;
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
        const x = `${ident(e.binder.name)}_${fresh++}`;
        // `forall x: T` ranges over a domain type, `forall i in lo..hi` over an int range.
        const r =
          e.binder.kind === "range"
            ? `${expr(e.binder.lo, env, ctx)} <= ${x} <= ${expr(e.binder.hi, env, ctx)}`
            : range(domainOf(e.binder.type), x);
        const env2 = new Map(env).set(e.binder.name, { dafny: x, type: e.binder.type });
        const body = expr(e.body, env2, ctx);
        return e.kind === "forall" ? `(forall ${x}: int :: ${r} ==> (${body}))` : `(exists ${x}: int :: ${r} && (${body}))`;
      }
      case "count":
      case "sum":
        return aggregate(e, env, ctx);
      case "not":
        return `!(${expr(e.operand, env, ctx)})`;
      case "field": {
        if (namedKind(e.value.type) !== "struct") fail(`field of ${JSON.stringify(e.value.type)}`);
        return `${expr(e.value, env, ctx)}.${member(e.field)}`;
      }
      case "struct_lit": {
        const st = structs.get(e.name) ?? fail(`struct literal of ${e.name}`);
        const given = Object.keys(e.fields);
        if (given.length !== st.fields.length || st.fields.some((f) => !(f.name in e.fields))) fail(`struct literal ${e.name}: fields`);
        return `${st.name}(${st.fields.map((f) => expr(e.fields[f.name], env, ctx)).join(", ")})`;
      }
      case "seq_lit":
        return `[${e.items.map((x) => expr(x, env, ctx)).join(", ")}]`;
      case "set_lit":
        return `{${e.items.map((x) => expr(x, env, ctx)).join(", ")}}`;
      case "method":
        return method(e, env, ctx);
      case "min":
      case "max": {
        if (!e.arguments?.length) fail(`${e.kind} without arguments`);
        const op = e.kind === "min" ? "<=" : ">=";
        return e.arguments.map((a) => expr(a, env, ctx)).reduce((a, b) => `(if ${a} ${op} ${b} then ${a} else ${b})`);
      }
      default:
        return fail(`expression kind ${e.kind}`);
    }
  }

  // Seq and Set methods. A partial one (head/pop/at on a too-short Seq) is a
  // Dafny well-formedness obligation: the generated module verifies only if no
  // guard, update or invariant can reach it, which is FSL's own requirement.
  function method(e, env, ctx) {
    const r = expr(e.receiver, env, ctx);
    const args = e.arguments.map((a) => expr(a, env, ctx));
    const kind = e.receiver.type?.kind;
    const arity = (n) => {
      if (args.length !== n) fail(`${e.method} with ${args.length} arguments`);
    };
    if (kind === "seq") {
      switch (e.method) {
        case "size": arity(0); return `|${r}|`;
        case "push": arity(1); return `(${r} + [${args[0]}])`;
        case "pop": arity(0); return `${r}[1..]`;
        case "head": arity(0); return `${r}[0]`;
        case "at": arity(1); return `${r}[${args[0]}]`;
        case "contains": arity(1); return `(${args[0]} in ${r})`;
      }
    } else if (kind === "set") {
      switch (e.method) {
        case "size": arity(0); return `|${r}|`;
        case "add": arity(1); return `(${r} + {${args[0]}})`;
        case "remove": arity(1); return `(${r} - {${args[0]}})`;
        case "contains": arity(1); return `(${args[0]} in ${r})`;
      }
    }
    return fail(`method ${e.method} on ${kind}`);
  }

  const OPS = {
    "==": "==", "!=": "!=", "<": "<", "<=": "<=", ">": ">", ">=": ">=",
    "+": "+", "-": "-", "*": "*", "/": "/", "%": "%",
    and: "&&", or: "||", "=>": "==>",
  };
  function binary(e, env, ctx) {
    const op = OPS[e.operator];
    if (!op) fail(`binary operator ${e.operator}`);
    // `/` and `%`: Dafny's int division is Euclidean like FSL's, and a zero
    // divisor is a well-formedness obligation, so an unguarded one does not verify.
    // `X is some(v) and B` / `X is some(v) => B`: v is bound in B.
    if ((e.operator === "and" || e.operator === "=>") && e.left.kind === "is" && e.left.pattern.binding) {
      const x = expr(e.left.operand, env, ctx);
      const v = `${ident(e.left.pattern.binding)}_${fresh++}`;
      const env2 = new Map(env).set(e.left.pattern.binding, { dafny: v, type: e.left.operand.type.item });
      return `(${x}.Some? ${e.operator === "and" ? "&&" : "==>"} (var ${v} := ${x}.value; ${expr(e.right, env2, ctx)}))`;
    }
    return `(${expr(e.left, env, ctx)} ${op} ${expr(e.right, env, ctx)})`;
  }

  // count(x: T where P) becomes a recursive helper over the range of T, closed
  // over the free variables of P. Recursion (not set cardinality) so it compiles
  // to a loop and its proofs go by induction.
  function aggregate(e, env, ctx) {
    if (ctx.old) fail(`${e.kind} inside ensures (not in this spike)`);
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
    if (e.kind === "count") walk(e.condition ?? fail("count without condition"));
    else {
      walk(e.value ?? fail("sum without a value"));
      if (e.condition) walk(e.condition);
    }
    const free = [...env.entries()].filter(([k]) => used.has(k));
    const env2 = new Map(free.map(([k, v], n) => [k, { dafny: `p${n}`, type: v.type }]));
    env2.set(e.binding, { dafny: "i", type: { kind: "named", name: e.domain } });
    const saved = fresh;
    fresh = 0;
    const cond = e.condition ? expr(e.condition, env2, { st: "st" }) : null;
    const value = e.kind === "sum" ? expr(e.value, env2, { st: "st" }) : null;
    const body = e.kind === "count" ? cond : cond ? `(if ${cond} then ${value} else 0)` : value;
    fresh = saved;
    const hi = t.widened ? `c.N_${t.name}` : String(t.hi + 1);
    const params = free.map(([, v], n) => `, p${n}: ${dty(v.type)}`).join("");
    const key = `${e.kind}|${t.name}|${params}|${body}`;
    let h = helpers.find((x) => x.key === key);
    if (!h) {
      const name = `${e.kind === "count" ? "Count" : "Sum"}${helpers.length}`;
      const rec = free.map((_, n) => `, p${n}`).join("");
      h = {
        key,
        name,
        text:
          `  function ${name}(c: Config, st: State${params}, i: int): ${e.kind === "count" ? "nat" : "int"}\n` +
          `    requires Wf(c, st) && ${lower(t)} <= i\n` +
          `    decreases ${hi} - i\n` +
          `  {\n` +
          `    if i >= ${hi} then 0\n` +
          `    else ${e.kind === "count" ? `(if ${body} then 1 else 0)` : body} + ${name}(c, st${rec}, i + 1)\n` +
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
    } else if (s.kind === "forall") {
      // Each statement of the body assigns one map at the bound index.
      for (const a of s.statements) {
        if (a.kind !== "assign" || a.target.kind !== "index") fail("init forall body must be indexed assignments");
        if (a.target.index.kind !== "var" || a.target.index.name !== s.binder.name)
          fail("init forall must assign at the bound index");
        const name = a.target.name;
        if (initFields.has(name)) fail(`init assigns ${name} twice`);
        const t = domainOf(s.binder.type);
        const k = `k_${fresh++}`;
        const key = lower(t) === "0" ? k : `${k} + ${lower(t)}`;
        const env = new Map([[s.binder.name, { dafny: `(${key})`, type: s.binder.type }]]);
        initFields.set(name, `seq(${size(t)}, ${k} => ${expr(a.value, env, { st: "INIT_HAS_NO_STATE" })})`);
      }
    } else {
      fail(`init statement ${s.kind}`);
    }
  }
  for (const v of stateVars.keys()) if (!initFields.has(v)) fail(`init does not assign ${v}`);
  for (const text of initFields.values()) if (text.includes("INIT_HAS_NO_STATE")) fail("init reads state");

  // ---- properties --------------------------------------------------------
  const props = kernel.properties;
  const byFslc = [];
  for (const [k, v] of Object.entries(props)) {
    if (["invariants", "reachables"].includes(k)) continue;
    if (k === "leads_to" && opts.livenessByFslc) {
      byFslc.push(...v.map((p) => p.name));
      continue;
    }
    if (Array.isArray(v) && v.length) fail(`properties.${k} (not in this spike${k === "leads_to" ? "; --liveness-by-fslc leaves them to fslc verify" : ""})`);
  }
  const invariants = props.invariants.map((p) => ({
    name: `Inv_${ident(p.name)}`,
    body: expr(p.expression, new Map(), { st: "st" }),
  }));

  // ---- actions -----------------------------------------------------------
  const actions = kernel.actions.map((a) => {
    // a.partial_operations needs no translation: each partial read becomes a
    // Dafny well-formedness obligation where it occurs (see method()).
    if (a.update_semantics !== "simultaneous") fail(`${a.name}: update semantics ${a.update_semantics}`);
    const name = ident(a.name);
    const params = a.parameters.map((p) => {
      if (!p.type || p.type.kind !== "named" || namedKind(p.type) === "struct") fail(`${a.name}.${p.name}: parameter type`);
      return { fsl: p.name, dafny: ident(p.name), type: p.type, dom: namedKind(p.type) === "domain" ? domainOf(p.type) : null };
    });
    const sig = params.map((p) => `${p.dafny}: ${dty(p.type)}`).join(", ");
    const args = params.map((p) => p.dafny).join(", ");
    // An enum parameter is in range by its type.
    const inRange = params.filter((p) => p.dom).map((p) => range(p.dom, p.dafny));
    let env = new Map(params.map((p) => [p.fsl, { dafny: p.dafny, type: p.type }]));

    // Guards in order; an `is some(x)` guard binds x for the guards after it and for the updates.
    const bindings = []; // [dafny name, expression] in order
    const guardText = [];
    const letBound = new Set();
    for (const g of a.guards) {
      // A `let` among the guards binds for the guards after it (and the body).
      if (g.kind === "let") {
        const v = ident(g.name);
        const x = expr(g.value, env, { st: "st" });
        guardText.push({ cond: "true", bind: [v, x] });
        bindings.push([v, x]);
        env = new Map(env).set(g.name, { dafny: v, type: g.value.type });
        letBound.add(g.name);
        continue;
      }
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
    // `let` names bind after the guards, in order, over the pre-state; they are
    // visible to the updates and ensures, like the `is some(x)` bindings.
    for (const l of a.lets) {
      if (typeof l.name !== "string" || !l.value) fail(`${a.name}: let shape`);
      if (letBound.has(l.name)) continue; // already bound in guard order
      const v = ident(l.name);
      bindings.push([v, expr(l.value, env, { st: "st" })]);
      env = new Map(env).set(l.name, { dafny: v, type: l.value.type });
    }
    // Fold right so each binding scopes over the remaining guards.
    let enabled = "true";
    for (let k = guardText.length - 1; k >= 0; k--) {
      const g = guardText[k];
      enabled = g.bind ? `${g.cond} && (var ${g.bind[0]} := ${g.bind[1]}; ${enabled})` : `${g.cond} && ${enabled}`;
    }

    // Updates are simultaneous: every right-hand side and index reads the
    // pre-state st. Each state variable's new value is built by applying its
    // writes in turn; fslc rejects a spec whose writes may hit one location
    // twice on a path, so the order of distinct writes does not matter. An
    // `if` builds both branches from the same base and joins them per variable.
    const pre = (v) => `st.${stateVars.get(v).name}`;
    const writes = (list, base) => {
      const cur = new Map(base);
      const at = (v) => cur.get(v) ?? pre(v);
      for (const u of list) {
        if (u.kind === "if") {
          const c = expr(u.condition, env, { st: "st" });
          const th = writes(u.then ?? [], cur);
          const el = writes(u.else ?? [], cur);
          for (const v of new Set([...th.keys(), ...el.keys()])) {
            const t = th.get(v) ?? at(v);
            const e = el.get(v) ?? at(v);
            if (t !== e) cur.set(v, `(if ${c} then ${t} else ${e})`);
          }
          continue;
        }
        if (u.kind !== "assign") fail(`${a.name}: update ${u.kind}`);
        const value = expr(u.value, env, { st: "st" });
        let tg = u.target;
        let field = null;
        if (tg.kind === "field_lv") {
          field = member(tg.field);
          tg = tg.target;
        }
        if (!["var", "index"].includes(tg.kind)) fail(`${a.name}: update target ${tg.kind}`);
        const tv = tg.name;
        if (!stateVars.has(tv)) fail(`${a.name}: update target ${tv}`);
        const old = at(tv);
        if (tg.kind === "var") cur.set(tv, field ? `${old}.(${field} := ${value})` : value);
        else {
          const key = offset(domainOf(stateVars.get(tv).type.key), expr(tg.index, env, { st: "st" }));
          cur.set(tv, `${old}[${key} := ${field ? `${old}[${key}].(${field} := ${value})` : value}]`);
        }
      }
      return cur;
    };
    const byVar = writes(a.updates, new Map());
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
  // opts.name: the module and file name, for specs that share a spec name.
  const mod = ident(opts.name ?? kernel.spec.name);
  const out = [];
  out.push(`// GENERATED by rust/spikes/dafny-core/gen.mjs from ${kernel.spec.source.file}. Do not edit.`);
  out.push(`// widened: ${widened.map((t) => t.name).join(", ") || "(none)"}`);
  if (byFslc.length) out.push(`// not proved here (left to fslc verify): leadsTo ${byFslc.join(", ")}`);
  out.push(`abstract module ${mod}Spec {`);
  out.push(`  datatype Option<T> = None | Some(value: T)`);
  for (const e of enums.values()) out.push(`  datatype ${e.name} = ${e.members.join(" | ")}`);
  for (const t of structs.values()) out.push(`  datatype ${t.name} = ${t.name}(${t.fields.map((f) => `${f.name}: ${dty(f.type)}`).join(", ")})`);
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
  // The same for Config, structs and enums: Dafny's JS backend erases any
  // datatype with exactly one field, so the wrapper never uses constructors
  // or destructors of generated datatypes directly.
  if (widened.length || structs.size || enums.size) {
    out.push(`  function MkConfig(${widened.map((t) => `N_${t.name}: nat`).join(", ")}): Config { Config(${widened.map((t) => `N_${t.name}`).join(", ")}) }`);
    for (const t of structs.values()) {
      out.push(`  function Mk_${t.name}(${t.fields.map((f) => `${f.name}: ${dty(f.type)}`).join(", ")}): ${t.name} { ${t.name}(${t.fields.map((f) => f.name).join(", ")}) }`);
      for (const f of t.fields) out.push(`  function Get_${t.name}_${f.name}(x: ${t.name}): ${dty(f.type)} { x.${f.name} }`);
    }
    for (const e of enums.values())
      for (const m of e.members) {
        out.push(`  function Mk_${e.name}_${m}(): ${e.name} { ${e.name}.${m} }`);
        out.push(`  function Is_${e.name}_${m}(x: ${e.name}): bool { x.${m}? }`);
      }
  }
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
    const ni = args.indexOf("--name");
    const opts = { livenessByFslc: args.includes("--liveness-by-fslc"), ...(ni >= 0 ? { name: args[ni + 1] } : {}) };
    process.stdout.write(translate(JSON.parse(readFileSync(file, "utf8")), widen, opts));
  } catch (err) {
    if (err instanceof Unsupported) {
      process.stderr.write(`unsupported: ${err.message}\n`);
      process.exit(2);
    }
    throw err;
  }
}
