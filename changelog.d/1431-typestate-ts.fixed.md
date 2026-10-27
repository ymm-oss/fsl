Fixed (#1431): TypeScript typestate skeletons now escape ECMAScript and strict-mode
reserved action and parameter names (including `eval` and `arguments`), keep
function/signature identifiers distinct, and avoid capturing model types with the
phantom generic. Native output follows the frozen reference except for these
identifier rules and model-derived parameter types instead of blanket `number`.
