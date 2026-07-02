# Changelog

All notable changes to `codex-harness` are documented here.

## 0.1.0 - 2026-07-02

### Added

- Initial Rust CLI for applying Codex harness documentation.
- `work-notes` default mode for repo-safe scratch planning policy.
- Optional `tracked` mode for repositories that intentionally keep execution plans in `docs/exec-plans/`.
- JSON envelopes for command automation.
- Doctor, source, repo inspect/plan/apply, audit, validate, SOP, and upstream request commands.
- Template-drift gate for audit, validate, and inspect so exact bundled templates cannot be mistaken for customized harness docs.
- Unit and property-based tests for template paths, apply behavior, request path safety, and command invariants.
