# ADO Planner

Monday-lite Electron app: an Azure DevOps **Team** Work Item **Hierarchy** (Epic down to Task) on a **Gantt**, with Entra work/school SSO, date drag-edit, and a layout-driven Work Item form.

## Run

```bash
pnpm install
cp .env.example .env   # set ENTRA_CLIENT_ID, or leave empty for PAT fallback
pnpm dev
```

```bash
pnpm test          # Vitest (Hierarchy, dates, overlays, form PATCH, Session cache)
pnpm e2e           # Playwright `_electron` against a loopback ADO mock (needs a display / xvfb-run on Linux CI)
pnpm build:linux   # AppImage
pnpm build:win     # NSIS per-user
```

Linux e2e without a desktop session:

```bash
xvfb-run -a pnpm e2e
```

## Entra app registration

Ship **one public-client ID**. Maintainers register it once; it is not a secret. End users do not register their own.

1. Entra admin center → App registrations → New registration
2. Supported account types: **Accounts in any organizational directory** (not personal Microsoft accounts)
3. Platform: **Mobile and desktop applications**
4. Redirect URI: `http://localhost` (port is ignored on loopback)
5. Authentication → **Allow public client flows**
6. API permissions → **Azure DevOps** delegated (not Graph):
   - `vso.work` + `vso.work_write` — Hierarchy, Gantt-bar drag, form save
   - `vso.project` — projects and Teams
   - `vso.profile` — org listing
7. Put the Application (client) ID in `ENTRA_CLIENT_ID` / `MAIN_VITE_ENTRA_CLIENT_ID`
8. Do **not** add a client secret

If those env vars are empty, the app asks for a personal access token at startup and stores it with Electron `safeStorage` (Linux `basic_text` is memory-only, same as Entra Session). Log out wipes it. This is a fallback until the Entra public client is registered.

Authority is `https://login.microsoftonline.com/organizations`. Token scope is `499b84ac-1321-427f-aa17-267ca6975798/.default`.

Sign-in uses MSAL Node `acquireTokenInteractive` (PKCE, loopback, `shell.openExternal`). Session is stored in main with Electron `safeStorage` wrapping an MSAL `ICachePlugin`. On Linux, if `safeStorage.getSelectedStorageBackend() === 'basic_text'`, the Session is memory-only.

## Packaging

GitHub Releases: unsigned NSIS (Windows, per-user) + AppImage (Linux). `electron-updater` with `autoDownload: false`. Startup modal: Yes = download+install+restart; No snoozes until next launch. Not `npm i -g`. macOS is out of scope.

## Spec

Locked on [Monday-lite ADO Gantt spec](https://github.com/codeviking428/ado-planner/issues/1).
