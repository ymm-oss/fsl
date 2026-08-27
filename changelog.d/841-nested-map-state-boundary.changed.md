Changed (#841): reject nested Map state values that native `check` previously
accepted but explicit-state execution could not initialize; the shared CLI/LSP
type hints now describe the recursive nested-Option boundary for struct fields
and state values.
