Fixed (Refs #1467): symbolic scalar/enum Set/Seq expressions use shared checked
inferred/expected types, including empty equality operands and conditional arms.
Private lexical type metadata preserves raw `none`/composite values and the public
Rust `Bindings<T>` alias. Carrier, capacity and storage-bound rules are unchanged;
remaining composite/relation gaps stay tracked by #1467. State/storage restrictions
do not imply rejection of every composite expression literal. This does not claim
whole-corpus engine agreement or validation of a later integrated source state.
