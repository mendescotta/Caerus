# Codespaces

Open this repository in GitHub Codespaces and use the preinstalled stable Rust toolchain. The Void Linux container installs libxbps, GTK4, Rust, and the project’s build tools at creation time; no prebuild is configured.

Verify changes with:

```sh
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked
```

Codespaces are for development and checks. Do not store release credentials here or publish from an agent session.
