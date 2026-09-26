import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { PermissionsStep } from '../PermissionsStep'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) =>
      ({
        'onboarding.permissions.subtitle':
          'OpenTypeless asks for access only when a feature needs it.',
        'onboarding.permissions.microphone': 'Microphone',
        'onboarding.permissions.microphoneDesc': 'Capture your voice.',
        'onboarding.permissions.textOutput': 'Text output',
        'onboarding.permissions.textOutputDesc': 'Type into the app you are using.',
        'onboarding.permissions.browserApps': 'Browser apps',
        'onboarding.permissions.browserAppsDesc': 'Use Gmail, Docs, and Slack Web modes.',
        'onboarding.permissions.status.ready': 'Ready',
        'onboarding.permissions.status.later': 'Later',
      })[key] ?? key,
  }),
}))

afterEach(() => cleanup())

describe('PermissionsStep', () => {
  it('shows compact relevant permissions without a settings-style dashboard', () => {
    render(<PermissionsStep />)

    expect(screen.getByText('Microphone')).toBeInTheDocument()
    expect(screen.getByText('Text output')).toBeInTheDocument()
    expect(screen.getByText('Browser apps')).toBeInTheDocument()
  })

  it('marks text output and browser apps ready without permission prompts', () => {
    render(<PermissionsStep />)

    expect(screen.getAllByText('Ready').length).toBeGreaterThanOrEqual(2)
    expect(screen.queryByRole('button', { name: 'Fix' })).not.toBeInTheDocument()
  })
})
