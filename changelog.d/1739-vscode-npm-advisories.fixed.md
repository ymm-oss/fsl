Fixed (#1739): the VS Code extension `.vsix` no longer ships a vulnerable
`brace-expansion`. The `editors/vscode` lockfile is refreshed with
`npm audit fix` (no `--force`, no direct-dependency or `package.json`
change): the runtime `vscode-languageclient` -> `minimatch@5.1.9` ->
`brace-expansion` moves from 2.1.1 to 2.1.7, clearing its six high
denial-of-service advisories, and the build-only toolchain under
`@vscode/vsce` and `@azure/identity` takes in-range updates of `undici`,
`fast-uri`, `js-yaml`, `markdown-it`, `@azure/msal-node` and `qs`.
`npm audit --omit=dev` now reports no advisory; the full audit drops from
11 (7 high, 4 moderate) to 3 moderate, all in `glob`/`minimatch` under the
build-only `@vscode/vsce@3.9.2`, whose fixed versions lie outside that
release's dependency ranges. The packaged `.vsix` keeps the same 210 files;
only the three `brace-expansion` files change.
