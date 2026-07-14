# Caerus — AI Use & Security Disclaimer

## AI-Assisted Development

Caerus was developed with the help of AI coding assistants (Claude Code) for
implementation, debugging, and iteration. This means:

- **Human review is the safety net, not AI correctness.** Every AI-generated
  change was compiled, run, and manually verified by the maintainer before
  being kept. AI suggestions are a starting point, not a guarantee.
- **Novel or subtle bugs are more likely, not less.** AI models can produce
  code that compiles and looks idiomatic while still containing logic errors,
  especially around memory ownership, FFI boundaries, and privilege handling
  — the exact areas where this project needed the most care (see below).
- **Treat Caerus as software from an independent hobby project**, not an
  audited, vendor-backed package manager. Use the `xbps` CLI tools directly
  if you need a hardened, well-audited path for critical systems.

## Security Principles Caerus Is Built On

| Principle | How it's applied |
|---|---|
| **Least privilege** | Caerus itself runs unprivileged. Privileged operations (install/remove/update) are handed off through **polkit**, not run as root inside the GTK process. |
| **Memory safety** | The rewrite from C to **Rust** removes an entire class of bugs (use-after-free, double-free, buffer overflows) that were present and actively debugged in the original C version. |
| **No hidden network calls** | Caerus only talks to whatever repositories `xbps` is already configured to use — it doesn't introduce new remote endpoints. |
| **Read-only by default** | Browsing/searching packages requires no elevated privilege at all; only mutating actions request escalation. |

## Risks of Running Caerus

- **Privilege escalation surface**: any GUI wrapper around a package manager
  is a target — a bug in how Caerus constructs the command passed to polkit
  could be leveraged to run something other than intended. Review the
  polkit policy file before trusting it on a multi-user or sensitive machine.
- **Trust in upstream repos, not just Caerus**: Caerus does not vet package
  contents — it installs whatever `libxbps`/your configured repos serve.
  Compromised mirrors or packages are outside Caerus's threat model.
- **Early-stage software**: this is a rewrite that has not had extensive
  real-world testing across hardware/configs. Expect rough edges, and avoid
  running it as your only path to a system you can't otherwise recover.
- **AI-assisted code review gaps**: some logic (e.g. reference-counting
  around GObject lifecycles) is easy to get subtly wrong in ways that pass
  casual testing but fail under specific timing or object-lifetime conditions.

## How It's Built

A 3-crate Cargo workspace: `xbps-sys` (bindgen FFI bindings to `libxbps`,
generated at build time from your system's `<xbps.h>`), `caerus` (the
unprivileged GTK4 app), and `caerus-helper` (the one privileged component,
spawned via `pkexec`). No GtkBuilder `.ui` templates — the UI is hand-built
in Rust, so there's no separate template/GResource build step; Cargo needs
nothing beyond `libxbps-devel` and GTK4's own dev headers.

## Logic Behind Key Functionality

- **One thread touches libxbps.** Every `libxbps` call runs on a single
  dedicated OS thread for the process's whole lifetime (see the comment
  atop `caerus/src/backend/package_store.rs`) — everything else reaches it
  by message over a channel, so the UI thread never blocks on a package
  operation, and concurrent/re-entrant access is a type-level impossibility
  rather than something a runtime lock has to enforce. This was the fix for
  real crashes in the original C version, caused by concurrent/re-entrant
  `xbps_init`/`xbps_end` calls.
- **Privilege separation via polkit.** The GTK app never runs as root —
  asking for a password inside Caerus, or running the whole app as root,
  were both rejected early on. Instead, a change is queued as a
  line-oriented command (`INSTALL pkg1 pkg2`, `REMOVE ...`, `SYNC`, ...) and
  sent to `caerus-helper`, a small dependency-free binary spawned once via
  `pkexec` and kept alive (5-minute idle timeout) so one session doesn't
  keep re-prompting. It does nothing but parse that protocol and shell out
  to the matching `xbps-*` tool, streaming output back — the one
  privileged, security-relevant component in the project, kept
  intentionally small and auditable.
- **Buffer-based xbps calls**: functions like `xbps_pkgpattern_name` expect
  a caller-provided buffer rather than returning allocated memory. Caerus
  follows that contract explicitly rather than assuming ownership transfer,
  which was a real source of bugs during development.
- **Explicit object lifetimes**: GObject `dispose()`/`finalize()` ordering is
  handled carefully so cleanup never runs after the parent chain-up has
  already emitted `destroy` — a bug class that caused real segfaults in the
  original C implementation.

---
*This disclaimer is meant to be transparent about how Caerus was built and
where its risks lie — not to discourage use, but so anyone running it (or
reviewing the code) knows what assumptions were made.*
