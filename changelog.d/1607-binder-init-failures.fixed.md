Fixed (#1607): BMC and induction name the first reached concrete failure inside
quantified and aggregate init expressions, matching explicit and auto. Diagnostics
respect candidate membership, filters, quantifier short-circuiting, and each
running sum's overflow while preserving the containing init statement's location.
