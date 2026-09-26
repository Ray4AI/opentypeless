import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, Mic, MousePointer2, Globe2 } from 'lucide-react'

type PermissionState = 'ready' | 'later'
type PermissionRowModel = {
  id: string
  icon: React.ComponentType<{ size?: number; className?: string }>
  title: string
  desc: string
  state: PermissionState
}

export function PermissionsStep() {
  const { t } = useTranslation()

  const rows = useMemo(() => {
    const items: PermissionRowModel[] = [
      {
        id: 'microphone',
        icon: Mic,
        title: t('onboarding.permissions.microphone'),
        desc: t('onboarding.permissions.microphoneDesc'),
        state: 'later' as PermissionState,
      },
      {
        id: 'textOutput',
        icon: MousePointer2,
        title: t('onboarding.permissions.textOutput'),
        desc: t('onboarding.permissions.textOutputDesc'),
        state: 'ready' as PermissionState,
      },
      {
        id: 'browserApps',
        icon: Globe2,
        title: t('onboarding.permissions.browserApps'),
        desc: t('onboarding.permissions.browserAppsDesc'),
        state: 'ready' as PermissionState,
      },
    ]
    return items
  }, [t])

  return (
    <div className="space-y-3">
      <p className="text-[13px] leading-relaxed text-text-secondary">
        {t('onboarding.permissions.subtitle')}
      </p>
      <div className="space-y-2">
        {rows.map((row) => (
          <PermissionRow
            key={row.id}
            icon={row.icon}
            title={row.title}
            desc={row.desc}
            state={row.state}
          />
        ))}
      </div>
    </div>
  )
}

function PermissionRow({
  icon: Icon,
  title,
  desc,
  state,
}: {
  icon: React.ComponentType<{ size?: number; className?: string }>
  title: string
  desc: string
  state: PermissionState
}) {
  const { t } = useTranslation()
  const stateClass =
    state === 'ready' ? 'bg-green-500/10 text-green-600' : 'bg-bg-tertiary text-text-tertiary'
  const StateIcon = state === 'ready' ? Check : null

  return (
    <div className="flex items-center gap-3 rounded-[10px] bg-bg-secondary px-3 py-2.5">
      <div className="grid h-7 w-7 shrink-0 place-items-center rounded-[8px] bg-bg-tertiary text-text-tertiary">
        <Icon size={14} />
      </div>
      <div className="min-w-0 flex-1">
        <p className="truncate text-[13px] font-medium text-text-primary">{title}</p>
        <p className="truncate text-[11px] text-text-tertiary">{desc}</p>
      </div>
      <span
        className={`inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-1 text-[10px] font-medium ${stateClass}`}
      >
        {StateIcon && <StateIcon size={10} />}
        {t(`onboarding.permissions.status.${state}`)}
      </span>
    </div>
  )
}
