## Summary

<!-- What does this change do, and why? One or two paragraphs. -->

## Type

<!-- Check one -->
- [ ] bug fix
- [ ] feature
- [ ] refactor (no behavior change)
- [ ] docs
- [ ] chore / deps / CI
- [ ] breaking change

## Scope

<!-- Check all that apply -->
- [ ] coordify-core (Rust)
- [ ] coordify-hook (Node)
- [ ] coordify-cli (TS)
- [ ] coordify-sim (TS)
- [ ] documentation
- [ ] CI / release

## Checklist

<!-- Required for code changes. CI enforces most of these; do not push red. -->
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --all-targets -- -D warnings` passes
- [ ] `cargo test` passes
- [ ] Coverage did not drop below 90% (Core changes)
- [ ] `npm run build` + `npm test` pass (TS packages)
- [ ] `cargo audit` and `npm audit` introduce no new advisories
- [ ] No new outbound network calls, telemetry, or account requirements
- [ ] No new runtime dependency added without prior discussion in an issue
- [ ] No secrets, tokens, or personal paths committed
- [ ] README / QUICKSTART updated if user-visible behavior changed
- [ ] CHANGELOG entry added under the next version section

## Test plan

<!-- How did you verify this? For Core changes, list the sim scenarios you ran. -->

- [ ] ...
- [ ] ...

## Linked issues

<!-- `Fixes #123`, `Refs #456`, or none. -->
