import { test as base, expect, _electron as electron } from '@playwright/test'
import type { ElectronApplication, Page } from '@playwright/test'
import { mkdtemp } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { startAdoMock, type AdoMock } from './ado-mock'

const repoRoot = join(__dirname, '..')

type Fixtures = {
  adoMock: AdoMock
  electronApp: ElectronApplication
  window: Page
}

export const test = base.extend<Fixtures>({
  adoMock: async ({}, use) => {
    const mock = await startAdoMock()
    await use(mock)
    await mock.close()
  },
  electronApp: async ({ adoMock }, use) => {
    const userDataDir = await mkdtemp(join(tmpdir(), 'ado-planner-e2e-'))
    const env = {
      ...process.env,
      ADO_PLANNER_E2E: '1',
      ADO_PLANNER_USER_DATA_DIR: userDataDir,
      ADO_PLANNER_ADO_BASE_URL: adoMock.url,
      ADO_PLANNER_VSSPS_URL: adoMock.url,
      ADO_PLANNER_E2E_DISPLAY_NAME: 'Ada Lovelace',
      ADO_PLANNER_E2E_USERNAME: 'ada@contoso.com'
    }
    delete env.ELECTRON_RUN_AS_NODE

    const electronApp = await electron.launch({
      args: ['.'],
      cwd: repoRoot,
      env
    })
    await use(electronApp)
    await electronApp.close()
  },
  window: async ({ electronApp }, use) => {
    const window = await electronApp.firstWindow()
    await use(window)
  }
})

export { expect }
