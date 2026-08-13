<!-- SPDX-License-Identifier: Apache-2.0 -->
# AI project in literate Markdown

```fsl
ai_component EnvelopeParityLiterateProject {
  model model_v1;
  prompt prompt_v1;
  input Request;
  output Response;
}

dataset EnvelopeParityRecords {
  source "records.jsonl"
}

statistical_property EnvelopeParityStatistical {
  target EnvelopeParityLiterateProject
  dataset EnvelopeParityRecords
  require min_samples >= 0
}

observed_property EnvelopeParityObserved {
  target EnvelopeParityLiterateProject
  source "records.jsonl"
}

ai_migration EnvelopeParityMigration {
  preserve {
    no_regression {
      metric accuracy drop <= 0
    }
  }
}
```
