// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { exitStatusFor, ProbeTimerExpiredError } from "./browser-probe-outcome.mjs";

const realSetTimeout = globalThis.setTimeout;
const realClearTimeout = globalThis.clearTimeout;

function deferred() {
  let resolve;
  const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve };
}

async function withinBound(promise) {
  let watchdog;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        watchdog = realSetTimeout(() => reject(new Error("probe did not finish within 1000ms")), 1_000);
      }),
    ]);
  } finally {
    realClearTimeout(watchdog);
  }
}

function trackTimers(t) {
  const created = [];
  const expired = new Set();
  const cleared = new Set();
  // Keep real timers: this exercises actual expiry, not a fake timeout rejection.
  t.mock.method(globalThis, "setTimeout", (callback, delay, ...args) => {
    const timer = realSetTimeout(() => {
      expired.add(timer);
      callback(...args);
    }, delay);
    created.push(timer);
    return timer;
  });
  t.mock.method(globalThis, "clearTimeout", (timer) => {
    cleared.add(timer);
    return realClearTimeout(timer);
  });
  t.after(() => {
    // Even a failing cleanup assertion must not leave the test process waiting.
    for (const timer of created) realClearTimeout(timer);
    for (const timer of created) {
      assert.ok(cleared.has(timer), "every acquired probe timer is explicitly cleared");
    }
  });
  return { created, expired };
}

async function fixture(t, overrides = {}, onLaunch = () => {}) {
  // Import before asserting rejection so a missing module cannot masquerade as
  // the ordinary acquisition/launch error expected by a negative control.
  const { runBrowserProbe } = await import("./browser-probe-run.mjs");
  const timers = trackTimers(t);
  const calls = [];
  const result = deferred();
  const closed = deferred();
  const profile = "fake-profile";
  let running = false;
  const closeChrome = (status) => {
    running = false;
    closed.resolve(status);
  };
  const options = {
    startServer: async () => {
      calls.push("startServer");
      return {
        result: result.promise,
        close: async () => {
          assert.equal(running, false, "Chrome is stopped before closing the server");
          calls.push("closeServer");
        },
      };
    },
    makeProfile: async () => { calls.push("makeProfile"); return profile; },
    launchChrome: (path) => {
      calls.push("launchChrome");
      assert.equal(path, profile);
      running = true;
      queueMicrotask(() => onLaunch({ result, closeChrome }));
      return {
        closed: closed.promise,
        kill: () => {
          assert.equal(running, true, "never kill an already closed Chrome or kill it twice");
          calls.push("killChrome");
          closeChrome({ code: null, signal: "SIGKILL" });
        },
        stderr: () => "fake Chrome diagnostic",
      };
    },
    removeProfile: async (path) => {
      assert.equal(path, profile);
      calls.push("removeProfile");
    },
    timeoutMs: 50,
    ...overrides,
  };
  return { run: () => withinBound(runBrowserProbe(options)), calls, timers };
}

function ordinaryError(error) {
  assert.ok(error instanceof Error);
  assert.equal(error instanceof ProbeTimerExpiredError, false);
  assert.equal(exitStatusFor(error), 1);
  return true;
}

test("a posted result is returned and live Chrome, server and profile are cleaned up once", async (t) => {
  const completed = { ok: true, details: { answer: 42 } };
  const f = await fixture(t, {}, ({ result }) => result.resolve(completed));
  assert.deepEqual(await f.run(), completed);
  assert.deepEqual(f.calls, [
    "startServer", "makeProfile", "launchChrome", "killChrome", "closeServer", "removeProfile",
  ]);
});

test("the real result timer rejects with status 124 and finishes cleanup within a bound", async (t) => {
  const f = await fixture(t);
  const started = performance.now();
  await assert.rejects(f.run(), (error) => {
    assert.ok(error instanceof ProbeTimerExpiredError);
    assert.equal(exitStatusFor(error), 124);
    return true;
  });
  assert.ok(performance.now() - started < 1_000, "timer expiry and cleanup must not hang");
  assert.ok(f.timers.created.length > 0, "the probe acquires a real timer");
  assert.ok(f.timers.expired.size > 0, "status 124 follows an actual timer callback, not an immediate throw");
  assert.deepEqual(f.calls, [
    "startServer", "makeProfile", "launchChrome", "killChrome", "closeServer", "removeProfile",
  ]);
});

test("a profile creation rejection closes the server without launching or removing anything", async (t) => {
  const failure = new Error("profile creation failed");
  const f = await fixture(t, { makeProfile: async () => { throw failure; } });
  await assert.rejects(f.run(), (error) => {
    assert.equal(error, failure);
    return ordinaryError(error);
  });
  assert.deepEqual(f.calls, ["startServer", "closeServer"]);
});

test("a synchronous Chrome launch throw closes the server and removes the acquired profile", async (t) => {
  const failure = new Error("Chrome launch threw synchronously");
  const f = await fixture(t, { launchChrome: () => { throw failure; } });
  await assert.rejects(f.run(), (error) => {
    assert.equal(error, failure);
    return ordinaryError(error);
  });
  assert.deepEqual(f.calls, ["startServer", "makeProfile", "closeServer", "removeProfile"]);
});

test("early Chrome close is an ordinary error and does not kill the closed process", async (t) => {
  const f = await fixture(t, {}, ({ closeChrome }) => closeChrome({ code: 7, signal: null }));
  await assert.rejects(f.run(), ordinaryError);
  assert.deepEqual(f.calls, ["startServer", "makeProfile", "launchChrome", "closeServer", "removeProfile"]);
});

test("a server startup rejection acquires no other resources", async (t) => {
  const failure = new Error("server startup failed");
  let starts = 0;
  const f = await fixture(t, { startServer: async () => { starts += 1; throw failure; } });
  await assert.rejects(f.run(), (error) => {
    assert.equal(error, failure);
    return ordinaryError(error);
  });
  assert.equal(starts, 1);
  assert.deepEqual(f.calls, []);
});

test("the real browser entry wires the lifecycle, outcome classifier and 75000ms budget", async () => {
  const source = await readFile(new URL("./browser-probe.mjs", import.meta.url), "utf8");
  assert.ok(/import\s*\{\s*runBrowserProbe\s*\}\s*from\s*["']\.\/browser-probe-run\.mjs["']/.test(source),
    "import the production runBrowserProbe lifecycle");
  assert.ok(/import\s*\{[^}]*\bexitStatusFor\b[^}]*\}\s*from\s*["']\.\/browser-probe-outcome\.mjs["']/.test(source),
    "import the production exitStatusFor classifier");
  assert.ok(/\brunBrowserProbe\(\s*\{[\s\S]*?\btimeoutMs:\s*75_?000\b/.test(source),
    "pass the unchanged 75000ms budget to runBrowserProbe");
  assert.ok(/process\.exitCode\s*=\s*exitStatusFor\(error\)/.test(source),
    "map lifecycle errors through exitStatusFor without forcing process exit");
});
