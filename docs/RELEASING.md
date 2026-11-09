# Releasing

Every release is a `vX.Y.Z` tag on `master` and one GitHub Release holding the app's installers and binaries. A person approves each release by merging its release PR.

## Workflows

| Workflow                           | Runs on                                           | Does                                                                                                                         |
| ---------------------------------- | ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `.github/workflows/ci.yml`         | every PR, every push to `master`                  | fmt, clippy, nextest, build, and `cargo deny` on Linux, Windows, and macOS; actionlint, shellcheck, and release script tests |
| `.github/workflows/pr-title.yml`   | PR opened, edited, or pushed                      | rejects PR titles that are not Conventional Commits                                                                          |
| `.github/workflows/release-pr.yml` | every push to `master`, manual on `master` only   | opens or updates the release PR; after it merges, tags the version-bump commit and dispatches `release.yml` on that tag      |
| `.github/workflows/release.yml`    | a `vX.Y.Z` tag push or dispatch, release-file PRs | builds, signs, checksums, and publishes the GitHub Release                                                                   |

## How the version is chosen

[git-cliff](https://git-cliff.org) reads the Conventional Commits on `master` since the last `vX.Y.Z` tag (`cliff.toml`):

| Commits since the last tag                                      | Bump  | Example         |
| --------------------------------------------------------------- | ----- | --------------- |
| any `BREAKING CHANGE:` footer or `!` after the type             | major | `0.4.2 → 1.0.0` |
| any `feat`                                                      | minor | `0.4.2 → 0.5.0` |
| only `fix` or `perf`                                            | patch | `0.4.2 → 0.4.3` |
| only other types (`docs`, `ci`, `chore`, `refactor`, `test`, …) | none  | no release PR   |

Below 1.0.0 the same rules apply: a breaking change still goes to the next major.

PRs are squash-merged, so the **PR title** becomes the commit that counts. CI rejects PR titles that are not Conventional Commits. Breaking changes need `!` in the title (`feat(api)!: …`) or a `BREAKING CHANGE:` footer in the squash message.

The workspace version in the root `Cargo.toml` is the single version. Every crate inherits it with `version.workspace = true`. CI runs `.github/scripts/release-version.sh check` to keep them in step. The binary embeds it: `ado-planner --version` prints it, and the sign-in card and header show it.

## Cutting a release

1. Merge PRs to `master` as usual. After each push, **Release PR** opens or refreshes the PR `chore(release): vX.Y.Z` on branch `release/next`. That PR bumps `Cargo.toml` and `Cargo.lock`, and prepends the new section to `CHANGELOG.md`.
2. Review it. To reword the notes, edit `CHANGELOG.md` on that branch. The next push to `master` regenerates the PR and drops those edits.
3. Merge it. **Release PR** sees a version with no tag, creates `vX.Y.Z` on the commit that set that version (the release PR's merge commit), and starts **Release** on that tag. Commits pushed after it are not in the release; they go into the next release PR, which the same run opens.
4. **Release** builds the following and publishes them to the `vX.Y.Z` GitHub Release, marked latest:

   | Asset                                                | What                             |
   | ---------------------------------------------------- | -------------------------------- |
   | `ado-planner-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` | binary for Linux                 |
   | `ado-planner-vX.Y.Z-x86_64-pc-windows-msvc.zip`      | binary for Windows               |
   | `ado-planner-vX.Y.Z-x86_64-setup.exe` (+ `.sig`)     | NSIS installer, all users        |
   | `ado-planner-vX.Y.Z-x86_64.AppImage` (+ `.sig`)      | AppImage                         |
   | `latest.json`                                        | updater feed                     |
   | `latest.yml`, `latest-linux.yml`                     | Electron migration feeds (below) |
   | `SHA256SUMS`                                         | checksums for every file above   |

The notes come from that version's `CHANGELOG.md` section.

The Windows installer installs for all users to `C:\Program Files\ADO Planner` and asks UAC for admin rights. It removes a per-user install from releases up to 0.2.1 and any Electron install, and leaves user data in `%APPDATA%\ado-planner` alone. Updates also need admin rights, so a standard user gets a UAC credential prompt on every update. Before publishing, the **Windows installers** job installs v0.2.1 per-user and then the new build on a runner, then the v0.2.1 Electron app and then the new build, and checks the paths, both migrations, the shortcuts, and that user data survives. The NSIS script is `crates/app/packaging/windows/installer.nsi`, cargo-packager 0.11.8's template plus marked changes; CI fails if it drifts from that version.

The Linux binaries are built on Ubuntu 22.04 and need glibc 2.35 or newer (Ubuntu 22.04+, Debian 12+, Fedora 36+). The release stops if a build needs a newer glibc.

The app's updater reads the release GitHub marks as latest. It fetches `releases/latest/download/latest.json` and checks each download against the minisign public key built into the binary. Before publishing, the job verifies every `.sig` against that key with `minisign`, so a mismatched key pair stops the release.

### Other ways in

- **Rebuild a tag:** Actions → Release → Run workflow, and set _Use workflow from_ to the `vX.Y.Z` tag. A branch is rejected. Assets with the same name are replaced. The release is marked latest only if its tag is the highest `vX.Y.Z`, so rebuilding an older tag leaves updaters on the newest release. Tick _Build without publishing_ for a dry run.
- **Push a tag by hand:** `git tag -a v1.2.3 -m "ADO Planner 1.2.3" && git push origin v1.2.3`. The tag must match the `Cargo.toml` version at that commit and point at a commit on `master`, or the build stops.
- **Dry run on a PR:** PRs that touch `release.yml`, `.github/scripts/`, `crates/app/Cargo.toml`, `crates/app/packaging/`, or `cliff.toml` run the whole build. A throwaway key signs it, nothing is published, and the result is uploaded as the `release-dry-run-vX.Y.Z` artifact.
- **Caches:** release builds neither restore nor save Actions caches, and they always build the tag they run on, never a ref passed in. Code built for a release therefore can't write a cache that a `master` CI run would restore.
- **Preview the next version locally:** `git cliff --bumped-version` and `git cliff --unreleased --bump`.

## Electron migration

Releases before 1.0.0 also shipped an Electron app, which updates with electron-updater from `latest.yml` (Windows) and `latest-linux.yml` (Linux) in the latest release. **Release** still publishes those two files, written by `.github/scripts/write-electron-feed.py`, but they list this app's installers. An Electron user sees the usual update prompt, and Yes installs this app:

- **Windows:** electron-updater checks the SHA-512 from `latest.yml` and runs `ado-planner-vX.Y.Z-x86_64-setup.exe` with admin rights. The installer runs each Electron uninstaller it finds (per-user and per-machine) before it creates its own shortcuts, and keeps `%APPDATA%\ado-planner`.
- **Linux:** electron-updater downloads `ado-planner-vX.Y.Z-x86_64.AppImage` beside the old AppImage, deletes the old one, and starts the new one.

Users sign in again and pick their Scope, filters, and Flavor again: the Electron app kept its Session in `safeStorage` and its settings in Chromium storage, and this app reads neither.

Keep publishing the two feeds while Electron installs may still be out there; they cost nothing. To stop, delete the **Write Electron migration feeds** step in `release.yml`. Electron apps that are still installed then show an "Updater failed" toast on every launch and never offer the new app.

## Secrets and settings

Set these under Settings → Secrets and variables → Actions. No workflow hardcodes a key.

| Secret                         | Required            | Value                                                                          |
| ------------------------------ | ------------------- | ------------------------------------------------------------------------------ |
| `UPDATER_PRIVATE_KEY`          | yes                 | contents of the private key file from `cargo packager signer generate`         |
| `UPDATER_PRIVATE_KEY_PASSWORD` | yes                 | that key's password                                                            |
| `UPDATER_PUBLIC_KEY`           | yes                 | contents of the matching `.pub` file; it is compiled into the app              |
| `RELEASE_PR_TOKEN`             | no, but recommended | fine-grained PAT or GitHub App token with _Contents_ and _Pull requests_ write |

Generate the key pair once and keep it. Installed apps trust only the public key they shipped with, so a new pair cuts them off from updates.

```bash
cargo install cargo-packager --locked
cargo packager signer generate --path ./ado-planner-updater.key   # prompts for a password
# UPDATER_PRIVATE_KEY = contents of ado-planner-updater.key
# UPDATER_PUBLIC_KEY  = contents of ado-planner-updater.key.pub
```

Repository settings:

- Settings → Actions → General → Workflow permissions: allow GitHub Actions to create and approve pull requests. Without this, the release PR can't be opened with `GITHUB_TOKEN`.
- Without `RELEASE_PR_TOKEN`, GitHub does not run CI on the release PR, because `GITHUB_TOKEN` events don't trigger workflows. If branch protection requires CI checks, set the token or the PR can't merge.
- Settings → General → Pull Requests: allow squash merging and default the commit message to the PR title.

## Not covered yet

- macOS: the app doesn't ship for Mac until Apple signing exists. See README → Publishing for Mac.
- Windows Authenticode signing: installers are not code-signed, so SmartScreen warns on first run. Updater signatures are separate and are checked.
