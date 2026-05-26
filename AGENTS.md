# Agent Instructions

These instructions apply to the entire workspace. Follow them before making or reviewing any code changes.

## Source of Truth

- Treat [`specs/`](specs/) as the product and protocol source of truth.
- Before implementing behavior, changing APIs, or adding tests, read the relevant spec documents first.
- If existing code and specs disagree, prefer the specs and call out the mismatch clearly.

## Rust Workspace Conventions

- Keep shared dependencies in the root [`Cargo.toml`](Cargo.toml) under `[workspace.dependencies]`.
- Subcrates should depend on workspace-managed dependencies with:

  ```toml
  dependency-name = { workspace = true }
  ```

- Before adding or updating dependencies, check the current versions on [crates.io](https://crates.io) and the docs on [docs.rs](https://docs.rs) when network access is available or needed.
- Respect [`flake.nix`](flake.nix). Prefer running tools through the flake dev environment, and only fall back to system-installed tools when the flake environment is unavailable.

## Code Style

- Generated Rust code must satisfy [`clippy.toml`](clippy.toml), [`rustfmt.toml`](rustfmt.toml), and idiomatic Rust conventions.
- Keep Rust source files below 1000 lines of actual code, excluding tests. When a file grows too large, split it by clear responsibility.
- Read existing code before adding new code so behavior, helpers, and abstractions are not duplicated.
- Prefer simple, direct implementations. Follow KISS: choose the smallest clear design that solves the current problem.
- Follow YAGNI: do not add unused abstractions, options, helpers, or future-facing code.
- Document behavior when documentation adds real value. Skip comments and docs when the code is already obvious.

## Tests

- Add unit tests when they protect meaningful behavior, edge cases, or regressions.
- Do not add tests that only duplicate implementation details or assert trivial behavior.
- Keep tests close to the code they exercise unless an integration-style test is more appropriate.

## Verification

After generating or changing Rust code, run the relevant checks. Prefer the flake environment:

```sh
nix develop -c cargo check
nix develop -c cargo fmt --check
nix develop -c cargo clippy --all-targets --all-features -- -D warnings
nix develop -c cargo deny check
nix develop -c cargo audit
```

If `nix develop` is unavailable, fall back to:

```sh
cargo check
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo deny check
cargo audit
```

If any verification command cannot be run, explain why and report the remaining risk.
