# z3-solver Worker spike

This Phase-0 spike pins the official `z3-solver` npm package to 4.16.0, the
same version used by the Python reference environment. It proves the critical
term construction → async `check()` → synchronous model evaluation round trip
inside a disposable Worker and records term-construction throughput.

Run from this directory:

```bash
npm ci
npm run probe
npm run probe:browser
npm run bench
```

`probe.mjs` terminates the whole Worker after every request. This is the
portable cancellation boundary: the official package exposes no supported
in-flight solver interruption API. `solver.mjs` also calls Emscripten's
`PThread.terminateAllThreads()` when available after a completed check.

`probe:browser` bundles the official browser entry, copies its separately loaded
Emscripten JS/WASM assets, serves them with COOP/COEP headers, and executes the
same round trip in a dedicated Web Worker under headless Chrome. Set
`CHROME_BIN` when Chrome is not in a standard location.
