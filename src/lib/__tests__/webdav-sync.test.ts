import { beforeEach, describe, expect, it, vi } from 'vitest'

const uploadMock = vi.hoisted(() => vi.fn())
const downloadMock = vi.hoisted(() => vi.fn())
const updateConfigMock = vi.hoisted(() => vi.fn())
const restoreBackupMock = vi.hoisted(() => vi.fn())
const setAutoStartMock = vi.hoisted(() => vi.fn())

vi.mock('../tauri', () => ({
  webdavUploadBackup: uploadMock,
  webdavDownloadBackup: downloadMock,
  updateConfig: updateConfigMock,
  restoreBackupData: restoreBackupMock,
  setAutoStart: setAutoStartMock,
  getConfig: vi.fn(),
  getHistory: vi.fn().mockResolvedValue([]),
  getDictionary: vi.fn().mockResolvedValue([]),
  getCorrectionRules: vi.fn().mockResolvedValue([]),
}))

import { getConfig } from '../tauri'
import { useAppStore } from '../../stores/appStore'
import {
  autoSyncIfEnabled,
  buildSyncPayload,
  downloadAndRestore,
  uploadSyncPayload,
} from '../webdav-sync'

describe('webdav sync', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useAppStore.setState(useAppStore.getInitialState())
    // The real backend always echoes the persisted config back.
    vi.mocked(getConfig).mockImplementation(async () => useAppStore.getState().config)
  })

  it('builds a versioned payload with settings, history, and dictionary', () => {
    const payload = buildSyncPayload()

    expect(payload.format).toBe('opentypeless-backup')
    expect(payload.version).toBe(1)
    expect(payload.settings).toHaveProperty('stt_provider')
    expect(payload.dictionary).toHaveProperty('entries')
    expect(payload.dictionary).toHaveProperty('correction_rules')
  })

  it('uploads the serialized payload to the configured target', async () => {
    await uploadSyncPayload('https://dav.example.com/a.json', 'alice')

    expect(uploadMock).toHaveBeenCalledTimes(1)
    const [url, username, body] = uploadMock.mock.calls[0]
    expect(url).toBe('https://dav.example.com/a.json')
    expect(username).toBe('alice')
    expect(JSON.parse(body)).toMatchObject({ format: 'opentypeless-backup' })
  })

  it('applies a downloaded settings payload through the config merger', async () => {
    downloadMock.mockResolvedValue(
      JSON.stringify({
        format: 'opentypeless-backup',
        version: 1,
        settings: { stt_provider: 'deepgram' },
        history: null,
        dictionary: null,
      }),
    )

    await downloadAndRestore('https://dav.example.com/a.json', 'alice')

    expect(updateConfigMock).toHaveBeenCalledTimes(1)
    expect(updateConfigMock.mock.calls[0][0]).toMatchObject({ stt_provider: 'deepgram' })
  })

  it('skips auto sync when disabled or unconfigured', () => {
    uploadMock.mockClear()
    useAppStore.setState((state) => ({
      config: { ...state.config, webdav_auto_sync: false, webdav_url: '' },
    }))

    autoSyncIfEnabled()
    expect(uploadMock).not.toHaveBeenCalled()

    useAppStore.setState((state) => ({
      config: {
        ...state.config,
        webdav_auto_sync: true,
        webdav_url: 'https://dav.example.com/a.json',
        webdav_username: 'alice',
      },
    }))
    autoSyncIfEnabled()
    expect(uploadMock).toHaveBeenCalledTimes(1)
  })

  it('auto sync failures never throw into the save flow', async () => {
    uploadMock.mockRejectedValueOnce(new Error('network down'))
    useAppStore.setState((state) => ({
      config: {
        ...state.config,
        webdav_auto_sync: true,
        webdav_url: 'https://dav.example.com/a.json',
      },
    }))

    expect(() => autoSyncIfEnabled()).not.toThrow()
    await new Promise((resolve) => setTimeout(resolve, 0))
  })
})
