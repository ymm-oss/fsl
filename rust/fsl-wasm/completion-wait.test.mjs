// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import test from "node:test";

import { classifyOutcome, ProbeTimeoutError } from "./browser-outcome.mjs";
import { completionDeadlineMs, waitForCompletion } from "./completion-wait.mjs";

function fakeClock() {
  // A nonzero epoch also catches confusing an absolute clock with elapsed time.
  const start = 900_000;
  let time = start;
  const sleeps = [];
  return {
    now: () => time,
    elapsed: () => time - start,
    advance: (ms) => { time += ms; },
    sleeps,
    sleep: async (ms) => {
      assert.ok(Number.isFinite(ms) && ms > 0 && ms <= 250, `unexpected sleep: ${ms}`);
      sleeps.push(ms);
      time += ms;
    },
  };
}

function assertPollTimeout(error, elapsedMs, phase) {
  assert.ok(error instanceof ProbeTimeoutError, "use the existing production timeout type");
  assert.equal(error.mode, "poll");
  assert.deepEqual(classifyOutcome(error), { outcome: "probe_timeout", status: 124, mode: "poll" });
  assert.ok(error.message.includes(phase), "timeout reports the last observed client phase");
  const seconds = [...error.message.matchAll(/(\d+(?:\.\d+)?)\s*s(?:ec(?:ond)?s?)?\b/gi)]
    .map((match) => Number(match[1]));
  assert.ok(seconds.some((value) => value >= 360 && Math.abs(value - elapsedMs / 1_000) <= 1),
    `timeout reports elapsed seconds (${elapsedMs / 1_000}), got: ${error.message}`);
  return true;
}

test("the browser completion wall-clock budget is 360 seconds", () => {
  assert.equal(completionDeadlineMs, 360_000);
});

test("completion after 250 seconds and over 360 observations is accepted", async () => {
  const clock = fakeClock();
  let evaluations = 0;
  const completed = { done: "true", ok: "true", text: "finished", phase: "complete" };
  const result = await waitForCompletion({
    deadlineMs: completionDeadlineMs, intervalMs: 250, now: clock.now, sleep: clock.sleep,
    evaluate: async () => {
      evaluations += 1;
      assert.ok(evaluations <= 1_002, "completion wait must sleep, not busy-loop");
      return clock.elapsed() >= 250_000
        ? completed : { done: "false", ok: "false", text: "pending", phase: "verify-batches" };
    },
  });
  assert.deepEqual(result, completed);
  assert.ok(evaluations > 360, "the former observation-count limit must not survive");
  assert.ok(clock.elapsed() >= 250_000 && clock.elapsed() < completionDeadlineMs);
  assert.ok(clock.elapsed() > 171_000, "outlast the previously observed failing invocation");
  assert.equal(clock.sleeps.length, evaluations - 1, "sleep between observations, never after completion");
});

test("never-done observations expire on wall time with phase and status 124", async () => {
  const clock = fakeClock();
  let evaluations = 0;
  let lastPhase;
  await assert.rejects(waitForCompletion({
    deadlineMs: completionDeadlineMs, intervalMs: 250, now: clock.now, sleep: clock.sleep,
    evaluate: async () => {
      evaluations += 1;
      assert.ok(evaluations <= 1_442, "bounded observations require sleeping between polls");
      lastPhase = clock.elapsed() >= 180_000 ? "verify-batch-remaining" : "initializing-worker";
      // Boolean true is not the DOM's completion marker; only the string is.
      return { done: true, ok: "false", text: "pending", phase: lastPhase };
    },
  }), (error) => assertPollTimeout(error, clock.elapsed(), lastPhase));
  assert.equal(lastPhase, "verify-batch-remaining");
  assert.ok(clock.elapsed() >= 360_000 && clock.elapsed() <= 360_250);
  assert.ok(evaluations > 360 && evaluations <= 1_442);
  assert.ok(clock.sleeps.length >= evaluations - 1 && clock.sleeps.length <= evaluations);
});

test("slow evaluations consume the deadline instead of receiving a fresh poll-count budget", async () => {
  const clock = fakeClock();
  let evaluations = 0;
  await assert.rejects(waitForCompletion({
    deadlineMs: completionDeadlineMs, intervalMs: 250, now: clock.now, sleep: clock.sleep,
    evaluate: async () => {
      evaluations += 1;
      assert.ok(evaluations <= 9, "slow evaluate must count toward the wall-clock deadline");
      clock.advance(45_000);
      return { done: "false", ok: "false", text: "pending", phase: "slow-corpus-verification" };
    },
  }), (error) => assertPollTimeout(error, clock.elapsed(), "slow-corpus-verification"));
  assert.ok(clock.elapsed() >= 360_000 && clock.elapsed() <= 405_250,
    "deadline overshoot is bounded by one in-flight evaluation and sleep");
  assert.ok(evaluations <= 9);
  assert.ok(clock.sleeps.length >= evaluations - 1 && clock.sleeps.length <= evaluations);
});

test("an immediate completion returns the observation after one evaluate and no sleep", async () => {
  const clock = fakeClock();
  let evaluations = 0;
  const completed = { done: "true", ok: "false", text: "client error", phase: "failed" };
  const result = await waitForCompletion({
    deadlineMs: completionDeadlineMs, intervalMs: 250, now: clock.now, sleep: clock.sleep,
    evaluate: async () => { evaluations += 1; return completed; },
  });
  assert.deepEqual(result, completed, "completion is not the same as a passing client result");
  assert.equal(evaluations, 1);
  assert.equal(clock.elapsed(), 0);
  assert.deepEqual(clock.sleeps, []);
});

test("an in-flight CDP timeout retains its details and adds elapsed time and the last phase", async () => {
  const clock = fakeClock();
  let evaluations = 0;
  const phase = "verify-final-batch";
  const diagnostic = "CDP Runtime.evaluate timed out; id=17; socket=open; outstanding=1; stderr=fake Chrome";
  await assert.rejects(waitForCompletion({
    deadlineMs: completionDeadlineMs, intervalMs: 250, now: clock.now, sleep: clock.sleep,
    evaluate: async () => {
      evaluations += 1;
      if (evaluations === 1) {
        clock.advance(completionDeadlineMs - 1_000);
        return { done: "false", ok: "false", text: "pending", phase };
      }
      assert.equal(evaluations, 2, "do not retry a failed CDP evaluation");
      clock.advance(30_000); // The call already in flight crosses the completion deadline.
      throw new ProbeTimeoutError("cdp", diagnostic);
    },
  }), (error) => {
    assert.ok(error instanceof ProbeTimeoutError);
    assert.equal(error.mode, "cdp", "a CDP timeout must not be relabeled as poll");
    assert.deepEqual(classifyOutcome(error), { outcome: "probe_timeout", status: 124, mode: "cdp" });
    assert.ok(error.message.includes(diagnostic), "retain the original CDP diagnostic in full");
    assert.ok(error.message.includes(phase), "report the most recent observed client phase");
    const seconds = [...error.message.matchAll(/(\d+(?:\.\d+)?)\s*s(?:ec(?:ond)?s?)?\b/gi)]
      .map((match) => Number(match[1]));
    assert.ok(seconds.some((value) => Math.abs(value - clock.elapsed() / 1_000) <= 1),
      `report elapsed seconds (${clock.elapsed() / 1_000}), got: ${error.message}`);
    return true;
  });
  assert.equal(evaluations, 2);
  assert.equal(clock.elapsed(), 389_250);
  assert.deepEqual(clock.sleeps, [250]);
});
