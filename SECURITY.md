# Security Policy

## Reporting a vulnerability

Please report security issues privately — do **not** open a public issue.

- Preferred: GitHub **private vulnerability reporting** on this repository
  (Security → Report a vulnerability).
- Alternative: create a private issue or contact the maintainer directly.

Include, when possible:

- the ChipClean version and OS build;
- steps to reproduce;
- expected vs actual behavior;
- any relevant log excerpts (do not include personal files or registry dumps).

## Scope

- Security bugs in ChipClean code and its dependencies.
- Unsafe handling of downloaded rules (winapp2.ini) — they are data, never
  executed.
- Data exposure: ChipClean must never upload local data.

## Handling

Vulnerabilities are acknowledged within 5 business days and fixed in a release
with an advisory note in the changelog.
