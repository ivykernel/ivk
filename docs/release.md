# Release procedure

This document describes how to cut a new `ivk` release and ship it via Homebrew.

The mechanism is **cargo-dist + a custom Homebrew tap at `ivykernel/homebrew-tap`**. We deliberately do not target Homebrew core: a custom tap removes the months-long review queue and keeps full control of the formula.

## One-time setup (already done)

1. `[workspace.metadata.dist]` in [`Cargo.toml`](../Cargo.toml) declares targets, installers, and the tap location.
2. CI lives in [`.github/workflows/ci.yml`](../.github/workflows/ci.yml). cargo-dist generates a separate `release.yml` on `cargo dist init`.

## Per-release procedure

```bash
# 0. Bump versions in every crate's Cargo.toml. The three of them are:
#       crates/ivk-core/Cargo.toml
#       crates/ivk-cli/Cargo.toml
#       crates/clonewt/Cargo.toml
#    Keep them in lockstep until we split publishing schedules.

# 1. Refresh the cargo-dist plan (regenerates release workflow if needed).
cargo install cargo-dist --version 0.27.0   # if not installed
cargo dist init --yes                       # idempotent
cargo dist plan                             # dry-run: prints what would happen

# 2. Sanity check: tests + clippy + fmt locally.
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --release

# 3. Tag and push.
git tag v0.1.0
git push origin v0.1.0

# 4. The release workflow on GitHub Actions will:
#       - build x86_64 + aarch64 binaries for macOS and Linux
#       - upload them to a GitHub Release for the tag
#       - open a PR against ivykernel/homebrew-tap with the new Formula
# Merge that PR; brew tap users get the update on next `brew update`.

# 5. Verify the install path.
brew tap ivykernel/tap
brew install ivk
ivk --version            # should print v0.1.0
ivk help --agent
```

## Homebrew tap repo

The tap lives at `ivykernel/homebrew-tap`. Bootstrap once:

```bash
gh repo create ivykernel/homebrew-tap --public --description "Homebrew tap for ivykernel projects"
git clone https://github.com/ivykernel/homebrew-tap.git
cd homebrew-tap
mkdir -p Formula
# cargo-dist's first release PR will populate Formula/ivk.rb automatically.
```

Subsequent releases land as automated PRs from the main `ivykernel/ivk` repo's release workflow.

## Things to verify before tagging

- [ ] `README.md` install snippet matches the version about to be tagged
- [ ] [`results/summary.md`](../results/summary.md) and [`results/build-summary.md`](../results/build-summary.md) numbers reflect the build going out
- [ ] [`docs/index.html`](./index.html) hero numbers are consistent
- [ ] `examples/demo-task/setup.sh` and `examples/todo-100/setup.sh` still complete cleanly with the new binary
- [ ] Plan checkboxes for the phases included in this release are up to date

## Things that can wait until after v0.1.0

- Homebrew core submission (we are explicitly Phase 9-deferred on this)
- macOS notarization / codesigning of release binaries (only relevant if signed
  installers are needed; brew users don't see this)
- Windows / ReFS support (out of scope per [docs/portability.md](./portability.md))
