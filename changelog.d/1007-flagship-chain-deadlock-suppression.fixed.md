Fixed (#1007): the business-layer manual pages
(`docs/intro/business-layer.{en,ja}.html`) no longer print the flagship e2e
chain with `--deadlock ignore`. The flag was suppressing a diagnostic that
passes: measured with `--no-cache`, twice, `verify examples/e2e/1_business.fsl
--engine induction` and `verify examples/e2e/2_requirements.fsl --engine
induction` both exit 0 with `result: "proved"` without it, and `2_requirements`
checks a real `deadlock` property while doing so. The pages sat directly under a
callout telling the reader not to weaken business intent to make a check pass.
This covers the site pages only -- the same suppression survives in
`examples/e2e/README.md` and twelve other `examples/` documents, tracked in #979
and #998.
