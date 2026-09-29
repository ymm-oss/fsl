// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

// Spike experiment (#1126, rows F1/F2): a typed `{fsl}` role backed by the FSL
// corpus, to see whether MyST can carry the doc -> FSL forward check that
// #1124 wants, and whether the failure reaches the exit code.
//
// Two reporters are wired deliberately, selected by FSL_REF_REPORTER:
//   role       -- report from inside the role's run(), with fatal = true
//   transform  -- report from a document-stage transform, with fatal = true
// The pair measures the claim that `fatal` is honoured in a transform but not
// in a role.  See docs/design/DESIGN-myst-spike.md.

import fs from 'node:fs';
import path from 'node:path';

const SPEC_DIR = path.resolve(process.cwd(), '..', 'specs');
const REPORTER = process.env.FSL_REF_REPORTER || 'transform';

function loadCorpus() {
  const corpus = new Map();
  for (const name of fs.readdirSync(SPEC_DIR).filter((f) => f.endsWith('.fsl'))) {
    const source = fs.readFileSync(path.join(SPEC_DIR, name), 'utf8');
    const elements = new Set();
    for (const m of source.matchAll(/^\s*(action|invariant|trans|forbidden)\s+([A-Za-z_][A-Za-z0-9_]*)/gm)) {
      elements.add(`${m[1]}:${m[2]}`);
    }
    for (const m of source.matchAll(/^\s*(spec|refinement|domain|dbsystem)\s+([A-Za-z_][A-Za-z0-9_]*)/gm)) {
      elements.add(`${m[1]}:${m[2]}`);
    }
    corpus.set(name.replace(/\.fsl$/, ''), elements);
  }
  return corpus;
}

const CORPUS = loadCorpus();

// `{fsl}`cart_v1/action:add_to_cart`` or `{fsl}`cart_v1``
function resolve(target) {
  const [file, element] = target.split('/');
  if (!CORPUS.has(file)) return `no spec file specs/${file}.fsl`;
  if (!element) return undefined;
  if (!CORPUS.get(file).has(element)) {
    return `specs/${file}.fsl declares no ${element}`;
  }
  return undefined;
}

function report(vfile, node, message) {
  const m = vfile.message(`FSL reference does not resolve: ${message}`, node);
  m.fatal = true;
  m.ruleId = 'fsl-reference-resolves';
}

const fslRole = {
  name: 'fsl',
  body: { type: String, required: true },
  run(data, vfile) {
    const target = data.body.trim();
    const problem = resolve(target);
    const node = { type: 'inlineCode', value: target, data: { fslTarget: target, fslProblem: problem } };
    if (problem && REPORTER === 'role') report(vfile, node, problem);
    return [node];
  },
};

function walk(node, out) {
  if (!node || typeof node !== 'object') return out;
  if (node.data && node.data.fslTarget) out.push(node);
  (node.children || []).forEach((c) => walk(c, out));
  return out;
}

// The link-shaped notation: [text](../specs/cart_v1.fsl#action:add_to_cart).
// Unlike the role, GitHub renders this as an ordinary link, so the same source
// is correct in both renderers.  Measured: docs/design/DESIGN-myst-spike.md.
function linkNodes(node, out) {
  if (!node || typeof node !== 'object') return out;
  if (node.type === 'link') out.push(node);
  (node.children || []).forEach((c) => linkNodes(c, out));
  return out;
}

const fslCheckTransform = {
  name: 'fsl-reference-check',
  stage: 'document',
  plugin: () => (tree, vfile) => {
    if (REPORTER !== 'transform') return;
    walk(tree, []).forEach((node) => {
      if (node.data.fslProblem) report(vfile, node, node.data.fslProblem);
    });
    linkNodes(tree, []).forEach((node) => {
      const url = node.urlSource || node.url;
      if (typeof url !== 'string') return;
      const m = /(?:^|\/)([A-Za-z0-9_.-]+)\.fsl#(.+)$/.exec(url);
      if (!m) return;
      const problem = resolve(`${m[1]}/${decodeURIComponent(m[2])}`);
      if (problem) report(vfile, node, problem);
    });
  },
};

const plugin = { name: 'FSL references', roles: [fslRole], transforms: [fslCheckTransform] };
export default plugin;