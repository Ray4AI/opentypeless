import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { CheckCircle2, CloudUpload, Download, Loader2, XCircle } from 'lucide-react'
import { useAppStore } from '../../stores/appStore'
import {
  clearCredential,
  readCredential,
  setCredential,
  webdavTestConnection,
  type WebDavTestResult,
} from '../../lib/tauri'
import { downloadAndRestore, uploadSyncPayload } from '../../lib/webdav-sync'
import { FormField } from './shared/FormField'
import { Toggle } from './shared/Toggle'
import { toast } from '../toast-service'

type BusyAction = 'test' | 'upload' | 'download' | null

export function SyncPane() {
  const { t } = useTranslation()
  const config = useAppStore((s) => s.config)
  const updateConfig = useAppStore((s) => s.updateConfig)

  const [password, setPassword] = useState('')
  const [passwordLoaded, setPasswordLoaded] = useState(false)
  const [busy, setBusy] = useState<BusyAction>(null)
  const [testResult, setTestResult] = useState<WebDavTestResult | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [passwordSaveError, setPasswordSaveError] = useState<string | null>(null)

  const configured = Boolean(config.webdav_url.trim())

  useEffect(() => {
    let cancelled = false
    readCredential('sync', 'webdav')
      .then((secret) => {
        if (!cancelled) {
          setPassword(secret ?? '')
          setPasswordLoaded(true)
        }
      })
      .catch((error) => {
        console.error('[sync] failed to read WebDAV credential', error)
        if (!cancelled) setPasswordLoaded(true)
      })
    return () => {
      cancelled = true
    }
  }, [])

  const persistPassword = (value: string, delayMs = 350) => {
    setPassword(value)
    setPasswordSaveError(null)
    if (!passwordLoaded) return
    window.setTimeout(() => {
      const operation = value.trim()
        ? setCredential('sync', 'webdav', value)
        : clearCredential('sync', 'webdav')
      operation.catch((error) => {
        const message = error instanceof Error ? error.message : String(error)
        setPasswordSaveError(message)
        console.error('[sync] failed to save WebDAV credential', error)
      })
    }, delayMs)
  }

  const requireTarget = () => {
    const url = config.webdav_url.trim()
    if (!url) {
      setError(t('sync.urlRequired'))
      return null
    }
    return url
  }

  const flushPassword = async () => {
    if (password.trim()) await setCredential('sync', 'webdav', password)
  }

  const handleTest = async () => {
    const url = requireTarget()
    if (!url) return
    setBusy('test')
    setError(null)
    setTestResult(null)
    try {
      await flushPassword()
      const result = await webdavTestConnection(url, config.webdav_username)
      setTestResult(result)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(null)
    }
  }

  const handleUpload = async () => {
    const url = requireTarget()
    if (!url) return
    setBusy('upload')
    setError(null)
    try {
      await flushPassword()
      await uploadSyncPayload(url, config.webdav_username)
      toast(t('sync.uploadOk'), 'success')
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      toast(t('sync.uploadFail'), 'error')
    } finally {
      setBusy(null)
    }
  }

  const handleDownload = async () => {
    const url = requireTarget()
    if (!url) return
    setBusy('download')
    setError(null)
    try {
      await flushPassword()
      await downloadAndRestore(url, config.webdav_username)
      toast(t('sync.downloadOk'), 'success')
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      toast(t('sync.downloadFail'), 'error')
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="space-y-5">
      <p className="text-[12px] text-text-secondary leading-relaxed">{t('sync.description')}</p>

      <FormField label={t('sync.url')}>
        <input
          type="text"
          value={config.webdav_url}
          onChange={(e) => updateConfig({ webdav_url: e.target.value })}
          placeholder="https://dav.example.com/dav/opentypeless-settings.json"
          className="w-full px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
        />
      </FormField>

      <FormField label={t('sync.username')}>
        <input
          type="text"
          value={config.webdav_username}
          onChange={(e) => updateConfig({ webdav_username: e.target.value })}
          placeholder={t('sync.usernamePlaceholder')}
          className="w-full px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
        />
      </FormField>

      <FormField label={t('sync.password')}>
        <input
          type="password"
          value={password}
          onChange={(e) => persistPassword(e.target.value)}
          onBlur={() => persistPassword(password, 0)}
          placeholder={t('sync.passwordPlaceholder')}
          className="w-full px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
        />
        <p className="text-[11px] text-text-tertiary mt-1.5">{t('sync.passwordStoredLocally')}</p>
        {passwordSaveError && (
          <p className="text-[11px] text-error mt-1.5">
            {t('sync.passwordSaveFailed', { details: passwordSaveError })}
          </p>
        )}
      </FormField>

      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={handleTest}
          disabled={!configured || busy !== null}
          className="px-4 py-2.5 bg-accent text-white rounded-[10px] text-[13px] border-none cursor-pointer hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed transition-colors flex items-center gap-1.5"
        >
          {busy === 'test' && <Loader2 size={14} className="animate-spin" />}
          {t('sync.test')}
        </button>
        <button
          type="button"
          onClick={handleUpload}
          disabled={!configured || busy !== null}
          className="px-4 py-2.5 rounded-[10px] text-[13px] border border-border bg-bg-secondary text-text-primary cursor-pointer hover:border-border-focus disabled:opacity-40 disabled:cursor-not-allowed transition-colors flex items-center gap-1.5"
        >
          {busy === 'upload' ? (
            <Loader2 size={14} className="animate-spin" />
          ) : (
            <CloudUpload size={14} />
          )}
          {t('sync.upload')}
        </button>
        <button
          type="button"
          onClick={handleDownload}
          disabled={!configured || busy !== null}
          className="px-4 py-2.5 rounded-[10px] text-[13px] border border-border bg-bg-secondary text-text-primary cursor-pointer hover:border-border-focus disabled:opacity-40 disabled:cursor-not-allowed transition-colors flex items-center gap-1.5"
        >
          {busy === 'download' ? (
            <Loader2 size={14} className="animate-spin" />
          ) : (
            <Download size={14} />
          )}
          {t('sync.download')}
        </button>
      </div>

      {testResult && (
        <p className="flex items-center gap-1 text-[12px] text-success">
          <CheckCircle2 size={13} /> {testResult.message}
        </p>
      )}
      {error && (
        <div className="flex items-start gap-1 text-[12px] text-error">
          <XCircle size={13} className="mt-[1px] flex-shrink-0" />
          <span>{error}</span>
        </div>
      )}

      <div className="pt-1">
        <Toggle
          checked={config.webdav_auto_sync}
          onChange={(checked) => updateConfig({ webdav_auto_sync: checked })}
          label={t('sync.autoSync')}
        />
        <p className="text-[11px] text-text-tertiary mt-1.5 ml-[68px]">{t('sync.autoSyncDesc')}</p>
      </div>
    </div>
  )
}
