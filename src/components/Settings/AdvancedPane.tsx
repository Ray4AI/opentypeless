import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Code2, Mic, RotateCcw, Save, Sparkles, Wand2 } from 'lucide-react'
import { useAppStore, type AppConfig } from '../../stores/appStore'
import { askAnything, getAskPromptDefaults, getConfig, updateConfig } from '../../lib/tauri'
import { FormField } from './shared/FormField'
import { toast } from '../toast-service'

/** Advanced field names, used to reset the pane back to built-in defaults. */
type AdvancedField =
  | 'ask_max_tokens'
  | 'ask_temperature'
  | 'ask_system_prompt'
  | 'ask_request_timeout_secs'
  | 'polish_max_tokens'
  | 'polish_temperature'
  | 'polish_system_prompt_append'
  | 'llm_request_timeout_secs'
  | 'ask_request_extra_params'
  | 'polish_request_extra_params'

// `Pick` (not `Record<K, AppConfig[K]>`) keeps each field's own type instead of
// collapsing them into a single `string | number` union.
const ADVANCED_DEFAULTS: Pick<AppConfig, AdvancedField> = {
  ask_max_tokens: 4096,
  ask_temperature: 0.2,
  ask_system_prompt: '',
  ask_request_timeout_secs: 120,
  polish_max_tokens: 4096,
  polish_temperature: 0.3,
  polish_system_prompt_append: '',
  llm_request_timeout_secs: 120,
  ask_request_extra_params: '',
  polish_request_extra_params: '',
}

type JsonValidation = { ok: boolean; message?: string }

function validateJsonObject(raw: string): JsonValidation {
  const trimmed = raw.trim()
  if (!trimmed) return { ok: true }
  let parsed: unknown
  try {
    parsed = JSON.parse(trimmed)
  } catch (error) {
    return {
      ok: false,
      message: error instanceof Error ? error.message : 'Invalid JSON',
    }
  }
  if (Array.isArray(parsed) || parsed === null || typeof parsed !== 'object') {
    return { ok: false, message: 'Must be a JSON object, e.g. { "key": "value" }' }
  }
  const keys = Object.keys(parsed as Record<string, unknown>)
  const blocked = keys.filter((key) => key === 'model' || key === 'messages')
  if (blocked.length > 0) {
    return {
      ok: false,
      message: `Ignored protected keys: ${blocked.join(', ')} (model/messages cannot be overridden)`,
    }
  }
  return { ok: true }
}

function NumberField({
  label,
  hint,
  value,
  min,
  max,
  step = 1,
  fallback,
  onChange,
}: {
  label: string
  hint: string
  value: number
  min: number
  max: number
  step?: number
  fallback: number
  onChange: (value: number) => void
}) {
  const invalid = !Number.isFinite(value) || value < min || value > max
  return (
    <FormField label={label}>
      <input
        type="number"
        value={Number.isFinite(value) ? value : ''}
        min={min}
        max={max}
        step={step}
        onChange={(event) => {
          const next = Number(event.target.value)
          onChange(Number.isFinite(next) ? next : fallback)
        }}
        className={`w-full px-3 py-2.5 bg-bg-secondary border rounded-[10px] text-[13px] text-text-primary outline-none transition-colors ${
          invalid ? 'border-error' : 'border-border focus:border-border-focus'
        }`}
      />
      <p className="text-[11px] text-text-tertiary mt-1.5">{hint}</p>
    </FormField>
  )
}

function JsonField({
  label,
  placeholder,
  value,
  onChange,
}: {
  label: string
  placeholder: string
  value: string
  onChange: (value: string) => void
}) {
  const validation = useMemo(() => validateJsonObject(value), [value])
  return (
    <FormField label={label}>
      <textarea
        value={value}
        onChange={(event) => onChange(event.target.value)}
        rows={5}
        spellCheck={false}
        placeholder={placeholder}
        className={`w-full resize-y px-3 py-2.5 bg-bg-secondary border rounded-[10px] text-[12px] font-mono text-text-primary outline-none transition-colors ${
          validation.ok ? 'border-border focus:border-border-focus' : 'border-error'
        }`}
      />
      <p className={`text-[11px] mt-1.5 ${validation.ok ? 'text-text-tertiary' : 'text-error'}`}>
        {validation.ok
          ? 'Top-level merge into the request body. Applied after built-in provider heuristics.'
          : validation.message}
      </p>
    </FormField>
  )
}

export function AdvancedPane() {
  const { t } = useTranslation()
  const config = useAppStore((s) => s.config)
  const updateConfigInStore = useAppStore((s) => s.updateConfig)
  const setSavedConfig = useAppStore((s) => s.setSavedConfig)
  const [builtInAskPrompt, setBuiltInAskPrompt] = useState('')
  const [saving, setSaving] = useState(false)
  const [testing, setTesting] = useState(false)
  const [testResult, setTestResult] = useState<{ ok: boolean; text: string } | null>(null)

  useEffect(() => {
    getAskPromptDefaults()
      .then((defaults) => setBuiltInAskPrompt(defaults.plain))
      .catch(() => setBuiltInAskPrompt(''))
  }, [])

  const jsonIssues = [
    validateJsonObject(config.ask_request_extra_params),
    validateJsonObject(config.polish_request_extra_params),
  ].filter((result) => !result.ok)

  const handleSave = async () => {
    if (jsonIssues.length > 0) {
      toast(t('settings.advancedFixJson'), 'error')
      return
    }
    setSaving(true)
    try {
      await updateConfig(config)
      const backendConfig = await getConfig()
      setSavedConfig(backendConfig)
    } catch (error) {
      toast(error instanceof Error ? error.message : String(error), 'error')
    } finally {
      setSaving(false)
    }
  }

  const handleReset = () => {
    updateConfigInStore({ ...ADVANCED_DEFAULTS })
    toast(t('settings.advancedResetHint'), 'info')
  }

  const handleTestAsk = async () => {
    setTesting(true)
    setTestResult(null)
    try {
      const answer = await askAnything('Reply with exactly: Ask path OK')
      setTestResult({ ok: true, text: answer })
    } catch (error) {
      setTestResult({
        ok: false,
        text: error instanceof Error ? error.message : String(error),
      })
    } finally {
      setTesting(false)
    }
  }

  return (
    <div className="space-y-7 max-w-[720px]">
      <p className="text-[12px] text-text-tertiary leading-relaxed">
        {t('settings.advancedIntro')}
      </p>

      {/* ── Ask Anything ─────────────────────────────────────────── */}
      <section className="space-y-5">
        <h3 className="flex items-center gap-2 text-[13px] font-semibold text-text-primary">
          <Sparkles size={15} /> {t('settings.advancedAskSection')}
        </h3>

        <div className="grid grid-cols-2 gap-4">
          <NumberField
            label={t('settings.advancedAskMaxTokens')}
            hint={t('settings.advancedAskMaxTokensHint')}
            value={config.ask_max_tokens}
            min={16}
            max={128000}
            step={128}
            fallback={ADVANCED_DEFAULTS.ask_max_tokens}
            onChange={(value) => updateConfigInStore({ ask_max_tokens: value })}
          />
          <NumberField
            label={t('settings.advancedAskTemperature')}
            hint={t('settings.advancedTemperatureHint')}
            value={config.ask_temperature}
            min={0}
            max={2}
            step={0.05}
            fallback={ADVANCED_DEFAULTS.ask_temperature}
            onChange={(value) => updateConfigInStore({ ask_temperature: value })}
          />
        </div>

        <NumberField
          label={t('settings.advancedAskTimeout')}
          hint={t('settings.advancedAskTimeoutHint')}
          value={config.ask_request_timeout_secs}
          min={5}
          max={600}
          fallback={ADVANCED_DEFAULTS.ask_request_timeout_secs}
          onChange={(value) => updateConfigInStore({ ask_request_timeout_secs: value })}
        />

        <FormField label={t('settings.advancedAskPrompt')}>
          <textarea
            value={config.ask_system_prompt}
            onChange={(event) => updateConfigInStore({ ask_system_prompt: event.target.value })}
            rows={4}
            placeholder={builtInAskPrompt || t('settings.advancedPromptPlaceholder')}
            className="w-full resize-y px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[12px] font-mono text-text-primary outline-none focus:border-border-focus transition-colors"
          />
          <p className="text-[11px] text-text-tertiary mt-1.5">
            {t('settings.advancedAskPromptHint')}
          </p>
          {builtInAskPrompt && (
            <details className="mt-2">
              <summary className="text-[11px] text-text-tertiary cursor-pointer">
                {t('settings.advancedBuiltInPrompt')}
              </summary>
              <pre className="mt-1.5 p-2.5 bg-bg-tertiary rounded-[8px] text-[11px] text-text-secondary whitespace-pre-wrap break-words max-h-[160px] overflow-y-auto">
                {builtInAskPrompt}
              </pre>
            </details>
          )}
        </FormField>

        <JsonField
          label={t('settings.advancedAskExtra')}
          placeholder={'{\n  "reasoning": { "effort": "none" }\n}'}
          value={config.ask_request_extra_params}
          onChange={(value) => updateConfigInStore({ ask_request_extra_params: value })}
        />
      </section>

      {/* ── Polish / Translate ───────────────────────────────────── */}
      <section className="space-y-5 pt-2 border-t border-border">
        <h3 className="flex items-center gap-2 text-[13px] font-semibold text-text-primary">
          <Wand2 size={15} /> {t('settings.advancedPolishSection')}
        </h3>

        <div className="grid grid-cols-2 gap-4">
          <NumberField
            label={t('settings.advancedPolishMaxTokens')}
            hint={t('settings.advancedPolishMaxTokensHint')}
            value={config.polish_max_tokens}
            min={16}
            max={128000}
            step={128}
            fallback={ADVANCED_DEFAULTS.polish_max_tokens}
            onChange={(value) => updateConfigInStore({ polish_max_tokens: value })}
          />
          <NumberField
            label={t('settings.advancedPolishTemperature')}
            hint={t('settings.advancedTemperatureHint')}
            value={config.polish_temperature}
            min={0}
            max={2}
            step={0.05}
            fallback={ADVANCED_DEFAULTS.polish_temperature}
            onChange={(value) => updateConfigInStore({ polish_temperature: value })}
          />
        </div>

        <NumberField
          label={t('settings.advancedLlmTimeout')}
          hint={t('settings.advancedLlmTimeoutHint')}
          value={config.llm_request_timeout_secs}
          min={5}
          max={600}
          fallback={ADVANCED_DEFAULTS.llm_request_timeout_secs}
          onChange={(value) => updateConfigInStore({ llm_request_timeout_secs: value })}
        />

        <FormField label={t('settings.advancedPolishPromptAppend')}>
          <textarea
            value={config.polish_system_prompt_append}
            onChange={(event) =>
              updateConfigInStore({ polish_system_prompt_append: event.target.value })
            }
            rows={3}
            placeholder={t('settings.advancedPromptPlaceholder')}
            className="w-full resize-y px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[12px] font-mono text-text-primary outline-none focus:border-border-focus transition-colors"
          />
          <p className="text-[11px] text-text-tertiary mt-1.5">
            {t('settings.advancedPolishPromptAppendHint')}
          </p>
        </FormField>

        <JsonField
          label={t('settings.advancedPolishExtra')}
          placeholder={'{\n  "top_p": 0.9\n}'}
          value={config.polish_request_extra_params}
          onChange={(value) => updateConfigInStore({ polish_request_extra_params: value })}
        />
      </section>

      {/* ── Notes ────────────────────────────────────────────────── */}
      <section className="space-y-2 pt-2 border-t border-border">
        <h3 className="flex items-center gap-2 text-[13px] font-semibold text-text-primary">
          <Code2 size={15} /> {t('settings.advancedNotesTitle')}
        </h3>
        <ul className="text-[12px] text-text-tertiary leading-relaxed list-disc pl-5 space-y-1">
          <li>{t('settings.advancedNoteStt')}</li>
          <li>{t('settings.advancedNoteAnthropic')}</li>
          <li>{t('settings.advancedNoteSharedModel')}</li>
        </ul>
      </section>

      {/* ── Actions ──────────────────────────────────────────────── */}
      <div className="flex items-center gap-2 pt-2">
        <button
          onClick={handleSave}
          disabled={saving || jsonIssues.length > 0}
          className="px-4 py-2.5 bg-accent text-white rounded-[10px] text-[13px] border-none cursor-pointer hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed transition-colors flex items-center gap-1.5"
        >
          <Save size={14} />
          {saving ? t('common.saving') : t('common.save')}
        </button>
        <button
          onClick={handleReset}
          className="px-4 py-2.5 bg-bg-secondary text-text-primary rounded-[10px] text-[13px] border border-border cursor-pointer hover:bg-bg-tertiary transition-colors flex items-center gap-1.5"
        >
          <RotateCcw size={14} />
          {t('settings.advancedReset')}
        </button>
        <button
          onClick={handleTestAsk}
          disabled={testing || saving || jsonIssues.length > 0}
          className="px-4 py-2.5 bg-bg-secondary text-text-primary rounded-[10px] text-[13px] border border-border cursor-pointer hover:bg-bg-tertiary disabled:opacity-40 disabled:cursor-not-allowed transition-colors flex items-center gap-1.5"
        >
          <Mic size={14} />
          {testing ? t('settings.testing') : t('settings.advancedTestAsk')}
        </button>
      </div>

      {testResult && (
        <div
          className={`p-3 rounded-[10px] text-[12px] whitespace-pre-wrap break-words border ${
            testResult.ok
              ? 'bg-success/10 text-success border-success/30'
              : 'bg-error/10 text-error border-error/30'
          }`}
        >
          {testResult.text}
        </div>
      )}
    </div>
  )
}
