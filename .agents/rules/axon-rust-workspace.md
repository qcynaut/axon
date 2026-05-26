# Axon Rust Workspace Rules

Apply these rules for every task in this workspace.

## Source of Truth

- Treat @/specs/ as the source of truth for Axon behavior, protocol details, implementation profile, and test vectors.
- Read the relevant spec documents before changing behavior, APIs, wire formats, lifecycle handling, error handling, security behavior, or tests.
- If implementation and specs disagree, prefer the specs and report the mismatch.

## Dependencies

- Before adding or updating Rust dependencies, check the latest available crate version on https://crates.io and review the relevant crate documentation on https://docs.rs when network access is available or dependency information is needed.
- Store shared dependency declarations in @/Cargo.toml under `[workspace.dependencies]`.
- Subcrates must use workspace-managed dependencies:

  ```toml
  dependency-name = { workspace = true }
  ```

- Do not add dependency declarations directly to subcrates unless the dependency is intentionally local to that crate and cannot be shared.

## Nix And Tooling

- Respect @/flake.nix as the expected development environment.
- Prefer running tools through `nix develop -c`.
- Fall back to system-installed tools only when the flake environment is unavailable, and mention the fallback.

## Rust Code Style

- Generated Rust code must match @/clippy.toml and @/rustfmt.toml.
- Use idiomatic Rust: clear ownership, small types, explicit errors, standard traits, and simple module boundaries.
- Keep each non-test Rust source file under 1000 lines of actual code. Split large files by responsibility before they become hard to review.
- Read existing code before adding new helpers, types, modules, or tests. Reuse existing patterns and avoid duplicate implementations.
- Follow KISS: choose the simplest clear implementation that solves the current requirement.
- Follow YAGNI: do not add unused abstractions, options, extension points, or future-facing code.
- Add documentation only when it clarifies non-obvious behavior, invariants, public APIs, safety requirements, or spec mapping. Do not comment obvious code.

## Tests

- Add unit tests when they protect meaningful behavior, edge cases, spec requirements, or regressions.
- Do not add tests that only restate implementation details or assert trivial behavior.
- Keep tests focused and close to the code under test unless an integration test better matches the behavior.

## Verification

After changing Rust code, run:

```sh
nix develop -c cargo check
nix develop -c cargo fmt --check
nix develop -c cargo clippy --all-targets --all-features -- -D warnings
nix develop -c cargo deny check
nix develop -c cargo audit
```

If `nix develop` is unavailable, run the fallback commands:

```sh
cargo check
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo deny check
cargo audit
```

If any verification command cannot be run, say exactly which command failed or was skipped and why.
