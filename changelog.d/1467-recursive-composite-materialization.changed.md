Changed (#1467): already-admitted recursive Set/Seq expressions share checked
expected-type construction, equality, conditional joins, methods and binders,
including Struct, Option/nested absence, nested collections, relation and existing
Map values. Sets use logical deduplication; Seq keeps order, duplicates and live
length. Carrier/capacity, presence, partial-operation and storage-bound rules stay
unchanged; temporaries are not clipped. No grammar, admission or Map syntax is added.
**Breaking public Rust library API:** `SymbolicValue::Collection.entries` changes
from `Vec<(Vec<T>, T)>` to `Vec<(Vec<SymbolicValue<T>>, T)>`; Rust constructor and
field consumers must adapt to typed recursive keys. The private CollectionEntries
alias does not hide this public break. The exact `Bindings<T>` BTreeMap alias,
root export and SymbolicValue variant census remain unchanged, not every Rust API.
This is not a CLI/Kernel JSON schema or assurance change; existing errors,
locations, invalid-enum projection and solver-free engine boundaries remain.
