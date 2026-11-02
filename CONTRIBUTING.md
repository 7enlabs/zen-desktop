# Contributing to ZenCode Desktop

Thank you for your interest in contributing to ZenCode Desktop! This document
outlines the process and expectations.

## Branch Policy

- **Never push directly to `main`.** All changes go through pull requests.
- Branch naming (English, kebab-case):
  - `dev/<name>` - developer tasks
  - `lead/<name>` - leader tasks
  - `hotfix/<name>` - urgent fixes
- One branch = one task = one PR.
- Create branches from the latest `main`.

## Pull Request Process

1. PRs must be in **English** (title, description, comments, commits).
2. Link related issues. Keep the diff minimal - no unrelated refactors.
3. A leader reviews every PR. Developers do **not** self-merge.
4. CI must be green. Run before pushing:
   ```bash
   cd engine && cargo fmt --check
   cd engine && cargo clippy --all-targets -- -D warnings
   cd engine && cargo test --locked
   ```
5. Merge strategy: squash. Delete the branch after merge.

## Code Standards

- Rust edition 2024. Follow `rustfmt` defaults.
- No `unwrap()` in production paths - use proper error types (`thiserror`/`anyhow`).
- No hardcoded secrets, API keys, or credentials. Ever.
- Cross-platform: code must work on Linux, macOS, and Windows.
  Use the `dirs` crate for paths, never hardcode `/home` or `C:\`.

## Clean-Room Rule

ZenCode Desktop is built clean-room. Never copy code, prompts, or tool descriptions
from Claude Code, Codex, or similar tools - even from memory of leaked sources.
Only public binaries, SDK types, official docs, and changelogs may be used
as references.

## Contributions

Contributions follow the Developer Certificate of Origin (DCO). No CLA.

## Reporting Security Issues

See [SECURITY.md](SECURITY.md).
Do **not** open public issues for vulnerabilities.
