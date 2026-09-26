import { describe, expect, it } from 'vitest'
import en from '../locales/en.json'
import zh from '../locales/zh.json'

const locales = { en, zh }

function leafKeys(value: unknown, prefix = ''): string[] {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return prefix ? [prefix] : []
  }

  return Object.entries(value as Record<string, unknown>).flatMap(([key, child]) =>
    leafKeys(child, prefix ? `${prefix}.${key}` : key),
  )
}

describe('locale message coverage', () => {
  it('keeps every locale aligned with English leaf keys', () => {
    const expectedKeys = leafKeys(en).sort()

    for (const [locale, messages] of Object.entries(locales)) {
      expect(leafKeys(messages).sort(), locale).toEqual(expectedKeys)
    }
  })
})
