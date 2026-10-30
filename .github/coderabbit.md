# CodeRabbit review configuration

[.coderabbit.yaml](../.coderabbit.yaml) adapts CodeRabbit to Caerus's Rust/Cargo
workspace. It enables assertive incremental reviews for non-draft pull requests,
concise summaries, linked-issue assessment and advisory title/description checks. Automatic requests for changes remain disabled.

Review guidance covers libxbps/bindgen ownership and ABI safety, GTK responsiveness
and accessibility, the root-helper protocol and polkit boundary, XBPS/APK feature
gates, POSIX installation scripts, Discover verification, workflow security and
the Codespaces setup.

Clippy, ShellCheck, Ruff, actionlint, zizmor, Markdown/YAML linting, secret scanning,
OSV dependency scanning and existing GitHub check results are enabled. Cargo.lock
remains in review scope. Build artifacts and image assets are excluded. Repository
learnings stay local; automatic chat replies are limited to organization members.

CodeRabbit may comment and summarise but never commits: the finishing touches
that push commits or open PRs (fix comments, fix CI, merge-conflict resolution,
docstring/unit-test generation and simplify) are disabled. Generated Discover
evidence (`scripts/discover/*.json`) is excluded from review.

The native build/test workflows remain authoritative for Void-specific compilation
and GTK smoke tests. CodeRabbit's tools depend on its service plan and runner
capabilities; enabling a tool does not prove it ran successfully.

Validate changes using the saved checker:

```sh
uv run scripts/check-coderabbit.py
```

This fetches the [official schema](https://coderabbit.ai/integrations/schema.v2.json)
with a timeout, parses YAML safely, rejects duplicate settings and reports schema
errors. Dependencies are declared in the script and managed by `uv`. With a saved
schema and available dependencies, validation can run without fetching the schema:

```sh
uv run scripts/check-coderabbit.py --schema /path/to/schema.v2.json
```

Saved schemas must describe an object and define CodeRabbit's `reviews` and `chat`
object properties. A different JSON document or schema is a setup error. Offline
checker regression tests: `uv run scripts/test-check-coderabbit.py`.

Exit status 0 means valid, 1 means invalid configuration, and 2 means the check
could not run. Options can be checked against the
[CodeRabbit configuration reference](https://docs.coderabbit.ai/reference/configuration).

The configuration takes effect once it is on the default branch and the
CodeRabbit GitHub App has access to the repository. It does not install the
app or change branch protection, repository permissions or required checks.
