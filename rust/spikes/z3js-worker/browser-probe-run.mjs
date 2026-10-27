// SPDX-License-Identifier: Apache-2.0

import { ProbeTimerExpiredError } from "./browser-probe-outcome.mjs";

export async function runBrowserProbe({
  startServer, makeProfile, launchChrome, removeProfile, timeoutMs,
}) {
  let server;
  let profile;
  let chrome;
  let closed;
  let chromeClosed = false;
  let timeout;
  try {
    server = await startServer();
    profile = await makeProfile();
    chrome = launchChrome(profile);
    closed = chrome.closed.then((status) => {
      chromeClosed = true;
      return status;
    }, (error) => {
      chromeClosed = true;
      throw error;
    });
    return await Promise.race([
      server.result,
      closed.then(({ code, signal }) => {
        throw new Error(`Chrome exited before reporting a result (${signal ?? code}): ${chrome.stderr()}`);
      }),
      new Promise((_, reject) => {
        timeout = setTimeout(() => {
          reject(new ProbeTimerExpiredError(
            `Chrome browser probe timed out after ${timeoutMs / 1_000}s: ${chrome.stderr()}`,
          ));
        }, timeoutMs);
      }),
    ]);
  } finally {
    clearTimeout(timeout);
    try {
      if (chrome) {
        if (!chromeClosed) chrome.kill();
        await closed.catch(() => {});
      }
    } finally {
      try {
        if (server) await server.close();
      } finally {
        if (profile !== undefined) await removeProfile(profile);
      }
    }
  }
}
