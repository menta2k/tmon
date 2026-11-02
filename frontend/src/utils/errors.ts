import { ApiError } from '@/api/client'

const FRIENDLY: Readonly<Record<string, string>> = {
  unauthorized: 'Your session has expired. Please log in again.',
  not_found: 'The requested item no longer exists.',
  conflict: 'This change conflicts with the current state.',
  rate_limited: 'Too many requests. Please wait a moment and try again.',
  internal: 'The server hit an internal error. Check the tmon logs.',
  network_error: 'Cannot reach the tmon server. Is it running?',
}

/** Converts any thrown value into a message suitable for a toast. */
export function errorMessage(err: unknown, context?: string): string {
  const prefix = context ? `${context}: ` : ''
  if (err instanceof ApiError) {
    if (err.code === 'validation_failed' || err.code === 'unsupported_filter_range')
      return prefix + err.message
    return prefix + (FRIENDLY[err.code] ?? err.message)
  }
  if (err instanceof Error) return prefix + err.message
  return `${prefix}Unexpected error`
}

/** Field errors from a `validation_failed` response, or an empty object. */
export function fieldErrors(err: unknown): Readonly<Record<string, string>> {
  return err instanceof ApiError ? err.fields : {}
}

export function isAbortError(err: unknown): boolean {
  return err instanceof DOMException && err.name === 'AbortError'
}
