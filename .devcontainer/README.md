# Codespaces

Two configurations are available from **Code → Codespaces → New with options**:

| Configuration | Use it for |
| --- | --- |
| **Caerus (Void Linux)** (default) | Building, testing and reviewing changes. |
| **Caerus with browser desktop** | Running the GTK interface. A virtual display is shown in the browser through noVNC on forwarded port 6080. |

Both run the official `ghcr.io/void-linux/void-glibc-full` image as root and install
libxbps, GTK4/libadwaita, Rust, rust-analyzer, ShellCheck, uv and the GitHub CLI
on creation ([setup.sh](setup.sh)). No prebuild is configured, so the first start
takes a few minutes. Because the container is Void, Caerus reads a real XBPS
package database there.

Run the same checks as CI:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test -p caerus --no-default-features --features apk,adwaita --locked
```

In the desktop configuration, open the **Desktop (noVNC)** port and select
**Connect**, then start the interface from a terminal:

```sh
cargo run -p caerus
```

If the desktop stops, run `sh .devcontainer/desktop/start-desktop.sh` again (logs
are in `/tmp/caerus-desktop`). VNC listens only on localhost; the forwarded port
stays private to your GitHub account unless you change its visibility. Package
changes need the polkit helper and a session agent, which a codespace does not
provide, so use it for browsing and UI work, not for applying transactions.

Codespaces are for development and checks. Do not store release credentials here or
publish from an agent session.
