Fixed (#1345): Cache replay preserves stored f64 values and JSON tokens via
serde_json `float_roundtrip`, without changing cache shape, schema, or keys.
Document sweep cache provenance as execution metadata: stable comparisons
exclude root `cost`/`cache` per verification envelope, preserving domain state;
use `FSLC_CACHE=off` to disable caching rather than a sweep `--no-cache` flag.
