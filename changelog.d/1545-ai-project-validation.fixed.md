Fixed (#1545): native `check` and the shared Worker check stages validate every
AI-project component before reporting success. Undeclared authority tools and
unknown hard-contract rules now produce the same error diagnostic and status 2
as project `ai check`, rather than an analyzed-project success.
