import { BrowserWindow, dialog } from 'electron'
import type { UpdaterPrompt } from '@shared/types'

export type UpdaterBridge = {
  prompt: UpdaterPrompt | null
  apply(): Promise<void>
  snooze(): void
}

export function createUpdaterBridge(isPackaged: boolean): UpdaterBridge {
  let prompt: UpdaterPrompt | null = null
  let snoozed = false

  if (!isPackaged || process.env.ADO_PLANNER_E2E === '1') {
    return {
      get prompt() {
        return null
      },
      async apply() {
        return
      },
      snooze() {
        return
      }
    }
  }

  void import('electron-updater')
    .then(({ autoUpdater }) => {
      autoUpdater.autoDownload = false
      autoUpdater.on('update-available', (info) => {
        if (snoozed) {
          return
        }
        prompt = {
          version: info.version,
          releaseNotes: typeof info.releaseNotes === 'string' ? info.releaseNotes : null
        }
        for (const win of BrowserWindow.getAllWindows()) {
          win.webContents.send('updater:available', prompt)
        }
      })
      autoUpdater.on('error', (error) => {
        for (const win of BrowserWindow.getAllWindows()) {
          win.webContents.send('updater:error', error.message)
        }
      })
      autoUpdater.checkForUpdates().catch(() => undefined)
    })
    .catch(() => undefined)

  return {
    get prompt() {
      return prompt
    },
    async apply() {
      const { autoUpdater } = await import('electron-updater')
      await autoUpdater.downloadUpdate()
      autoUpdater.quitAndInstall()
    },
    snooze() {
      snoozed = true
      prompt = null
    }
  }
}

export async function confirmUpdate(prompt: UpdaterPrompt): Promise<boolean> {
  const result = await dialog.showMessageBox({
    type: 'question',
    buttons: ['Yes', 'No'],
    defaultId: 0,
    cancelId: 1,
    title: 'Update available',
    message: `Version ${prompt.version} is available. Download, install, and restart?`,
    detail: prompt.releaseNotes ?? undefined
  })
  return result.response === 0
}
