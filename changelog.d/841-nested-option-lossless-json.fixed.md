Fixed (#841): ordinary trace, replay, testgen, and Worker JSON no longer collapse
`none` and `some(none)` for a nested `Option` state value; the tagged form
`{"kind":"some","value":…}` is introduced only where the declared payload is
itself an `Option`, so every previously supported `Option<scalar>` byte is
unchanged, and trace changes are computed from typed values so a struct's own
`kind`/`value` fields are never mistaken for an `Option` tag.
