Changed (#1543): CLI and Worker JSON messages for concrete init evaluation
failures append ` in init at L:C`, including shared Monitor/replay, scenarios,
and testgen init execution. Existing runtime message stems, error kinds, exit
codes, and JSON fields are preserved; no structured location is added and
unsatisfiable-init messages remain unchanged.
