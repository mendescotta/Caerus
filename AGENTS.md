# Rust agent policy

This is the shared policy for these Rust repositories. Keep each repository's
`AGENTS.md` copy identical when changing these rules. `CLAUDE.md` adds
project-specific context; `.github/copilot-instructions.md` points here.

## Before editing

- Read this repository's `CLAUDE.md` when present, its `README.md`, and the
  relevant code and tests. Check `git status` and preserve unrelated work.
- Follow the existing architecture and Rust edition. Do not restructure a
  working crate, change the MSRV, add a dependency, or enable a new lint group
  solely to satisfy a style preference.
- Keep privileged operations in the established helper or backend. Treat file
  paths, package names, D-Bus messages, device data, and subprocess output as
  untrusted input. Never print secrets or run destructive host operations for
  verification.

## Rust changes

- Prefer standard Rust idioms and the Rust API Guidelines. Use clear names,
  focused modules, and explicit ownership. Add a new crate or abstraction when
  it solves a real boundary, not just to follow a layout rule.
- Represent domain invariants with types and validate at construction or
  parsing boundaries. Use `Option` and `Result` for absence and expected
  failure. Give errors enough context to diagnose the failed operation.
- A panic is for a broken invariant or an unrecoverable programming error;
  handle expected user, I/O, and system failures as errors. Avoid `unwrap()` on
  production input. Explain any `expect()` invariant in its message.
- Use `unsafe` only where a safe API cannot do the job. Keep the unsafe surface
  small, document the safety argument beside it, and test the boundary. Never
  use `unsafe` to bypass ownership or thread-safety checks.
- Keep platform and FFI details behind a small interface so core behavior can
  be tested without hardware, root privileges, or a live desktop session.
- Document public APIs, non-obvious invariants, and the reason for magic
  values. Keep comments and examples accurate. Avoid duplicate public paths
  to the same item unless compatibility requires them.
- Follow the repository's existing logging convention. Use structured fields
  where supported and do not log credentials, personal data, or raw secrets.
- Optimize observed hot paths after measurement. Avoid mandatory allocators,
  CPU-specific flags, parallelism, and extra caching without a measured need.

## Verification and handoff

- Add or update tests for observable behavior and meaningful failure cases.
  Do not add tests that merely repeat a constant or mirror implementation
  branches. For risky FFI or unsafe changes, use the project's available
  specialized checks where practical.
- For Rust changes, run `cargo fmt --check`, `cargo clippy --all-targets --
  -D warnings`, and `cargo test` from the repository root, plus applicable
  project-specific checks from `CLAUDE.md`. Report checks that could not run.
- Prefer a scoped `#[expect(lint, reason = "...")]` for a justified lint
  exception when the toolchain supports it. Do not suppress unrelated warnings.
- Keep changes scoped to the task. Report what changed, what was verified,
  and any remaining risk. Follow the repository's release and publishing rules.

Reference: Microsoft's [Pragmatic Rust Guidelines](https://microsoft.github.io/rust-guidelines/)
(version 2026.6), adopted selectively for these Void Linux projects.
