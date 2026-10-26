Fixed (#1345): Cache replay preserves stored f64 values and JSON tokens via
serde_json `float_roundtrip`, without changing cache shape, schema, or keys.
Document sweep cache provenance as execution metadata: stable comparisons
exclude root `cost`/`cache` per verification envelope, preserving domain state;
use `FSLC_CACHE=off` to disable caching rather than a sweep `--no-cache` flag.
Accurate JSON f64 reading applies workspace-wide, including Worker requests,
scenario approval normalization, AI eval/JSONL, causal evidence/plan/lifecycle,
and log replay/implementation-log/event inputs; older scenario approval or causal
digests containing boundary decimal values can change. The fslc/fsl-tools test
graphs already enabled this feature via the `jsonschema` dev-dependency, so tests
and distributed builds previously parsed such values differently.
