Fixed (#1424): testgen preserves wide Kotlin integer literals and compares integral
leaf values by value, rejects lossy Vitest integers with a located error, escapes
Python string controls, and safely escapes source basenames in every target's
headers without changing ordinary output outside the Kotlin helper.
