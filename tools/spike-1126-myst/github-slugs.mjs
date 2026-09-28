// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

// Spike experiment (#1126): can a MyST plugin make heading identifiers match
// GitHub's slug algorithm, so the anchors that already work on github.com keep
// resolving under mystmd?  See docs/DESIGN-myst-spike.md.

function text(node) {
  if (node.value) return node.value;
  if (!node.children) return '';
  return node.children.map(text).join('');
}

function githubSlug(s) {
  return s
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s_-]/gu, '')
    .replace(/\s/g, '-');
}

function headings(node, out) {
  if (!node || typeof node !== 'object') return out;
  if (node.type === 'heading') out.push(node);
  (node.children || []).forEach((child) => headings(child, out));
  return out;
}

const githubSlugTransform = {
  name: 'github-slug-headings',
  doc: 'Re-identify headings with GitHub-compatible slugs',
  stage: 'document',
  plugin: () => (tree, vfile) => {
    const seen = new Map();
    headings(tree, []).forEach((node) => {
      const base = githubSlug(text(node));
      if (!base) return;
      const n = seen.get(base) || 0;
      seen.set(base, n + 1);
      const slug = n === 0 ? base : `${base}-${n}`;
      node.identifier = slug;
      node.html_id = slug;
      node.label = slug;
      node.implicit = false;
    });
  },
};


// Second experiment: mystmd resolves `other.md#anchor` against a PROJECT-GLOBAL
// label namespace, so an anchor that exists in some other document resolves
// silently (row L3c).  Can a plugin close that by resolving the anchor against
// the target file itself?

import fs from 'node:fs';
import path from 'node:path';

const anchorCache = new Map();

function anchorsOf(file) {
  if (anchorCache.has(file)) return anchorCache.get(file);
  let source;
  try {
    source = fs.readFileSync(file, 'utf8');
  } catch {
    anchorCache.set(file, null);
    return null;
  }
  const seen = new Map();
  const set = new Set();
  let fence = null;
  for (const raw of source.split('\n')) {
    const opener = /^\s{0,3}(```|~~~)/.exec(raw);
    if (fence) {
      if (opener) fence = null;
      continue;
    }
    if (opener) {
      fence = opener[1];
      continue;
    }
    const h = /^#{1,6}\s+(.+?)\s*#*\s*$/.exec(raw);
    if (!h) continue;
    const base = githubSlug(h[1].replace(/`+([^`]*)`+/g, '$1'));
    if (!base) continue;
    const n = seen.get(base) || 0;
    seen.set(base, n + 1);
    set.add(n === 0 ? base : `${base}-${n}`);
  }
  anchorCache.set(file, set);
  return set;
}

function links(node, out) {
  if (!node || typeof node !== 'object') return out;
  if (node.type === 'link' || node.type === 'crossReference') out.push(node);
  (node.children || []).forEach((c) => links(c, out));
  return out;
}

const crossFileAnchorTransform = {
  name: 'cross-file-anchor-resolves',
  stage: 'document',
  plugin: () => (tree, vfile) => {
    const dir = path.dirname(vfile.path || '.');
    links(tree, []).forEach((node) => {
      const url = node.urlSource || node.url;
      if (typeof url !== 'string') return;
      const m = /^([^:#?]+\.md)#(.+)$/.exec(url);
      if (!m) return;
      const set = anchorsOf(path.resolve(dir, m[1]));
      if (set === null) return; // missing file is link-resolves' job
      if (!set.has(decodeURIComponent(m[2]).toLowerCase())) {
        const msg = vfile.message(`Anchor "#${m[2]}" is not a heading in ${m[1]}`, node);
        msg.fatal = true;
        msg.ruleId = 'cross-file-anchor-resolves';
      }
    });
  },
};

const plugin = {
  name: 'GitHub-compatible heading slugs',
  transforms: [githubSlugTransform, crossFileAnchorTransform],
};
export default plugin;