import { useAppStore } from '../stores/appStore'
import {
  getConfig,
  getCorrectionRules,
  getDictionary,
  getHistory,
  restoreBackupData,
  setAutoStart,
  updateConfig,
  webdavDownloadBackup,
  webdavUploadBackup,
} from './tauri'
import { createBackupSettings, mergeBackupSettings } from './backup-settings'

export interface SyncPayload {
  format: 'opentypeless-backup'
  version: number
  exportedAt: string
  settings: ReturnType<typeof createBackupSettings>
  history: unknown
  dictionary: { entries: unknown; correction_rules: unknown }
}

export function buildSyncPayload(): SyncPayload {
  const state = useAppStore.getState()
  return {
    format: 'opentypeless-backup',
    version: 1,
    exportedAt: new Date().toISOString(),
    settings: createBackupSettings(state.config),
    history: state.history,
    dictionary: {
      entries: state.dictionary,
      correction_rules: state.correctionRules,
    },
  }
}

/** Upload the current settings/dictionary/history to the WebDAV target. */
export async function uploadSyncPayload(url: string, username: string): Promise<void> {
  await webdavUploadBackup(url, username, JSON.stringify(buildSyncPayload()))
}

/**
 * Download the remote backup and apply it locally. Returns a short status so
 * callers can show their own toast wording.
 */
export async function downloadAndRestore(
  url: string,
  username: string,
): Promise<'restored' | 'restored-data-only'> {
  const raw = await webdavDownloadBackup(url, username)
  const data = JSON.parse(raw) as {
    settings?: unknown
    history?: unknown
    dictionary?: unknown
  }

  const store = useAppStore.getState()
  let autoStartApplied = false
  if (data.settings) {
    const current = store.config
    const restoredConfig = mergeBackupSettings(current, data.settings)
    if (restoredConfig.auto_start !== current.auto_start) {
      await setAutoStart(restoredConfig.auto_start)
      autoStartApplied = true
    }
    try {
      await updateConfig(restoredConfig)
    } catch (error) {
      if (autoStartApplied) await setAutoStart(current.auto_start).catch(() => {})
      throw error
    }
    const persistedConfig = await getConfig().catch(() => restoredConfig)
    store.setConfig(persistedConfig)
    store.setSavedConfig(persistedConfig)
  }

  const hasData = data.history != null || data.dictionary != null
  if (hasData) {
    const restored = await restoreBackupData(data.history ?? null, data.dictionary ?? null)
    store.setHistory(restored.history)
    store.setDictionary(restored.dictionary)
    store.setCorrectionRules(restored.correctionRules)
    return 'restored'
  }

  store.setHistory(await getHistory(200, 0).catch(() => []))
  store.setDictionary(await getDictionary().catch(() => []))
  store.setCorrectionRules(await getCorrectionRules().catch(() => []))
  return 'restored-data-only'
}

/**
 * Fire-and-forget auto-sync used after a settings save. Failures are logged,
 * never surfaced — manual upload/download in the Sync pane remains the source
 * of truth when debugging a server problem.
 */
export function autoSyncIfEnabled(): void {
  const { config } = useAppStore.getState()
  if (!config.webdav_auto_sync) return
  const url = config.webdav_url.trim()
  if (!url) return
  void uploadSyncPayload(url, config.webdav_username).catch((error) => {
    console.warn('[sync] auto upload failed:', error)
  })
}
