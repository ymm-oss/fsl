Added (#1344): HTML-only `--no-timing` omits execution timing fields from
stdout and `-o`/`--output` reports for byte-stable comparison of the same
input path, source, options, and toolchain. Default reports retain timings;
source, domain state, counts, verification JSON, and exit codes are unchanged.
