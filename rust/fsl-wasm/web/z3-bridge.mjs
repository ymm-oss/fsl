// SPDX-License-Identifier: Apache-2.0

import { init as initZ3 } from "z3-solver/build/browser.js";

let em;

export async function installZ3Bridge() {
  const initialized = await initZ3();
  em = initialized.em;
  const ctx = new initialized.Context("fsl-wasm");
  const { Array: Z3Array, Bool, Int, Solver } = ctx;
  const solver = new Solver();
  solver.set("unsat_core", true);
  const terms = [null];
  const sorts = [null];
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
    const handle = sorts.length - 1;
    sortHandles.set(descriptor, handle);
    return handle;
  };
  globalThis.fslZ3BoolValue = (value) => register(Bool.val(Boolean(value)));
  globalThis.fslZ3IntValue = (value) => register(Int.val(value.toString()));
  globalThis.fslZ3Constant = (name, sortHandle) => {
    const sort = sorts[Number(sortHandle)];
    if (sort.__typename === "BoolSort") return register(Bool.const(name));
    if (ctx.isIntSort(sort)) return register(Int.const(name));
    if (ctx.isArraySort(sort)) {
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
    register(term(condition).ite(term(thenTerm), term(elseTerm)));
  globalThis.fslZ3ConstArray = (domain, value) =>
    register(Z3Array.K(sorts[Number(domain)], term(value)));
  globalThis.fslZ3Substitute = (handle, rawFrom, rawTo) => {
    const substitutions = [...rawFrom].map((from, index) => [
      term(from),
      term(rawTo[index]),
    ]);
    return register(ctx.substitute(term(handle), ...substitutions));
  };
  globalThis.fslZ3Push = () => solver.push();
  globalThis.fslZ3Pop = (levels) => solver.pop(Number(levels));
  globalThis.fslZ3Assert = (handle) => solver.add(term(handle));
  globalThis.fslZ3AssertAndTrack = (handle, tracker) =>
    solver.addAndTrack(term(handle), term(tracker));
  globalThis.fslZ3Check = async (rawAssumptions) =>
    String(await solver.check(...[...rawAssumptions].map(term)));
  globalThis.fslZ3UnsatCore = () =>
    [...solver.unsatCore()].map((entry) => {
      const existing = terms.findIndex(
        (candidate) => candidate && candidate.eqIdentity(entry),
      );
      return existing >= 0 ? existing : register(entry);
    });
  globalThis.fslZ3ModelEval = (handle, boolean) => {
    const value = solver.model().eval(term(handle), true);
    return boolean ? ctx.isTrue(value) : Number(value.value());
  };
}

export async function terminateSolverThreads() {
  em?.PThread?.terminateAllThreads?.();
}
