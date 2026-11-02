import { describe, expect, it, vi } from 'vitest'
import { durationSeconds, formatAgo, formatDateTime, toMillis } from '@/utils/time'
import { errorMessage, fieldErrors, isAbortError } from '@/utils/errors'
import { downloadJson, readJsonFile, triggerDownload } from '@/utils/download'
import { ApiError } from '@/api/client'

describe('time utils', () => {
  it('normalises timestamps', () => {
    expect(toMillis(1_700_000_000)).toBe(1_700_000_000_000)
    expect(toMillis(1_700_000_000_000)).toBe(1_700_000_000_000)
    expect(toMillis('1700000000')).toBe(1_700_000_000_000)
    expect(toMillis('2024-01-01T00:00:00Z')).toBe(Date.UTC(2024, 0, 1))
  })
  it('parses durations', () => {
    expect(durationSeconds(60)).toBe(60)
    expect(durationSeconds('60s')).toBe(60)
    expect(durationSeconds('1m')).toBe(60)
    expect(durationSeconds('1h')).toBe(3600)
    expect(durationSeconds('30')).toBe(30)
    expect(durationSeconds('500ms')).toBe(0.5)
    expect(durationSeconds('bogus')).toBeNaN()
  })
  it('formats dates and ages', () => {
    expect(formatDateTime(null)).toBe('—')
    expect(formatDateTime('garbage')).toBe('garbage')
    expect(formatDateTime(Date.UTC(2024, 0, 1))).toMatch(/^2024-01-01|^2023-12-31/)
    const now = Date.UTC(2024, 0, 1, 12)
    expect(formatAgo(now - 5000, now)).toBe('5s ago')
    expect(formatAgo(now - 5 * 60_000, now)).toBe('5m ago')
    expect(formatAgo(now - 3 * 3600_000, now)).toBe('3h ago')
    expect(formatAgo(now - 2 * 86_400_000, now)).toBe('2d ago')
    expect(formatAgo(null)).toBe('never')
    expect(formatAgo('x')).toBe('x')
  })
})

describe('error utils', () => {
  it('produces friendly messages', () => {
    expect(errorMessage(new ApiError(404, { code: 'not_found', message: 'raw' }))).toBe(
      'The requested item no longer exists.',
    )
    expect(
      errorMessage(new ApiError(400, { code: 'validation_failed', message: 'name: too long' }), 'Save'),
    ).toBe('Save: name: too long')
    expect(errorMessage(new ApiError(418, { code: 'teapot', message: 'short and stout' }))).toBe(
      'short and stout',
    )
    expect(errorMessage(new Error('boom'))).toBe('boom')
    expect(errorMessage('weird')).toBe('Unexpected error')
  })
  it('extracts field errors and abort errors', () => {
    expect(
      fieldErrors(new ApiError(400, { code: 'validation_failed', message: '', fields: { a: 'b' } })),
    ).toEqual({ a: 'b' })
    expect(fieldErrors(new Error('x'))).toEqual({})
    expect(isAbortError(new DOMException('x', 'AbortError'))).toBe(true)
    expect(isAbortError(new Error('x'))).toBe(false)
  })
})

describe('download utils', () => {
  it('triggers anchor downloads', () => {
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {})
    triggerDownload('/api/v1/flows/export.csv', 'flows.csv')
    expect(click).toHaveBeenCalled()
    URL.createObjectURL = vi.fn(() => 'blob:x')
    URL.revokeObjectURL = vi.fn()
    downloadJson({ a: 1 }, 'x.json')
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:x')
    click.mockRestore()
  })
  it('reads JSON files', async () => {
    await expect(readJsonFile(new Blob(['{"a":1}']))).resolves.toEqual({ a: 1 })
    await expect(readJsonFile(new Blob(['nope']))).rejects.toThrow('not valid JSON')
  })
})
