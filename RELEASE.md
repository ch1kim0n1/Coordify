# Coordify Release Runbook

End-to-end steps to cut a Coordify release. Follow in order. Do not skip the
verification steps — a botched publish is hard to undo (crates.io and npm do
not allow re-publishing the same version, and yanking is imperfect).

## 0. Prerequisites (one-time setup)

### crates.io publish token
1. Log in to https://crates.io with the maintainer account.
2. Account Settings → API Tokens → Create New Token.
   - Scope: `publish-new` (publish-only, not update). Name it
     `github-actions-coordify-core`.
3. In the GitHub repo: Settings → Secrets and variables → Actions →
   New repository secret. Name: `CARGO_REGISTRY_TOKEN`. Value: the token.

### npm publish token
1. Log in to https://www.npmjs.com with the maintainer account.
2. Access Tokens → Generate New Token → **Granular Access Token**.
   - Allowed packages: `coordify-hook`, `coordify-cli`, `coordify-sim`.
   - Permissions: Read and write.
   - Expiration: set a short window (90 days max). Rotate before expiry.
3. In the GitHub repo: Settings → Secrets and variables → Actions →
   New repository secret. Name: `NPM_TOKEN`. Value: the token.

### 2FA
- Both crates.io and npm accounts must have 2FA enabled. npm requires it for
  publish; crates.io strongly recommends it.

### GitHub Actions billing
- The repo is **private**. GitHub Actions minutes for private repos are paid.
  Confirm billing is active at https://github.com/settings/billing before
  pushing the release tag, or the release workflow will not start.
- Alternative: make the repo public. Public repos get free ubuntu minutes and
  free npm provenance/SIGSTORE signing. macOS minutes stay paid either way,
  which is why CI is ubuntu-only.

## 1. Pre-release verification (on a clean macOS + Linux machine)

Run these on a machine that is **not** your dev box — ideally a fresh VM or a
colleague's machine — to catch "works on my machine" issues.

```bash
# 1. Clone fresh
git clone https://github.com/ch1kim0n1/Coordify.git
cd Coordify

# 2. Rust: build, lint, test
cd packages/coordify-core
cargo build --release
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo test
cd ../..

# 3. TypeScript: build + test each package
for p in coordify-hook coordify-cli coordify-sim; do
  (cd packages/$p && npm ci && npm run build && npm test)
done

# 4. Sim scenarios pass
cd packages/coordify-sim
npm run build
for s in scenarios/*.json; do
  echo "=== $s ==="
  node dist/cli.js simulate "$s" || { echo "FAIL: $s"; exit 1; }
done
cd ../..

# 5. Clean-machine install simulation
#    (uninstall first if present)
cargo uninstall coordify-core 2>/dev/null || true
npm uninstall -g coordify-hook coordify-cli coordify-sim 2>/dev/null || true
cargo install --path packages/coordify-core
npm install -g packages/coordify-hook packages/coordify-cli packages/coordify-sim

# 6. End-to-end: open a Claude Code session in a test project, run a few
#    prompts, verify `coordify status` shows the agent and heat.
```

All steps must pass on **both** macOS and Linux. If any step fails, fix and
re-commit before proceeding. Do not tag a broken release.

## 2. Version bump

If this is a new version (not the first 0.1.0), bump in lockstep:

- `packages/coordify-core/Cargo.toml` → `version = "X.Y.Z"`
- `packages/coordify-hook/package.json` → `"version": "X.Y.Z"`
- `packages/coordify-cli/package.json` → `"version": "X.Y.Z"`
- `packages/coordify-sim/package.json` → `"version": "X.Y.Z"`
- `packages/coordify-core/src/paths.rs` → `pub const VERSION: &str = "X.Y.Z";`
- `CHANGELOG.md` → new `## [X.Y.Z] - YYYY-MM-DD` section

All five must match. CI does not currently enforce this — a future CI job
should. For now, verify manually:

```bash
grep -h '"version"' packages/*/package.json
grep '^version' packages/coordify-core/Cargo.toml
grep 'pub const VERSION' packages/coordify-core/src/paths.rs
```

Commit the bump: `chore(release): bump to X.Y.Z`.

## 3. CHANGELOG

Ensure `CHANGELOG.md` has an entry for the version under `## [X.Y.Z] - YYYY-MM-DD`
following Keep a Changelog format. The release workflow extracts this section
for the GitHub Release body. Review it for:
- Accurate feature/fix lists.
- No accidental secret leakage (tokens, paths, emails).
- Breaking changes called out under `### Changed`.

## 4. Cut the tag

```bash
git tag -a vX.Y.Z -m "Coordify X.Y.Z"
git push origin vX.Y.Z
```

Pushing the tag triggers the `release.yml` workflow automatically.

## 5. Watch the release workflow

```bash
gh run list --workflow=release.yml --limit 3
gh run watch <run-id>
```

The workflow:
1. `publish-core` — `cargo publish` to crates.io.
2. `publish-npm` — `npm publish --provenance` for all three npm packages.
3. `github-release` — creates the GitHub Release with notes from CHANGELOG.md.

If any job fails:
- **crates.io publish fails** (version exists, metadata invalid, token bad):
  fix, bump the patch version, re-tag. Do **not** try to re-publish the same
  version — crates.io rejects it.
- **npm publish fails** (version exists, 2FA, token scope): same — bump patch,
  re-tag. npm also rejects re-publish of the same version.
- **github-release fails**: re-run the job via the Actions UI; no version
  conflict to worry about.

## 6. Post-release verification

```bash
# From a clean machine:
cargo install coordify-core
npm install -g coordify-hook coordify-cli coordify-sim

# Verify versions
coordify-core --version
coordify --version
coordify-sim --version

# Verify the GitHub Release exists and notes are correct
gh release view vX.Y.Z
```

## 7. Announce

Only after step 6 passes:
- Update README "latest version" badge if present.
- Post to the project's announcement channels.
- If a beta cohort exists, notify them.

## 8. Rollback (if something is wrong post-publish)

- **crates.io:** `cargo yank --vers X.Y.Z coordify-core`. Yank removes the
  version from the index for new installs but does not delete it. Existing
  `Cargo.lock` files pinning X.Y.Z still resolve. There is no un-yank.
- **npm:** `npm deprecate coordify-hook@X.Y.Z "use X.Y.Z+1"`. npm has no
  unpublish after 72 hours for packages with installs.
- **GitHub Release:** delete via `gh release delete vX.Y.Z`. The tag stays
  unless you also `git push origin :refs/tags/vX.Y.Z`.
- Cut a patch release (X.Y.Z+1) with the fix and re-run steps 4–6.

## 9. Rotate secrets

After each release:
- Rotate `NPM_TOKEN` (granular tokens have expiry; create a new one, update
  the GitHub Secret, revoke the old one).
- `CARGO_REGISTRY_TOKEN` does not expire but should be rotated quarterly or
  on any suspected compromise.
