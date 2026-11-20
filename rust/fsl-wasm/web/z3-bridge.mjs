// SPDX-License-Identifier: Apache-2.0

import { init as initZ3 } from "z3-solver/build/browser.js";

let em;

export async function installZ3Bridge() {
  const initialized = await initZ3();
  em = initialized.em;
  const ctx = new initialized.Context("fsl-wasm");
  const { Array: Z3Array, Bool, If, Int, Solver } = ctx;
  const solver = new Solver();
  solver.set("unsat_core", true);
  solver.set("random_seed", 0);
  solver.set("smt.random_seed", 0);
  globalThis.fslZ3Version = () => initialized.getFullVersion();
  const terms = [null];
  const sorts = [null];
  const sortDescriptors = [null];
  const sortHandles = new Map();

  const register = (term) => {
    terms.push(term);
    return terms.length - 1;
  };
  const term = (handle) => {
    const value = terms[Number(handle)];
    if (!value) throw new Error(`unknown FSL Z3 term handle ${handle}`);
    return value;
  };
  const splitArray = (descriptor) => {
    const body = descriptor.slice(6, -1);
    let depth = 0;
    for (let index = 0; index < body.length; index += 1) {
      const character = body[index];
      if (character === "(") depth += 1;
      else if (character === ")") depth -= 1;
      else if (character === "," && depth === 0) {
        return [body.slice(0, index), body.slice(index + 1)];
      }
    }
    throw new Error(`invalid array sort descriptor: ${descriptor}`);
  };
  const makeSort = (descriptor) => {
    if (descriptor === "bool") return Bool.sort();
    if (descriptor === "int") return Int.sort();
    if (descriptor.startsWith("array(") && descriptor.endsWith(")")) {
      const [domain, range] = splitArray(descriptor);
      return Z3Array.sort(makeSort(domain), makeSort(range));
    }
    throw new Error(`unknown FSL Z3 sort descriptor: ${descriptor}`);
  };

  globalThis.fslZ3Sort = (descriptor) => {
    if (sortHandles.has(descriptor)) return sortHandles.get(descriptor);
    sorts.push(makeSort(descriptor));
    sortDescriptors.push(descriptor);
    const handle = sorts.length - 1;
    sortHandles.set(descriptor, handle);
    return handle;
  };
  globalThis.fslZ3BoolValue = (value) => register(Bool.val(Boolean(value)));
  globalThis.fslZ3IntValue = (value) => register(Int.val(value.toString()));
  globalThis.fslZ3Constant = (name, sortHandle) => {
    const sort = sorts[Number(sortHandle)];
    const descriptor = sortDescriptors[Number(sortHandle)];
    if (descriptor === "bool") return register(Bool.const(name));
    if (descriptor === "int") return register(Int.const(name));
    if (descriptor?.startsWith("array(")) {
      return register(Z3Array.const(name, sort.domain(), sort.range()));
    }
    throw new Error(`unsupported constant sort for ${name}`);
  };
  globalThis.fslZ3Unary = (operation, handle) => {
    const value = term(handle);
    if (operation === "not") return register(value.not());
    if (operation === "neg") return register(value.neg());
    throw new Error(`unknown unary Z3 operation ${operation}`);
  };
  globalThis.fslZ3Binary = (operation, leftHandle, rightHandle) => {
    const left = term(leftHandle);
    const right = term(rightHandle);
    const operations = {
      implies: () => left.implies(right),
      eq: () => left.eq(right),
      add: () => left.add(right),
      sub: () => left.sub(right),
      mul: () => left.mul(right),
      div: () => left.div(right),
      mod: () => left.mod(right),
      lt: () => left.lt(right),
      le: () => left.le(right),
      gt: () => left.gt(right),
      ge: () => left.ge(right),
      select: () => left.select(right),
    };
    const invoke = operations[operation];
    if (!invoke) throw new Error(`unknown binary Z3 operation ${operation}`);
    return register(invoke());
  };
  globalThis.fslZ3Nary = (operation, rawHandles) => {
    const values = [...rawHandles].map(term);
    if (operation === "and") return register(ctx.And(...values));
    if (operation === "or") return register(ctx.Or(...values));
    if (operation === "store") {
      return register(values[0].store(values[1], values[2]));
    }
    throw new Error(`unknown n-ary Z3 operation ${operation}`);
  };
  globalThis.fslZ3Ite = (condition, thenTerm, elseTerm) =>
    register(If(term(condition), term(thenTerm), term(elseTerm)));
  globalThis.fslZ3ConstArray = (domain, value) =>
    register(Z3Array.K(sorts[Number(domain)], term(value)));
  globalThis.fslZ3Substitute = (handle, rawFrom, rawTo) => {
    const substitutions = [...rawFrom].map((from, index) => [
      term(from),
      term(rawTo[index]),
    ]);
    return register(ctx.substitute(term(handle), ...substitutions));
  };
  // #1940: the model read for output, re-solved on a fresh context, until
  // the session moves; and the assumptions of the last check, which it
  // satisfies too. The native backend does the same
  // (`fsl_solver_z3::Z3Solver::resolve_output_model`).
  let outputModel = null;
  let lastAssumptions = [];
  const moved = () => {
    outputModel = null;
  };
  globalThis.fslZ3Push = () => {
    moved();
    solver.push();
  };
  globalThis.fslZ3Pop = (levels) => {
    moved();
    solver.pop(Number(levels));
  };
  globalThis.fslZ3Reset = () => {
    moved();
    lastAssumptions = [];
    solver.reset();
  };
  globalThis.fslZ3Assert = (handle) => {
    moved();
    solver.add(term(handle));
  };
  globalThis.fslZ3AssertAndTrack = (handle, tracker) => {
    moved();
    solver.addAndTrack(term(handle), term(tracker));
  };
  globalThis.fslZ3Check = async (rawAssumptions) => {
    moved();
    lastAssumptions = [...rawAssumptions].map(term);
    return String(await solver.check(...lastAssumptions));
  };
  // Solve the current assertions and the last check's assumptions alone on a
  // new context, configured as the native re-solve is (seed 0), and read the
  // following model values from its model, translated back. The context is
  // deleted afterwards: the high-level API never frees one.
  globalThis.fslZ3ResolveOutputModel = async () => {
    moved();
    const Z3 = initialized.Z3;
    const config = Z3.mk_config();
    const fresh = Z3.mk_context_rc(config);
    Z3.del_config(config);
    try {
      const freshSolver = Z3.mk_solver(fresh);
      Z3.solver_inc_ref(fresh, freshSolver);
      const params = Z3.mk_params(fresh);
      Z3.params_inc_ref(fresh, params);
      Z3.params_set_uint(fresh, params, Z3.mk_string_symbol(fresh, "random_seed"), 0);
      Z3.params_set_uint(fresh, params, Z3.mk_string_symbol(fresh, "smt.random_seed"), 0);
      Z3.solver_set_params(fresh, freshSolver, params);
      Z3.params_dec_ref(fresh, params);
      for (const assertion of [...solver.assertions(), ...lastAssumptions]) {
        const translated = Z3.translate(ctx.ptr, assertion.ast, fresh);
        Z3.inc_ref(fresh, translated);
        Z3.solver_assert(fresh, freshSolver, translated);
        Z3.dec_ref(fresh, translated);
      }
      const answer = await Z3.solver_check(fresh, freshSolver);
      if (answer === 1) {
        const model = Z3.solver_get_model(fresh, freshSolver);
        Z3.model_inc_ref(fresh, model);
        outputModel = new ctx.Model(Z3.model_translate(fresh, model, ctx.ptr));
        Z3.model_dec_ref(fresh, model);
      } else if (answer === -1) {
        Z3.solver_dec_ref(fresh, freshSolver);
        throw new Error(
          "the assertions of a satisfiable check are unsatisfiable in a fresh Z3 context",
        );
      }
      // `unknown` (0): the reads keep the session's model.
      Z3.solver_dec_ref(fresh, freshSolver);
    } finally {
      Z3.del_context(fresh);
    }
  };
  globalThis.fslZ3Statistics = () => {
    const values = new Map(
      [...solver.statistics()].map(({ key, value }) => [key, value]),
    );
    const first = (...keys) => keys.map((key) => values.get(key)).find((value) => value !== undefined);
    const directPropagations = first("propagations");
    const satPropagations = [
      first("sat propagations 2ary"),
      first("sat propagations nary"),
    ].filter((value) => value !== undefined);
    const propagations =
      directPropagations ??
      (satPropagations.length > 0
        ? satPropagations.reduce((sum, value) => sum + value, 0)
        : Number.NaN);
    return Float64Array.of(
      first("conflicts", "sat conflicts") ?? Number.NaN,
      first("decisions", "sat decisions") ?? Number.NaN,
      propagations,
      first("memory", "max memory") ?? Number.NaN,
    );
  };
  globalThis.fslZ3UnsatCore = () =>
    [...solver.unsatCore()].map((entry) => {
      const existing = terms.findIndex(
        (candidate) => candidate && candidate.eqIdentity(entry),
      );
      return existing >= 0 ? existing : register(entry);
    });
  globalThis.fslZ3ModelEval = (handle, boolean) => {
    const value = (outputModel ?? solver.model()).eval(term(handle), true);
    return boolean ? ctx.isTrue(value) : Number(value.value());
  };
}

export async function terminateSolverThreads() {
  em?.PThread?.terminateAllThreads?.();
}
