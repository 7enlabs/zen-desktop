# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| < 1.0   | :x: (pre-release) |

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Instead, report privately via one of:

- GitHub Security Advisories on this repository
  (Security tab → "Report a vulnerability"), or
- Email the maintainers (see repository profile for contact).

Please include:

- Description of the vulnerability
- Steps to reproduce
- Affected versions / platforms
- Your assessment of impact (optional)

## What to Expect

- Acknowledgment within **72 hours**.
- A fix or mitigation plan for confirmed issues, prioritized by severity.
- Public disclosure only after a fix is available, coordinated with the reporter.

## Scope Notes

ZenCode Desktop runs the ZenCode agent core locally. It executes code and accesses
API keys by design. The threat model assumes the operator trusts the machine
ZenCode Desktop runs on. Out of scope:

- Social-engineering or phishing against operators
- Vulnerabilities in third-party model providers (report to them directly)
- Denial-of-service via local resource exhaustion (documented limitation)

## Dependency Security

- `cargo audit` and `cargo deny` run in CI. New dependencies must pass both.
- Prefer minimal, well-maintained crates. Avoid new dependencies unless necessary.
