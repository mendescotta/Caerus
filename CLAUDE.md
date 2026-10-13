# caerus

GTK4 package manager GUI for Void (workspace: `xbps-sys`, `caerus`,
`caerus-helper`). Flagship project. Public repo `mendescotta/Caerus`.
Design rationale and gotchas: @DEVNOTES.md (local, gitignored).

## Check before claiming done
- `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
- Optional `adwaita` feature: also build with `--features adwaita`.
- `scripts/audit.sh` (ripgrep audit of risky patterns; there is no autofix).

## Rules
- The GUI never runs as root. Only `caerus-helper` is elevated (pkexec); it
  stays GTK-free with zero external crates. Protocol: DEVNOTES.
- Only `backend::package_store` calls `xbps-sys`; one worker thread owns the
  `xbps_handle`. Do not call libxbps from anywhere else.
- Sources live in `caerus/caerus/src/...` (workspace member), not `caerus/src`.
- Version compare with `xbps_cmpver`, never string order.
- Release: bump Cargo version + CHANGELOG, tag, then bump the template in
  the overlay (`voidlab/srcpkgs/caerus`, `./voidlab update caerus`). The overlay
  template is the only copy; there is none in this repo.
- New design/plan docs: `docs/superpowers/{specs,plans}/` (gitignored, local).

# Agent Guidelines for Rust Code Quality

This document provides guidelines for maintaining high-quality Rust code. These rules MUST be followed by all AI coding agents and contributors.

## Your Core Principles

All code you write MUST be fully optimized.

"Fully optimized" includes:

- maximizing algorithmic big-O efficiency for memory and runtime
- using parallelization and SIMD where appropriate
- following proper style conventions for Rust (e.g. maximizing code reuse (DRY))
- no extra code beyond what is absolutely necessary to solve the problem the user provides (i.e. no technical debt)

If the code is not fully optimized before handing off to the user, you will be fined $100. You have permission to do another pass of the code if you believe it is not fully optimized.

## Preferred Tools

- Use `cargo` for project management, building, and dependency management.
- Use `indicatif` to track long-running operations with progress bars. The message should be contextually sensitive.
- Use `serde` with `serde_json` for JSON serialization/deserialization.
- Use `ratatui` adnd `crossterm` for terminal applications/TUIs.
- Use `axum` for creating any web servers or HTTP APIs.
  - Keep request handlers async, returning `Result<Response, AppError>` to centralize error handling.
  - Use layered extractors and shared state structs instead of global mutable data.
  - Add `tower` middleware (timeouts, tracing, compression) for observability and resilience.
  - Offload CPU-bound work to `tokio::task::spawn_blocking` or background services to avoid blocking the reactor.
- When reporting errors to the console, use `tracing::error!` or `log::error!` instead of `println!`.
- If designing applications with a web-based front end interface, e.g. compiling to WASM or using `dioxus`:
  - All deep computation **MUST** occur within Rust processes (i.e. the WASM binary or the `dioxus` app Rust process). **NEVER** use JavaScript for deep computation.
  - The front-end **MUST** use Pico CSS and vanilla JavaScript. **NEVER** use jQuery or any component-based frameworks such as React.
  - The front-end should prioritize speed and common HID guidelines.
  - The app should use adaptive light/dark themes by default, with a toggle to switch the themes.
  - The typography/theming of the application **MUST** be modern and unique, similar to that of popular single-page web/mobile. **ALWAYS** add an appropriate font for headers and body text. You may reference fonts from Google Fonts.
  - **NEVER** use the Pico CSS defaults as-is: a separate CSS/SCSS file is encouraged. The design **MUST** logically complement the semantics of the application use case.
  - **ALWAYS** rebuild the WASM binary if any underlying Rust code that affects it is touched.
- For data processing:
  - **ALWAYS** use `polars` instead of other data frame libraries for tabular data manipulation.
  - If a `polars` dataframe will be printed, **NEVER** simultaneously print the number of entries in the dataframe nor the schema as it is redundant.
  - **NEVER** ingest more than 10 rows of a data frame at a time. Only analyze subsets of data to avoid overloading your memory context.

  - **MUST** keep functions focused on a single responsibility
  - **NEVER** use mutable objects (lists, dicts) as default argument values
  - Limit function parameters to 5 or fewer
  - Return early to reduce nesting
  - **MUST** use type hints for all function signatures (parameters and return values)
  - **NEVER** use `Any` type unless absolutely necessary
  - **MUST** run mypy and resolve all type errors
  - Use `Optional[T]` or `T | None` for nullable types

## Code Style and Formatting

- **MUST** use meaningful, descriptive variable and function names
- **MUST** follow Rust API Guidelines and idiomatic Rust conventions
- **MUST** use 4 spaces for indentation (never tabs)
- **NEVER** use emoji, or unicode that emulates emoji (e.g. ✓, ✗). The only exception is when writing tests and testing the impact of multibyte characters.
- Use snake_case for functions/variables/modules, PascalCase for types/traits, SCREAMING_SNAKE_CASE for constants
- Limit line length to 100 characters (rustfmt default)
- Assume the user is a Python expert, but a Rust novice. Include additional code comments around Rust-specific nuances that a Python developer may not recognize.

## Documentation

- **MUST** include doc comments for all public functions, structs, enums, and methods
- **MUST** document function parameters, return values, and errors
- Keep comments up-to-date with code changes
- Include examples in doc comments for complex functions

Example doc comment:

````rust
/// Calculate the total cost of items including tax.
///
/// # Arguments
///
/// * `items` - Slice of item structs with price fields
/// * `tax_rate` - Tax rate as decimal (e.g., 0.08 for 8%)
///
/// # Returns
///
/// Total cost including tax
///
/// # Errors
///
/// Returns `CalculationError::EmptyItems` if items is empty
/// Returns `CalculationError::InvalidTaxRate` if tax_rate is negative
///
/// # Examples
///
/// ```
/// let items = vec![Item { price: 10.0 }, Item { price: 20.0 }];
/// let total = calculate_total(&items, 0.08)?;
/// assert_eq!(total, 32.40);
/// ```
pub fn calculate_total(items: &[Item], tax_rate: f64) -> Result<f64, CalculationError> {
````

## Type System

- **MUST** leverage Rust's type system to prevent bugs at compile time
- **NEVER** use `.unwrap()` in library code; use `.expect()` only for invariant violations with a descriptive message
- **MUST** use meaningful custom error types with `thiserror`
- Use newtypes to distinguish semantically different values of the same underlying type
- Prefer `Option<T>` over sentinel values

## Error Handling

- **NEVER** use `.unwrap()` in production code paths
- **MUST** use `Result<T, E>` for fallible operations
- **MUST** use `thiserror` for defining error types and `anyhow` for application-level errors
- **MUST** propagate errors with `?` operator where appropriate
- Provide meaningful error messages with context using `.context()` from `anyhow`

## Function Design

- **MUST** keep functions focused on a single responsibility
- **MUST** prefer borrowing (`&T`, `&mut T`) over ownership when possible
- Limit function parameters to 5 or fewer; use a config struct for more
- Return early to reduce nesting
- Use iterators and combinators over explicit loops where clearer

## Struct and Enum Design

- **MUST** keep types focused on a single responsibility
- **MUST** derive common traits: `Debug`, `Clone`, `PartialEq` where appropriate
- Use `#[derive(Default)]` when a sensible default exists
- Prefer composition over inheritance-like patterns
- Use builder pattern for complex struct construction
- Make fields private by default; provide accessor methods when needed

## Testing

- **MUST** write unit tests for all new functions and types
- **MUST** mock external dependencies (APIs, databases, file systems)
- **MUST** use the built-in `#[test]` attribute and `cargo test`
- Follow the Arrange-Act-Assert pattern
- Do not commit commented-out tests
- Use `#[cfg(test)]` modules for test code

## Imports and Dependencies

- **MUST** avoid wildcard imports (`use module::*`) except for preludes, test modules (`use super::*`), and prelude re-exports
- **MUST** document dependencies in `Cargo.toml` with version constraints
- Use `cargo` for dependency management
- Organize imports: standard library, external crates, local modules
- Use `rustfmt` to automate import formatting

## Rust Best Practices

- **NEVER** use `unsafe` unless absolutely necessary; document safety invariants when used
- **MUST** call `.clone()` explicitly on non-`Copy` types; avoid hidden clones in closures and iterators
- **MUST** use pattern matching exhaustively; avoid catch-all `_` patterns when possible
- **MUST** use `format!` macro for string formatting
- Use iterators and iterator adapters over manual loops
- Use `enumerate()` instead of manual counter variables
- Prefer `if let` and `while let` for single-pattern matching

## Memory and Performance

- **MUST** avoid unnecessary allocations; prefer `&str` over `String` when possible
- **MUST** use `Cow<'_, str>` when ownership is conditionally needed
- Use `Vec::with_capacity()` when the size is known
- Prefer stack allocation over heap when appropriate
- Use `Arc` and `Rc` judiciously; prefer borrowing

## Concurrency

- **MUST** use `Send` and `Sync` bounds appropriately
- **MUST** prefer `tokio` for async runtime in async applications
- **MUST** use `rayon` for CPU-bound parallelism
- Avoid `Mutex` when `RwLock` or lock-free alternatives are appropriate
- Use channels (`mpsc`, `crossbeam`) for message passing

## Security

- **NEVER** store secrets, API keys, or passwords in code. Only store them in `.env`.
  - Ensure `.env` is declared in `.gitignore`.
- **MUST** use environment variables for sensitive configuration via `dotenvy` or `std::env`
- **NEVER** log sensitive information (passwords, tokens, PII)
- Use `secrecy` crate for sensitive data types

## Version Control

- **MUST** write clear, descriptive commit messages
- **NEVER** commit commented-out code; delete it
- **NEVER** commit debug `println!` statements or `dbg!` macros
- **NEVER** commit credentials or sensitive data

## Tools

- **MUST** use `rustfmt` for code formatting
- **MUST** use `clippy` for linting and follow its suggestions
- **MUST** ensure code compiles with no warnings (use `-D warnings` flag in CI, not `#![deny(warnings)]` in source)
- Use `cargo` for building, testing, and dependency management
- Use `cargo test` for running tests
- Use `cargo doc` for generating documentation
- **NEVER** build with `cargo build --features python`: this will always fail. Instead, **ALWAYS** use `maturin`.

## Before Committing

- [ ] All tests pass (`cargo test`)
- [ ] No compiler warnings (`cargo build`)
- [ ] Clippy passes (`cargo clippy -- -D warnings`)
- [ ] Code is formatted (`cargo fmt --check`)
- [ ] If the project creates a WASM package and Rust code is touched, rebuild the WASM package (`wasm-pack build --target web --out-dir web/pkg`)
- [ ] All public items have doc comments
- [ ] No commented-out code or debug statements
- [ ] No hardcoded credentials

---

**Remember:** Prioritize clarity and maintainability over cleverness.

## Open
- xlint: template uses deprecated `wrksrc`; GPL vlicense hint.

## Fixed
- 2026-10-13: "Remove Anyway" segfault. Root cause confirmed with a local
  debug build of gtk4 4.24.1 (symbols; stock Void gtk4 ships stripped) and
  two real core dumps with byte-identical crash offsets. The checkbox that
  triggers removal lives inside a `GtkColumnViewRowWidget`; `set_mark()`
  replaces that package's list item via `GListStore::splice()` (the
  documented "replace, don't mutate" rebind pattern — see DEVNOTES), which
  unparents/recycles the row widget that still has GTK keyboard focus. If
  the window regains X11 focus right as the confirm dialog closes (it
  always does) before GTK's deferred stale-focus cleanup runs, GTK tries to
  restore focus to the now-parentless row and crashes in
  `gtk_column_view_row_widget_get_column_view()` — upstream gtk4 4.24.1,
  `gtk/gtkcolumnviewrowwidget.c:45`, forces a second
  `_gtk_widget_get_parent()` call with no NULL guard. Not a caerus logic
  bug, but caerus controls the race window: `remove_confirm.rs`'s
  "Remove Anyway" handler now clears the parent window's focus
  (`GtkWindowExt::set_focus(None)`) before `dlg.destroy()`, so there's
  nothing stale left for GTK to restore. Verified against the exact repro
  (search → toggle a row's checkbox → Remove Anyway) and ~30 other
  mark/unmark cycles with zero crashes after the fix, versus 2/2
  reproductions before it. Worth reporting upstream (same bug family as
  gtk4-rs#1673 / GTK MR!7095, MR!7253 — recycled list-view row widgets
  racing with GTK's focus/selection bookkeeping).
- 2026-10-12: crash at launch. Not a caerus bug — `ibus-gtk4` 1.5.34's GTK4
  immodule recurses infinitely in `g_object_new` under gtk4 4.24.x, aborting
  every GTK4 app on launch. Fixed by pinning `ibus` to 1.5.32_4 in the overlay
  (`voidlab/srcpkgs/ibus`, voidlab-testing commit `171d6e9`).
