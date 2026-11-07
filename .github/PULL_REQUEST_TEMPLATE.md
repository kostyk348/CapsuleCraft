## What this changes

<!-- One paragraph. Link the issue if there is one: "Closes #12" -->

## Type

- [ ] Bug fix
- [ ] Feature
- [ ] Documentation
- [ ] CI / build / tooling
- [ ] Refactor (no behaviour change)

## Capsule format impact

- [ ] None
- [ ] New header / MIME part (describe below)

<!-- If the format changed, show a minimal .eml before and after -->

## Checks

- [ ] `cargo fmt --manifest-path native/Cargo.toml --check`
- [ ] `cargo clippy --manifest-path native/Cargo.toml --all-targets -- -D warnings`
- [ ] `cargo test --manifest-path native/Cargo.toml -j 1`
- [ ] Exported `.eml` round-trips back into the same capsule (if format touched)

## Screenshots

<!-- UI changes: before / after -->
