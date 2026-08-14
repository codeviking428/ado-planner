# PAT Session persistence

The proper way for this Electron 39 app to remember a PAT Session across launches is: after `app.whenReady()`, encrypt `{ pat, organization }` with Electron `safeStorage.encryptString`, write the ciphertext to `app.getPath('userData')/session/pat.bin`, and on the next launch decrypt that file in the main process so `session:get` returns `signedIn: true`. Persist whenever `safeStorage.isEncryptionAvailable()` is true, except on Linux when `getSelectedStorageBackend()` is `basic_text` or `unknown`. Do not use `getSelectedStorageBackend()` as a persist gate on Windows or macOS — that method is Linux-only and is not registered on those platforms. The current `'unknown'` fallback therefore refuses to write or read `pat.bin` on Windows and forgets the Session every launch.

## Recommendation

- Keep Electron `safeStorage` + `userData/session/pat.bin`. That is Electron's official secret-at-rest pattern: the API encrypts a string with the OS crypto backend; the app stores the `Buffer` itself. On Windows this is DPAPI (`CryptProtectData`), which Microsoft lists as the correct local-secret API for desktop apps.
- Persist-gate (call only after `app.whenReady()`):

  ```ts
  function shouldPersistSession(): boolean {
    if (!safeStorage.isEncryptionAvailable()) return false
    if (process.platform === 'linux') {
      const backend = safeStorage.getSelectedStorageBackend()
      return backend !== 'basic_text' && backend !== 'unknown'
    }
    return true
  }
  ```

- Keep `{ pat, organization }` JSON inside the encrypted Session blob. The PAT is a password-equivalent secret. The organization slug is not a secret, but Session restore requires both fields (`createPatTokenProvider` rejects PAT-only storage). Co-locating them is proper.
- Logout = `unlink` `pat.bin` (already done).
- Do not add `keytar`. Electron 39's first-party API is `safeStorage`. Microsoft also allows Windows Credential Manager; Electron does not wrap it.
- Do not put the PAT in renderer `localStorage`, cookies, or plain JSON under `userData`. Decrypt only in the main process; IPC should return `SessionInfo`, not the token.
- Electron 39 has the synchronous `encryptString` / `decryptString` API only. The async `encryptStringAsync` / `decryptStringAsync` APIs appear in later Electron docs, not in v39.2.6.

## Current code vs official API

What we already do that matches the official API:

- Main process, after `app.whenReady()`, builds the token provider (`src/main/index.ts`).
- `createSafeStorageStore` encrypts a string with `safeStorage.encryptString` and writes the `Buffer` atomically to `app.getPath('userData')/session/pat.bin`.
- `createPatTokenProvider` serializes `{ pat, organization }` (org normalized to the slug).
- Linux `basic_text` is memory-only. That matches Electron's warning that `basic_text` is a hardcoded-password obfuscation, not a secret store.
- Logout unlinks the file.
- Renderer asks `session:get` and shows the sign-in shell only when `signedIn` is false.

Mismatch that forgets creds on every Windows launch:

1. `createSessionTokenProvider` treats a missing `getSelectedStorageBackend` as `'unknown'`.
2. `shouldPersistSession` returns `false` for `'unknown'`.
3. `getSelectedStorageBackend` is compiled in only on Linux (`#if BUILDFLAG(IS_LINUX)`). On Windows and macOS the method is not registered, so `typeof … === 'function'` is false.
4. Electron's type union is `'basic_text' | 'gnome_libsecret' | 'kwallet' | 'kwallet5' | 'kwallet6' | 'unknown'`. It never returns `'dpapi'`.
5. Tests pass `storageBackend: 'dpapi'` on win32, a value the runtime API cannot produce.

On Windows the live path is therefore `backend = 'unknown'` → `persistToDisk = false` → login never writes `pat.bin` and launch never reads it. That is a bug. The README claim ("stored with Electron safeStorage") is the intended design; the persist gate prevents it on win32/darwin.

`node_modules/electron` was not present in this workspace at research time. Types and C++ below are from the published `electron@39.2.6` package and the `v39.2.6` tag.

## Electron safeStorage

Official module purpose: "simple encryption and decryption of strings for storage on the local machine" using OS-provided cryptography. It does not persist. The app writes the returned `Buffer`.

| Method (Electron 39.2.6)                  | Fact                                                                                                                                     |
| ----------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `encryptString(plainText)`                | Returns `Buffer`. Throws if encryption fails, including `"safeStorage cannot be used before app is ready"`.                              |
| `decryptString(encrypted)`                | Returns the string. Throws if decryption fails or the buffer lacks the `v10`/`v11` prefix.                                               |
| `isEncryptionAvailable()`                 | Linux: true after `ready` **and** a secret key is available. macOS: true if Keychain is available. Windows: true once `ready` has fired. |
| `getSelectedStorageBackend()`             | **Linux only.** Returns the password-manager name, or `unknown` if called before `ready`.                                                |
| `setUsePlainTextEncryption(usePlainText)` | Linux: force the in-memory/hardcoded password path when no OS manager is found. **No-op on Windows and macOS.**                          |

`app.whenReady()` must have fired before encrypt/decrypt. Electron 39 C++ throws `"safeStorage cannot be used before app is ready"` when encryption is unavailable and the browser is not ready. This app already constructs the provider inside `app.whenReady().then(…)`, which is correct.

Current (non-39) Electron docs recommend `encryptStringAsync` / `decryptStringAsync`. Those methods are absent from the v39.2.6 docs, `electron.d.ts`, and `electron_api_safe_storage.cc`. Stay on the sync API until the app upgrades.

Sources: [1], [2], [3], [4].

## Platform backends

| Platform | Backend                                                                                                                           | How you detect it                                                                             | Persist?                                                                     |
| -------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| Windows  | DPAPI (`CryptProtectData`). Same user logon can decrypt; other users on the machine cannot; other apps in the same userspace can. | `isEncryptionAvailable()` after `ready`. `getSelectedStorageBackend` **does not exist**.      | Yes, after `ready`.                                                          |
| macOS    | Keychain Access. Other apps cannot load the key without user override. Calls can block for a Keychain prompt.                     | `isEncryptionAvailable()` (Keychain present). `getSelectedStorageBackend` **does not exist**. | Yes, when Keychain is available.                                             |
| Linux    | `gnome_libsecret`, `kwallet`, `kwallet5`, `kwallet6` when a desktop secret store is selected.                                     | `getSelectedStorageBackend()` after `ready`.                                                  | Yes, when the backend is a real store and `isEncryptionAvailable()` is true. |
| Linux    | `basic_text` — desktop unknown, or `--password-store=basic`. Ciphertext is "encrypted" with a hardcoded plaintext password.       | `getSelectedStorageBackend() === 'basic_text'`.                                               | No. Memory-only.                                                             |
| Linux    | `unknown`                                                                                                                         | Function called before `ready`.                                                               | No. Wait for `ready` and ask again.                                          |

`getSelectedStorageBackend()` on non-Linux: the Electron 39 docs mark it `_Linux_`. The v39.2.6 types annotate `@platform linux`. The v39.2.6 C++ registers the method only inside `#if BUILDFLAG(IS_LINUX)`. Calling it is not defined on Windows/macOS; at runtime the property is missing (not a throw of `unknown` after ready). `unknown` means "called before `ready`" **on Linux**, not "Windows DPAPI".

Windows DPAPI quote that Electron cites: "Typically, only a user with the same logon credential as the user who encrypted the data can typically decrypt the data." Encryption and decryption usually must be on the same computer (roaming profiles are the documented exception). App User Model ID (`app.setAppUserModelId`) is a Windows taskbar/toast identity. Official Electron and DPAPI docs do not tie AUMID to the DPAPI key. Changing `com.ado-planner.app` does not, by itself, invalidate `pat.bin`.

Sources: [1], [2], [3], [5], [6].

## What to encrypt vs what is preference

**PAT — secret.** Microsoft: a PAT "acts as an alternative password"; "treat PATs with the same level of caution as passwords"; "keep it confidential"; "Don't share PATs"; "store it in a secure location"; "Store your PATs in a secure key management solution, like Azure Key Vault." Auth guidance calls a PAT a "long-lived bearer secret" and the highest-risk common choice. Azure DevOps stores only an HMAC of the raw PAT, not the raw value. Do not persist the PAT as plaintext JSON, `localStorage`, or cookies.

**Organization URL/slug — not a secret.** It is the public path segment in every Azure DevOps Services URL (`https://dev.azure.com/{organization}/_apis/...`) and in the sign-in URL (`https://dev.azure.com/{Your_Organization}`). Microsoft never classifies the org name as a credential.

**Where the org slug should live.** Either is proper:

- Same encrypted Session blob (current design). `safeStorage` encrypts a string; a JSON Session `{ pat, organization }` is the intended use. Restore needs both fields. Encrypting a non-secret next to a secret is not a leak of the PAT.
- Separate unencrypted preference under `userData`. Valid because the slug is not a secret. Then `pat.bin` holds only the PAT, and restore must read both files. Incomplete PAT-only storage already fails closed (`signedIn: false`).

Prefer the single encrypted Session blob. Session is "the securely stored login" (`CONTEXT.md`). Splitting saves nothing on Windows/macOS and adds a second restore path.

Do not store the PAT in renderer storage. Electron's security tutorial keeps secrets behind validated main-process IPC and forbids exposing Electron APIs to untrusted content. This app already uses `sandbox`, `contextIsolation`, and `nodeIntegration: false`. Keep it that way.

Sources: [7], [8], [9], [10], [11], [12], [13].

## Restore-on-launch

Main-process sequence after a previous successful persist:

1. If overriding `userData`, call `app.setPath('userData', …)` **before** `ready` (this app already does for `ADO_PLANNER_USER_DATA_DIR`).
2. Await `app.whenReady()`.
3. Confirm `safeStorage.isEncryptionAvailable()`. On Linux, also confirm `getSelectedStorageBackend()` is a real store.
4. `readFile(join(app.getPath('userData'), 'session', 'pat.bin'))`.
5. `safeStorage.decryptString(buf)` → JSON `{ pat, organization }` → in-memory Session.
6. Register IPC. `session:get` → `{ signedIn: true, displayName: 'PAT', authMode: 'pat' }`.
7. Create the window. The renderer query `window.planner.session.get()` skips `SignInShell` when `signedIn` is true.

Gotchas:

- **Encrypt/decrypt before `ready`.** Throws `"safeStorage cannot be used before app is ready"`.
- **`persistToDisk === false` skips both write and read.** Even an existing `pat.bin` is ignored. The Windows `'unknown'` gate hits this.
- **`userData` follows the app name.** `userData` is `appData` + the app name. Electron prefers `package.json` `productName` over `name`. This repo's `package.json` `name` is `ado-planner`; `electron-builder.yml` `productName` is `ADO Planner`. `electron-vite dev` and a packaged build can use different directories (`%APPDATA%\ado-planner` vs `%APPDATA%\ADO Planner`). A Session saved in one will not appear in the other. That is a path split, not a DPAPI failure.
- **`ADO_PLANNER_USER_DATA_DIR`.** A different override per launch looks like a forgotten Session.
- **App User Model ID** does not scope DPAPI. Safe to keep `electronApp.setAppUserModelId('com.ado-planner.app')`.
- **Packaged vs `electron-vite dev`.** Same `safeStorage` API. The usual break is the `userData` name above, not packing itself.
- **Linux Keychain/secret-store prompts** can block the encrypt/decrypt thread. After the user unlocks the store once, later launches should decrypt without a login form.
- **Expired PAT.** Microsoft: when a PAT expires or is revoked, callers get `401` / "invalid or expired". Restore still yields `signedIn: true` until a REST call fails. That is token lifetime, not storage. Entra-backed orgs: a new PAT that is never used within 90 days becomes inactive.

Sources: [1], [2], [4], [5], [7], [14].

## Sources

1. Electron 39.2.6 `safeStorage` docs — https://github.com/electron/electron/blob/v39.2.6/docs/api/safe-storage.md
2. Electron 39.2.6 implementation — https://github.com/electron/electron/blob/v39.2.6/shell/browser/api/electron_api_safe_storage.cc
3. Electron 39.2.6 `SafeStorage` types (`@platform linux` on `getSelectedStorageBackend`) — https://cdn.jsdelivr.net/npm/electron@39.2.6/electron.d.ts (interface around the `SafeStorage` block; local install: `node_modules/electron/electron.d.ts`)
4. Current Electron `safeStorage` docs (async API is newer than 39) — https://www.electronjs.org/docs/latest/api/safe-storage
5. Electron `app.getPath('userData')` / `app.getName()` / `app.setAppUserModelId` — https://www.electronjs.org/docs/latest/api/app
6. Windows DPAPI `CryptProtectData` — https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata
7. Use personal access tokens (password-equivalent, store securely, lifetime) — https://learn.microsoft.com/en-us/azure/devops/organizations/accounts/use-personal-access-tokens-to-authenticate?view=azure-devops
8. Azure DevOps authentication guidance (PAT = long-lived bearer secret) — https://learn.microsoft.com/en-us/azure/devops/integrate/get-started/authentication/authentication-guidance?view=azure-devops
9. Azure DevOps credential storage (raw PAT not stored server-side) — https://learn.microsoft.com/en-us/azure/devops/organizations/security/data-protection?view=azure-devops
10. Azure DevOps REST URL structure (`dev.azure.com/{organization}`) — https://learn.microsoft.com/en-us/azure/devops/integrate/how-to/call-rest-api?view=azure-devops
11. Windows handling passwords (DPAPI for local secret storage; never plaintext) — https://learn.microsoft.com/en-us/windows/win32/secbp/handling-passwords
12. Electron security tutorial (secrets behind validated main-process IPC) — https://www.electronjs.org/docs/latest/tutorial/security
13. Chromium OSCrypt (OS-user-bound crypto that Electron wraps) — https://chromium.googlesource.com/chromium/src/+/main/components/os_crypt/README.md
14. This app: `src/main/session.ts`, `src/main/pat-session.ts`, `src/main/msal-cache.ts`, `src/main/index.ts`, `src/main/ipc.ts`, `src/renderer/src/App.tsx`, `CONTEXT.md` (Session), `package.json` (`electron: ^39.2.6`), `electron-builder.yml` (`productName: ADO Planner`)
