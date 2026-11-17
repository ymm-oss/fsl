// SPDX-License-Identifier: Apache-2.0
//
// Spike (#1908) control: stands in for the `claude` CLI in prove.mjs, so the
// proof loop is tested without a model. It ignores its input and replies with
// the file named by FAKE_REPLY inside a ```dafny block.
import { readFileSync } from "node:fs";
process.stdin.resume();
process.stdin.on("end", () => process.stdout.write("```dafny\n" + readFileSync(process.env.FAKE_REPLY, "utf8") + "```\n"));
