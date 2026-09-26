import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { TranslationConfig } from '../../../stores/appStore'
import { TranslationTargets } from '../TranslationTargets'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

afterEach(cleanup)

function renderTargets(
  value: TranslationConfig = {
    targets: ['en', 'zh'],
    active_target: 'en',
  },
) {
  const onChange = vi.fn()
  render(<TranslationTargets value={value} onChange={onChange} />)
  return onChange
}

describe('TranslationTargets', () => {
  it('keeps the common single-language state compact', () => {
    renderTargets({ targets: ['en'], active_target: 'en' })

    expect(screen.getByRole('combobox', { name: 'settings.targetLanguage' })).toHaveValue('en')
    expect(screen.queryByRole('radio')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /moveTranslationTarget/ })).not.toBeInTheDocument()
    expect(
      screen.queryByRole('button', { name: /removeTranslationTarget/ }),
    ).not.toBeInTheDocument()
    expect(
      screen.queryByRole('button', { name: 'settings.manageTranslationTargets' }),
    ).not.toBeInTheDocument()
  })

  it('keeps language choices unique and adds the first available target', () => {
    const onChange = renderTargets({ targets: ['en'], active_target: 'en' })

    fireEvent.click(screen.getByRole('button', { name: 'settings.addTranslationTarget' }))

    expect(onChange).toHaveBeenCalledWith({
      targets: ['en', 'zh'],
      active_target: 'en',
    })
  })

  it('does not offer languages that are already selected', () => {
    const onChange = renderTargets({ targets: ['en', 'zh'], active_target: 'en' })

    fireEvent.click(screen.getByRole('button', { name: 'settings.manageTranslationTargets' }))
    const chineseRow = screen.getByTestId('translation-target-zh')
    expect(within(chineseRow).queryByRole('option', { name: 'English' })).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'settings.addTranslationTarget' }))
    expect(onChange).not.toHaveBeenCalled()
  })

  it('reorders targets without changing the active target', () => {
    const onChange = renderTargets({
      targets: ['en', 'zh'],
      active_target: 'zh',
    })

    fireEvent.click(screen.getByRole('button', { name: 'settings.manageTranslationTargets' }))
    fireEvent.click(screen.getByRole('button', { name: 'settings.moveTranslationTargetUp zh' }))

    expect(onChange).toHaveBeenCalledWith({
      targets: ['zh', 'en'],
      active_target: 'zh',
    })
  })

  it('selects the nearest remaining target when removing the active target', () => {
    const onChange = renderTargets({
      targets: ['en', 'zh'],
      active_target: 'zh',
    })

    fireEvent.click(screen.getByRole('button', { name: 'settings.manageTranslationTargets' }))
    fireEvent.click(screen.getByRole('button', { name: 'settings.removeTranslationTarget zh' }))

    expect(onChange).toHaveBeenCalledWith({
      targets: ['en'],
      active_target: 'en',
    })
  })

  it('changes the active target from the compact selector', () => {
    const onChange = renderTargets({ targets: ['en', 'zh'], active_target: 'en' })

    fireEvent.change(screen.getByRole('combobox', { name: 'settings.targetLanguage' }), {
      target: { value: 'zh' },
    })

    expect(onChange).toHaveBeenCalledWith({ targets: ['en', 'zh'], active_target: 'zh' })
  })
})
