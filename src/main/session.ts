import { mkdir, readFile, rename, unlink, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { BrowserWindow, app, safeStorage, session, shell } from 'electron'
import {
  InteractionRequiredAuthError,
  PublicClientApplication,
  type AccountInfo,
  type AuthenticationResult,
  type ICachePlugin
} from '@azure/msal-node'
import { ADO_RESOURCE_SCOPE, ENTRA_AUTHORITY } from '@shared/types'
import type { SessionInfo } from '@shared/types'
import { entraClientIdFromEnv } from './auth-mode'
import { createMsalCachePlugin, shouldPersistSession, type CacheStore } from './msal-cache'
import { createPatTokenProvider } from './pat-session'

const SCOPES = [ADO_RESOURCE_SCOPE]
const SSO_PARTITION = 'persist:ado-planner-sso'

export type LoginCreds = {
  pat?: string
  organization?: string
}

export type TokenProvider = {
  scheme: 'bearer' | 'pat'
  getAccessToken(): Promise<string>
  getOrganization(): Promise<string | null>
  getSessionInfo(): Promise<SessionInfo>
  login(creds?: LoginCreds): Promise<SessionInfo>
  logout(): Promise<void>
}

type SafeStorageLike = {
  getSelectedStorageBackend?: () => string
  encryptString: (plain: string) => Buffer
  decryptString: (cipher: Buffer) => string
}

export function isE2e(): boolean {
  return process.env.ADO_PLANNER_E2E === '1'
}

async function atomicWrite(filePath: string, data: Buffer): Promise<void> {
  await mkdir(dirname(filePath), { recursive: true })
  const tmp = `${filePath}.tmp`
  await writeFile(tmp, data)
  await rename(tmp, filePath)
}

function createSafeStorageStore(filePath: string, persistToDisk: boolean): CacheStore {
  const storage = safeStorage as unknown as SafeStorageLike
  return {
    persistToDisk,
    async read() {
      try {
        const buf = await readFile(filePath)
        return storage.decryptString(buf)
      } catch {
        return null
      }
    },
    async write(serialized) {
      const cipher = storage.encryptString(serialized)
      await atomicWrite(filePath, cipher)
    },
    async unlink() {
      try {
        await unlink(filePath)
      } catch {
        /* missing is fine */
      }
    }
  }
}

export function createE2eTokenProvider(): TokenProvider {
  const info: SessionInfo = {
    signedIn: true,
    displayName: process.env.ADO_PLANNER_E2E_DISPLAY_NAME ?? 'Ada Lovelace',
    username: process.env.ADO_PLANNER_E2E_USERNAME ?? 'ada@contoso.com',
    authMode: 'entra'
  }
  return {
    scheme: 'bearer',
    async getAccessToken() {
      return 'e2e-token'
    },
    async getOrganization() {
      return null
    },
    async getSessionInfo() {
      return info
    },
    async login() {
      return info
    },
    async logout() {
      info.signedIn = false
      info.displayName = null
      info.username = null
    }
  }
}

export async function createSessionTokenProvider(): Promise<TokenProvider> {
  if (isE2e()) {
    return createE2eTokenProvider()
  }

  const persistToDisk = shouldPersistSession({
    platform: process.platform,
    encryptionAvailable: safeStorage.isEncryptionAvailable(),
    storageBackend:
      process.platform === 'linux' && typeof safeStorage.getSelectedStorageBackend === 'function'
        ? safeStorage.getSelectedStorageBackend()
        : undefined
  })
  const sessionDir = join(app.getPath('userData'), 'session')
  const storeFor = (file: string): CacheStore =>
    createSafeStorageStore(join(sessionDir, file), persistToDisk)

  const clientId = entraClientIdFromEnv()
  if (!clientId) {
    return createPatTokenProvider({ store: storeFor('pat.bin') })
  }

  const plugin: ICachePlugin & { wipe(): Promise<void> } = createMsalCachePlugin(
    storeFor('msal.bin')
  )

  const pca = new PublicClientApplication({
    auth: {
      clientId,
      authority: ENTRA_AUTHORITY
    },
    cache: { cachePlugin: plugin }
  })

  const accountOf = (result: AuthenticationResult | null): AccountInfo | null =>
    result?.account ?? null

  let lastAccount: AccountInfo | null = null

  const silentOrInteractive = async (): Promise<AuthenticationResult> => {
    const accounts = await pca.getTokenCache().getAllAccounts()
    const account = lastAccount ?? accounts[0]
    if (account) {
      try {
        const silent = await pca.acquireTokenSilent({ account, scopes: SCOPES })
        if (silent) {
          lastAccount = accountOf(silent)
          return silent
        }
      } catch (error) {
        if (!(error instanceof InteractionRequiredAuthError)) {
          throw error
        }
      }
    }
    const interactive = await pca.acquireTokenInteractive({
      scopes: SCOPES,
      openBrowser: async (url) => {
        await shell.openExternal(url)
      }
    })
    lastAccount = accountOf(interactive)
    return interactive
  }

  return {
    scheme: 'bearer',
    async getAccessToken() {
      const result = await silentOrInteractive()
      return result.accessToken
    },
    async getOrganization() {
      return null
    },
    async getSessionInfo() {
      const accounts = await pca.getTokenCache().getAllAccounts()
      const account = lastAccount ?? accounts[0]
      if (!account) {
        return { signedIn: false, displayName: null, username: null, authMode: 'entra' }
      }
      return {
        signedIn: true,
        displayName: account.name ?? account.username,
        username: account.username,
        authMode: 'entra'
      }
    },
    async login() {
      await silentOrInteractive()
      const accounts = await pca.getTokenCache().getAllAccounts()
      const account = lastAccount ?? accounts[0]
      if (!account) {
        return { signedIn: false, displayName: null, username: null, authMode: 'entra' }
      }
      return {
        signedIn: true,
        displayName: account.name ?? account.username,
        username: account.username,
        authMode: 'entra'
      }
    },
    async logout() {
      const accounts = await pca.getTokenCache().getAllAccounts()
      for (const account of accounts) {
        await pca.getTokenCache().removeAccount(account)
      }
      lastAccount = null
      await plugin.wipe()
      await session.fromPartition(SSO_PARTITION).clearStorageData()
      for (const win of BrowserWindow.getAllWindows()) {
        win.webContents.session.clearStorageData().catch(() => undefined)
      }
    }
  }
}
