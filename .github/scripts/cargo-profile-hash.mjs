// SPDX-License-Identifier: Apache-2.0
//
// Hash of the `[profile.*]` sections of the workspace root manifest
// (`rust/Cargo.toml`), exported as `FSL_CARGO_PROFILE_HASH` for
// `Swatinem/rust-cache`'s `env-vars` input.
//
// Why this exists: rust-cache v2 (`src/config.ts`) builds its key as
// `<prefix-key>-<shared-key>-<os>-<arch>-<env hash>-<lock hash>`. The env hash
// covers the rustc version and `CARGO*`/`CC*`/`CFLAGS*`/`CXX*`/`CMAKE*`/`RUST*`
// variables plus any `env-vars` prefixes; the lock hash covers
// `.cargo/config.toml`, `rust-toolchain*`, each *member's* `Cargo.toml` and
// `Cargo.lock`. The workspace root here is a virtual manifest, not a member, so
// its `[profile.*]` sections -- the ones cargo actually applies -- are in
// neither hash. After `[profile.dev.package.z3-sys] opt-level = 2` (#1380) every
// run still exact-hit the old Debug-Z3 entry, rebuilt `z3-sys` from source, and
// rust-cache skips saving on an exact hit, so the cache never healed. Feeding
// this hash through `env-vars` makes a profile edit change the env hash (and so
// both the exact key and the restore prefix), so the next run builds against a
// fresh entry and saves it. See docs/design/DESIGN-ci.md, "Actions cache budget",
// "Cache key generation".
//
// Only the profile sections are hashed, not the whole file: `rust/Cargo.toml`
// also carries `[workspace.package] version`, which every release bumps and
// which rust-cache itself deliberately normalises away for members.
//
// Usage: node .github/scripts/cargo-profile-hash.mjs rust/Cargo.toml
// Appends `FSL_CARGO_PROFILE_HASH=<hash>` to `$GITHUB_ENV` when that is set,
// and always prints the same line to stdout. Shell-independent, so the same
// step runs on the Windows runner's default PowerShell.

import { createHash } from "node:crypto";
import { appendFileSync, readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

export const PROFILE_HASH_ENV = "FSL_CARGO_PROFILE_HASH";

// A table header at column 0: `[name]` or `[[name]]`. Indented lines are
// never headers here, so an array element such as `    ["a"]` inside a
// multi-line value cannot be mistaken for one.
const HEADER = /^\[\[?\s*([^\]]+?)\s*\]\]?\s*(#.*)?$/;

/**
 * The normalised text of every `[profile]` / `[profile.*]` table, in file
 * order. Line endings are normalised (a Windows checkout may use CRLF),
 * trailing whitespace is dropped, and blank and full-line comment lines are
 * skipped, so only setting edits change the result.
 */
export function profileSections(manifestText) {
  const kept = [];
  let inProfile = false;
  for (const rawLine of manifestText.split(/\r?\n/)) {
    const line = rawLine.trimEnd();
    const header = HEADER.exec(line);
    if (header) {
      const name = header[1];
      inProfile = name === "profile" || name.startsWith("profile.");
    }
    if (!inProfile) continue;
    const trimmed = line.trim();
    if (trimmed === "" || trimmed.startsWith("#")) continue;
    kept.push(line);
  }
  return kept.join("\n");
}

export function profileHash(manifestText) {
  return createHash("sha256").update(profileSections(manifestText)).digest("hex").slice(0, 16);
}

function main(argv) {
  const manifest = argv[2];
  if (!manifest) {
    process.stderr.write("usage: cargo-profile-hash.mjs <path/to/Cargo.toml>\n");
    return 2;
  }
  const line = `${PROFILE_HASH_ENV}=${profileHash(readFileSync(manifest, "utf8"))}\n`;
  if (process.env.GITHUB_ENV) appendFileSync(process.env.GITHUB_ENV, line);
  process.stdout.write(line);
  return 0;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  process.exitCode = main(process.argv);
}
