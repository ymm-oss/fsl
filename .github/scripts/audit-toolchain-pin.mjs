// SPDX-License-Identifier: Apache-2.0

// Every `dtolnay/rust-toolchain` reference under `.github/workflows` must name
// one agreed Rust version, so an upstream `stable` release cannot turn `main`
// red with no repository change (issue #847's live instance: rustc 1.98.0 was
// released 2026-08-18 and the scheduled product gate began failing on a
// zero-line diff).
//
// `release.yml` is deliberately exempt: it pins the MSRV declared by
// `rust/Cargo.toml`'s `rust-version`, not the development toolchain, and it
// pins the action by commit SHA rather than by version branch. Its exemption is
// declared here by exact path so that renaming or removing that workflow is a
// visible change rather than a silent widening.

import { readdir, readFile } from "node:fs/promises";

export const ACTION = "dtolnay/rust-toolchain";

export const EXPECTED_REF = "1.98.0";

// Exempt path => the reason it is exempt. An entry here is a declared decision,
// not a wildcard: a path not listed is audited.
export const DECLARED_EXEMPTIONS = Object.freeze({
  "release.yml":
    "pins the MSRV from rust/Cargo.toml's rust-version, by action commit SHA, " +
    "not the development toolchain",
});

// A `uses:` key, with or without the list dash. The dash is optional because
// `- name:` followed by a sibling `uses:` is ordinary YAML and requiring the
// dash made every step written that way invisible to this audit -- measured,
// not hypothesised.
const USES_KEY_PATTERN = /^\s*(?:-\s*)?uses:\s*(.+?)\s*$/;

/** Strip one layer of matching YAML quotes, if present. */
function unquote(value) {
  const match = /^(['"])(.*)\1$/.exec(value);
  return match ? match[2] : value;
}

/**
 * Every `uses:` reference to the toolchain action in one workflow's text.
 *
 * The value is unquoted first: `uses: "dtolnay/rust-toolchain@stable"` is valid
 * YAML and equivalent to the bare form, so a pattern anchored on the bare form
 * alone would let a quoted floating channel through.
 *
 * Comments are not `uses:` lines and are not collected here; `auditReferences`
 * checks them separately.
 */
export function collectReferences(fileName, text) {
  const references = [];
  const prefix = `${ACTION}@`;
  text.split("\n").forEach((line, index) => {
    const match = USES_KEY_PATTERN.exec(line);
    if (!match) {
      return;
    }
    // A trailing `# comment` on the same line is not part of the value. Only
    // strip it when whitespace precedes the `#`, so a `#` inside the ref is
    // preserved rather than silently truncating the ref we are about to judge.
    const value = unquote(match[1].replace(/\s+#.*$/, "").trim());
    if (!value.startsWith(prefix)) {
      return;
    }
    references.push({
      file: fileName,
      line: index + 1,
      ref: value.slice(prefix.length),
    });
  });
  return references;
}

/**
 * Every comment line that names the action with an explicit `@ref`. These are
 * cited justifications -- `merge-readiness.yml` explains its restore-only cache
 * key by naming the action and ref that produce the matching key -- so a ref
 * bump that leaves them behind makes the citation disagree with the code.
 */
export function collectCitations(fileName, text) {
  const citations = [];
  const pattern = new RegExp(`${ACTION.replace("/", "\\/")}@([\\w.-]+)`);
  text.split("\n").forEach((line, index) => {
    if (!line.trimStart().startsWith("#")) {
      return;
    }
    const match = pattern.exec(line);
    if (match) {
      citations.push({ file: fileName, line: index + 1, ref: match[1] });
    }
  });
  return citations;
}

/**
 * Audit one workflow's collected references and citations.
 *
 * Returns a finding per disagreement. An empty array means the workflow agrees
 * with `EXPECTED_REF`, or is a declared exemption.
 */
export function auditReferences({ references, citations, expectedRef, exemptions }) {
  const findings = [];
  const declared = exemptions ?? DECLARED_EXEMPTIONS;
  const expected = expectedRef ?? EXPECTED_REF;

  for (const reference of references) {
    if (Object.hasOwn(declared, reference.file)) {
      continue;
    }
    // A ref computed at run time cannot be statically shown to be pinned, and
    // `@${{ matrix.toolchain }}` with `stable` in the matrix reintroduces
    // exactly the drift this audit exists to prevent. Fail closed rather than
    // pass something unverifiable.
    if (reference.ref.includes("${{")) {
      findings.push({
        class: "expression-ref",
        file: reference.file,
        line: reference.line,
        ref: reference.ref,
        message:
          `${reference.file}:${reference.line} resolves ${ACTION} through the ` +
          `expression \`${reference.ref}\`. A run-time ref cannot be shown to be ` +
          `pinned, and a matrix containing \`stable\` would reintroduce the drift ` +
          `this audit prevents; write @${expected} literally.`,
      });
      continue;
    }
    if (reference.ref === "stable" || reference.ref === "nightly" || reference.ref === "beta") {
      findings.push({
        class: "floating-channel",
        file: reference.file,
        line: reference.line,
        ref: reference.ref,
        message:
          `${reference.file}:${reference.line} uses ${ACTION}@${reference.ref}. ` +
          "A floating channel lets an upstream Rust release turn `main` red with " +
          `no repository change; pin it to ${expected} (see issue #847).`,
      });
      continue;
    }
    if (reference.ref !== expected) {
      findings.push({
        class: "ref-disagreement",
        file: reference.file,
        line: reference.line,
        ref: reference.ref,
        message:
          `${reference.file}:${reference.line} uses ${ACTION}@${reference.ref} but ` +
          `every audited workflow must use @${expected}. Swatinem/rust-cache keys ` +
          "include the rustc version, so a split toolchain silently breaks " +
          "merge-readiness.yml's restore-only key match against ci.yml.",
      });
    }
  }

  for (const citation of citations) {
    if (Object.hasOwn(declared, citation.file)) {
      continue;
    }
    if (citation.ref !== expected) {
      findings.push({
        class: "stale-citation",
        file: citation.file,
        line: citation.line,
        ref: citation.ref,
        message:
          `${citation.file}:${citation.line} cites ${ACTION}@${citation.ref} as the ` +
          `reason its cache key matches, but the workflows now use @${expected}. ` +
          "The citation must name what the code actually does.",
      });
    }
  }

  return findings;
}

/** Audit every workflow file in `directory`. */
export async function auditWorkflowDirectory(directory, options = {}) {
  const entries = await readdir(directory, { withFileTypes: true });
  const declared = options.exemptions ?? DECLARED_EXEMPTIONS;
  const findings = [];
  let audited = 0;
  let exempted = 0;

  for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
    if (!entry.isFile() || !/\.ya?ml$/.test(entry.name)) {
      continue;
    }
    const text = await readFile(new URL(entry.name, directory), "utf8");
    const references = collectReferences(entry.name, text);
    const citations = collectCitations(entry.name, text);
    if (references.length === 0 && citations.length === 0) {
      continue;
    }
    // Count exempt references separately. Reporting them as "at the expected
    // ref" would be false -- release.yml is pinned by commit SHA to the MSRV.
    if (Object.hasOwn(declared, entry.name)) {
      exempted += references.length;
    } else {
      audited += references.length;
    }
    findings.push(...auditReferences({ references, citations, ...options }));
  }

  return { audited, exempted, findings };
}

async function main() {
  const directory = new URL("../workflows/", import.meta.url);
  const { audited, exempted, findings } = await auditWorkflowDirectory(directory);

  if (findings.length > 0) {
    for (const finding of findings) {
      console.error(`FAIL -- ${finding.message}`);
    }
    process.exitCode = 1;
    return;
  }

  const exempt = Object.keys(DECLARED_EXEMPTIONS).join(", ");
  console.log(
    `rust toolchain pin: PASS -- ${audited} audited reference(s) at @${EXPECTED_REF}, ` +
      `${exempted} exempt reference(s) in ${exempt}`,
  );
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) {
  await main();
}
