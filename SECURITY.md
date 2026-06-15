# Security Policy

## Supported Versions

Security fixes are provided for the latest release (`main` and the most recent tags).

## Reporting a Vulnerability

`fslc` is a verifier for formal specifications, and it parses and executes
untrusted input (`.fsl` files, trace JSON). If you find an issue that could lead
to a parser or evaluator crash, an infinite loop, or arbitrary code execution,
please report it privately **before opening a public issue**.

- Preferred: a private report via GitHub **Security Advisories** (the repository's
  "Security" → "Report a vulnerability").
- Or by email: ryoichi.a.izumita@accenture.com

Please include a minimal reproducing `.fsl` (or trace), the command you ran, and
the observed vs. expected behavior.

## Response Targets

- Acknowledgement of receipt: within a few days.
- Impact assessment and sharing of a fix plan: on an ongoing basis depending on
  the situation.
- After a fix, we publish it with credit to the reporter (if desired).

Please refrain from irresponsible disclosure (such as making details public
before reporting).
