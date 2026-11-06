## Agent skills

### Issue tracker

Issues live in this repo's GitHub Issues. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-role vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### Dev session

CDP / HMR / mocks: probe `127.0.0.1:9222` and attach to the user's `pnpm dev` window to see what they see; use Playwright e2e with mocks for scripted iteration. See `docs/agents/dev-session.md`.

## Git

### Commits

Conventional Commits: `type(scope): description`. Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.

### PR titles and releases

PRs are squash-merged, so the PR title becomes the commit on `master`. It must be a Conventional Commit, `type(scope)!: subject`, and the PR title check fails the PR otherwise.

The type sets the SemVer bump of the next release:

- `feat`: minor.
- `fix`, `perf`: patch.
- `!` after the type, or a `BREAKING CHANGE:` footer: major.
- Any other type (`ci`, `chore`, `docs`, `refactor`, `test`, `build`, `style`, `revert`): no release.

Pick the type for the change you want released. For example, `ci: Rust CI/CD release pipeline` ships nothing, while `feat: show the version on the sign-in card` bumps minor.

Don't edit versions (`Cargo.toml`, `Cargo.lock`, `package.json`) or `CHANGELOG.md` by hand. The release PR, `chore(release): vX.Y.Z`, owns them. See `docs/RELEASING.md`.

### Worktree branches

Conventional branch: `type/short-kebab-description` (same types as commits). Use this name for every worktree and its branch.
