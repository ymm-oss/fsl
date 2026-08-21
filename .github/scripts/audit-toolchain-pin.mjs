// SPDX-License-Identifier: Apache-2.0

// Every `dtolnay/rust-toolchain` reference under `.github/workflows` must name
// one agreed Rust version, so an upstream `stable` release cannot turn `main`
// red with no repository change (issue #847's live instance: rustc 1.98.0 was
// released 2026-08-18 and the scheduled product gate began failing on a
// zero-line diff).
//
// `release.yml` is held to a different contract rather than skipped: it builds
// the release artifact at the MSRV declared by `rust/Cargo.toml`'s
// `rust-version`, pinned by action commit SHA rather than by version branch, so
// it is audited against that instead of against `EXPECTED_REF`. It is named by
// exact path, so renaming or removing that workflow is a visible change rather
// than a silent widening.

import { readdir, readFile } from "node:fs/promises";

export const ACTION = "dtolnay/rust-toolchain";

export const EXPECTED_REF = "1.98.0";

// Paths held to the MSRV contract instead of the development-toolchain pin.
//
// This is deliberately NOT a skip list. Review probed the earlier version by
// flipping `release.yml` to `@stable` and the audit reported nothing, which made
// "exempt" mean "anything goes here" -- so the MSRV guarantee could have been
// dropped silently, which is the same class of drift this audit exists to catch,
// just on a different axis. An entry here now means: audited against a different
// contract, stated below and enforced by `auditMsrvReference`.
export const DECLARED_EXEMPTIONS = Object.freeze({
  "release.yml":
    "builds the release artifact at the MSRV declared by rust/Cargo.toml's " +
    "rust-version, pinned by action commit SHA rather than by version branch",
});

/** A 40-character git commit SHA, the only ref form that pins action code. */
const COMMIT_SHA = /^[0-9a-f]{40}$/;

/**
 * The MSRV declared by `rust/Cargo.toml`'s `rust-version`, or null when the
 * declaration cannot be found. A null is a finding, not a pass: the contract
 * cannot be checked against a value that is missing.
 */
export function declaredMsrv(cargoTomlText) {
  const match = /^\s*rust-version\s*=\s*"([^"]+)"/m.exec(cargoTomlText);
  return match ? match[1] : null;
}

/**
 * Audit a path held to the MSRV contract rather than to `EXPECTED_REF`.
 *
 * Two things must hold, and both were previously unchecked:
 *   - the action ref is a commit SHA, so the action code itself is pinned;
 *   - the `toolchain:` input names the declared MSRV, so the release artifact
 *     is built at the version the crates claim to support.
 */
export function auditMsrvReference({ file, line, ref, toolchainInput, msrv }) {
  const findings = [];
  if (!COMMIT_SHA.test(ref)) {
    findings.push({
      class: "msrv-ref-not-pinned",
      file,
      line,
      ref,
      message:
        `${file}:${line} uses ${ACTION}@${ref}, but this workflow builds the ` +
        "release artifact and must pin the action by 40-character commit SHA. " +
        "A branch or channel here would let the release toolchain change " +
        "without a reviewed diff.",
    });
  }
  if (msrv === null) {
    findings.push({
      class: "msrv-undeclared",
      file,
      line,
      ref,
      message:
        `${file}:${line} is held to the MSRV contract, but rust/Cargo.toml ` +
        "declares no rust-version to check it against.",
    });
  } else if (toolchainInput === null) {
    findings.push({
      class: "msrv-toolchain-missing",
      file,
      line,
      ref,
      message:
        `${file}:${line} must pass an explicit \`toolchain:\` input naming the ` +
        `declared MSRV ${msrv}; without it the action's own default applies.`,
    });
  } else if (toolchainInput.startsWith("*")) {
    // A YAML alias cannot be resolved by a line scan, and guessing would be
    // worse than saying so. Review probed `toolchain: *msrv` against an anchor
    // defined elsewhere in the file; the earlier version reported
    // `msrv-disagreement`, which named the wrong problem. Fail closed, but with
    // a diagnostic that states what is actually unproven. Resolving aliases
    // needs a real YAML parse -- tracked by #856.
    findings.push({
      class: "msrv-alias-unresolved",
      file,
      line,
      ref,
      message:
        `${file}:${line} passes \`toolchain: ${toolchainInput}\`, a YAML alias. ` +
        "This audit is a line scan and cannot resolve it, so it cannot show the " +
        `release toolchain equals rust-version "${msrv}". Write the version ` +
        "literally, or resolve aliases (see issue #856).",
    });
  } else if (
    toolchainInput !== msrv &&
    !toolchainInput.startsWith(`${msrv}.`)
  ) {
    // `rust-version = "1.88"` is satisfied by `toolchain: 1.88.0`; it is not
    // satisfied by 1.89.0 or by a channel name.
    findings.push({
      class: "msrv-disagreement",
      file,
      line,
      ref,
      message:
        `${file}:${line} passes \`toolchain: ${toolchainInput}\` but ` +
        `rust/Cargo.toml declares rust-version "${msrv}". The release artifact ` +
        "must be built at the version the crates claim to support.",
    });
  }
  return findings;
}

// A `uses:` key, with or without the list dash, and tolerating whitespace
// before the colon.
//
// Every relaxation here was found by probing, not by imagination:
//   - the dash is optional because `- name:` followed by a sibling `uses:` is
//     ordinary YAML, and requiring the dash made every step written that way
//     invisible;
//   - whitespace may precede the colon because `uses : action@ref` is valid
//     YAML whose key is `uses` -- confirmed with a YAML parser, not assumed.
//
// This is a line scan rather than a YAML parse, which is why each of those was
// a hole. Keep that in mind before adding a fifth relaxation: at some point the
// honest fix is to parse the document. It is a line scan today only because the
// required lane is stdlib/Node-only and adding a YAML dependency to it is a
// separate decision (see .github/workflows/cache-budget-audit-wiring.yml for
// why PyYAML lives outside that lane).
const USES_KEY_PATTERN = /^\s*(?:-\s*)?uses\s*:\s*(.+?)\s*$/;

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
 * The `toolchain:` input belonging to each toolchain-action `uses:` line, keyed
 * by that line's one-based number.
 *
 * The key must sit inside the step's `with:` mapping. Review found that an
 * earlier version accepted
 *
 *     - uses: dtolnay/rust-toolchain@<sha>
 *       env:
 *         toolchain: 1.88.0
 *
 * because it matched any later `toolchain:` line. The action receives no input
 * there, so the audit was satisfied by something that does nothing: `env:` and
 * `with:` are different mappings and only `with:` supplies action inputs.
 *
 * Scanning stops at the next `- ` list item, so a following step's `with:`
 * cannot be attributed to this one.
 */
export function collectToolchainInputs(text, references) {
  const lines = text.split("\n");
  const inputs = new Map();

  for (const reference of references) {
    // A step is a list item, so scan from its `- ` line to the next one. The
    // `uses:` key may appear anywhere inside it, including after `with:`.
    let start = reference.line - 1;
    while (start > 0 && !/^\s*-\s/.test(lines[start])) {
      start -= 1;
    }
    let end = reference.line;
    while (end < lines.length && !/^\s*-\s/.test(lines[end])) {
      end += 1;
    }

    let withIndent = null;
    let flowDepth = 0;
    let flowText = "";

    for (let i = start; i < end; i += 1) {
      // Normalise the leading dash so `- with:` is seen at its key's column.
      const raw = lines[i].replace(/^(\s*)-(\s)/, "$1 $2");

      // A flow mapping may span lines, so accumulate until the braces balance.
      if (flowDepth > 0) {
        flowText += " " + raw.trim();
        flowDepth += (raw.match(/\{/g) ?? []).length;
        flowDepth -= (raw.match(/\}/g) ?? []).length;
        if (flowDepth === 0) {
          const entry = /(?:^|,|\{)\s*toolchain\s*:\s*([^,}]+)/.exec(flowText);
          if (entry) {
            inputs.set(reference.line, unquote(entry[1].trim()));
            break;
          }
        }
        continue;
      }

      if (raw.trim() === "" || /^\s*#/.test(raw)) {
        continue;
      }
      const indent = raw.length - raw.trimStart().length;

      const flowStart = /^\s*with\s*:\s*\{/.exec(raw);
      if (flowStart) {
        // Drop a trailing `# comment`, which is not part of the mapping.
        flowText = raw.replace(/\s+#.*$/, "").trim();
        flowDepth =
          (flowText.match(/\{/g) ?? []).length - (flowText.match(/\}/g) ?? []).length;
        if (flowDepth === 0) {
          const entry = /(?:^|,|\{)\s*toolchain\s*:\s*([^,}]+)/.exec(flowText);
          if (entry) {
            inputs.set(reference.line, unquote(entry[1].trim()));
            break;
          }
        }
        continue;
      }

      if (/^\s*with\s*:\s*$/.test(raw)) {
        withIndent = indent;
        continue;
      }
      if (withIndent !== null && indent <= withIndent) {
        withIndent = null;
      }
      if (withIndent === null) {
        continue;
      }
      // A direct child of `with:` is any key indented FURTHER than `with:` but
      // not nested under another key of it. YAML does not mandate two spaces,
      // so the first child's indent defines the level for this mapping.
      const match = /^\s*toolchain\s*:\s*(.+?)\s*$/.exec(raw);
      if (match) {
        const childIndent = firstChildIndent(lines, i, withIndent);
        if (indent === childIndent) {
          inputs.set(reference.line, unquote(match[1].replace(/\s+#.*$/, "").trim()));
          break;
        }
      }
    }
  }
  return inputs;
}

/**
 * The indentation of the first key directly under a `with:` at `withIndent`,
 * searching back from `from`. GitHub does not require two-space indentation, so
 * hardcoding `withIndent + 2` falsely rejected a mapping indented by four.
 */
function firstChildIndent(lines, from, withIndent) {
  let candidate = null;
  for (let i = from; i >= 0; i -= 1) {
    const raw = lines[i].replace(/^(\s*)-(\s)/, "$1 $2");
    if (raw.trim() === "" || /^\s*#/.test(raw)) {
      continue;
    }
    const indent = raw.length - raw.trimStart().length;
    if (indent <= withIndent) {
      break;
    }
    candidate = indent;
  }
  return candidate;
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
export function auditReferences({
  references,
  citations,
  expectedRef,
  exemptions,
  msrv,
  toolchainInputs,
}) {
  const findings = [];
  const declared = exemptions ?? DECLARED_EXEMPTIONS;
  const expected = expectedRef ?? EXPECTED_REF;
  const inputs = toolchainInputs ?? new Map();

  for (const reference of references) {
    if (Object.hasOwn(declared, reference.file)) {
      // Held to the MSRV contract, not skipped.
      //
      // An earlier version skipped this whole branch when `msrv` was undefined.
      // Review showed the consequence: the repository-level suite scanned the
      // workflows without supplying it, so the required lane never ran the MSRV
      // contract at all and a floating `release.yml` passed. Omitting the MSRV
      // is now a finding rather than a silent pass -- a caller that forgets it
      // gets told, instead of getting a green audit of nothing.
      findings.push(
        ...auditMsrvReference({
          file: reference.file,
          line: reference.line,
          ref: reference.ref,
          toolchainInput: inputs.get(reference.line) ?? null,
          msrv: msrv === undefined ? null : msrv,
        }),
      );
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
    const toolchainInputs = collectToolchainInputs(text, references);
    // Count exempt references separately. Reporting them as "at the expected
    // ref" would be false -- release.yml is pinned by commit SHA to the MSRV.
    if (Object.hasOwn(declared, entry.name)) {
      exempted += references.length;
    } else {
      audited += references.length;
    }
    findings.push(
      ...auditReferences({ references, citations, toolchainInputs, ...options }),
    );
  }

  return { audited, exempted, findings };
}

// The repository must always contain at least this many audited references.
// Set from the measured count, deliberately as a floor rather than an equality:
// adding a workflow that installs the toolchain is normal, removing every one of
// them is not.
export const MINIMUM_AUDITED_REFERENCES = 11;

// Each declared MSRV path must actually contain a toolchain reference. Review
// deleted release.yml's toolchain step entirely and the CLI reported
// `PASS -- ... 0 reference(s) held to the MSRV contract`: a declared contract
// with nothing to apply it to is the same vacuous pass the audited floor exists
// to prevent. The required lane already caught it, because its test asserts the
// count; the CLI did not.
export const MINIMUM_MSRV_REFERENCES = Object.keys(DECLARED_EXEMPTIONS).length;

async function main() {
  const directory = new URL("../workflows/", import.meta.url);
  let audited;
  let exempted;
  let findings;
  try {
    // The MSRV contract needs the declaration to check against, so read it here
    // rather than letting a missing file quietly turn that check off.
    const cargoToml = await readFile(
      new URL("../../rust/Cargo.toml", import.meta.url),
      "utf8",
    );
    ({ audited, exempted, findings } = await auditWorkflowDirectory(directory, {
      msrv: declaredMsrv(cargoToml),
    }));
  } catch (error) {
    // An unreadable or missing workflows directory must fail, not be reported as
    // a clean audit.
    console.error(
      `FAIL -- could not read the workflow directory: ${error.message}`,
    );
    process.exitCode = 1;
    return;
  }

  if (findings.length > 0) {
    for (const finding of findings) {
      console.error(`FAIL -- ${finding.message}`);
    }
    process.exitCode = 1;
    return;
  }

  // A pin audit that audited nothing and reported PASS would be the exact
  // defect it exists to prevent: confidence with no evidence behind it.
  if (audited < MINIMUM_AUDITED_REFERENCES) {
    console.error(
      `FAIL -- audited only ${audited} toolchain reference(s); expected at least ` +
        `${MINIMUM_AUDITED_REFERENCES}. Either the workflows moved, or this audit ` +
        "stopped seeing them -- both mean it is no longer enforcing the pin.",
    );
    process.exitCode = 1;
    return;
  }

  if (exempted < MINIMUM_MSRV_REFERENCES) {
    console.error(
      `FAIL -- ${exempted} reference(s) held to the MSRV contract; expected ` +
        `${MINIMUM_MSRV_REFERENCES}, one per declared path ` +
        `(${Object.keys(DECLARED_EXEMPTIONS).join(", ")}). A declared contract ` +
        "with no reference to apply it to enforces nothing.",
    );
    process.exitCode = 1;
    return;
  }

  const msrvPaths = Object.keys(DECLARED_EXEMPTIONS).join(", ");
  console.log(
    `rust toolchain pin: PASS -- ${audited} reference(s) at @${EXPECTED_REF}, ` +
      `${exempted} reference(s) held to the MSRV contract in ${msrvPaths}`,
  );
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split("/").pop())) {
  await main();
}
