import { API_BASE } from '@/constants'
import type { ApiErrorBody, ApiMeta, ApiResult, Envelope } from './types'
import { buildQuery, type QueryParams } from './query'

/** Error thrown for every failed API call; `code` mirrors the contract's error codes. */
export class ApiError extends Error {
  readonly code: string
  readonly status: number
  readonly fields: Readonly<Record<string, string>>

  constructor(status: number, body: ApiErrorBody) {
    super(body.message)
    this.name = 'ApiError'
    this.code = body.code
    this.status = status
    this.fields = body.fields ?? {}
  }

  get isUnauthorized(): boolean {
    return this.status === 401 || this.code === 'unauthorized'
  }
}

type UnauthorizedHandler = () => void

const defaultUnauthorizedHandler: UnauthorizedHandler = () => {
  if (typeof window !== 'undefined' && window.location.pathname !== '/login') {
    window.location.assign('/login')
  }
}

let unauthorizedHandler: UnauthorizedHandler = defaultUnauthorizedHandler

/** Lets the router replace the hard redirect with an SPA navigation. */
export function setUnauthorizedHandler(handler: UnauthorizedHandler | null): void {
  unauthorizedHandler = handler ?? defaultUnauthorizedHandler
}

export interface RequestOptions {
  query?: QueryParams
  body?: unknown
  /** Do not trigger the global 401 redirect (used by the login form). */
  skipAuthRedirect?: boolean
  signal?: AbortSignal
}

export type HttpMethod = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'

export function apiUrl(path: string, query?: QueryParams): string {
  const qs = query ? buildQuery(query) : ''
  return `${API_BASE}${path}${qs ? `?${qs}` : ''}`
}

function isEnvelope(value: unknown): value is Envelope<unknown> {
  return typeof value === 'object' && value !== null && 'ok' in value
}

async function parseBody(response: Response): Promise<unknown> {
  const text = await response.text()
  if (!text) return null
  try {
    return JSON.parse(text) as unknown
  } catch {
    return { ok: false, data: null, error: { code: 'invalid_response', message: text.slice(0, 200) } }
  }
}

function fallbackError(status: number): ApiErrorBody {
  return { code: status === 401 ? 'unauthorized' : 'http_error', message: `Request failed (HTTP ${status})` }
}

function toResult<T>(response: Response, payload: unknown, opts: RequestOptions): ApiResult<T> {
  const envelope = isEnvelope(payload) ? payload : null
  if (response.ok && (envelope === null || envelope.ok)) {
    const data = (envelope ? envelope.data : payload) as T
    const meta: ApiMeta = envelope?.meta ?? {}
    return { data, meta }
  }
  const error = new ApiError(response.status, envelope?.error ?? fallbackError(response.status))
  if (error.isUnauthorized && !opts.skipAuthRedirect) unauthorizedHandler()
  throw error
}

/** Performs a request and returns the unwrapped `data` together with `meta`. */
export async function request<T>(
  method: HttpMethod,
  path: string,
  opts: RequestOptions = {},
): Promise<ApiResult<T>> {
  const headers: Record<string, string> = { Accept: 'application/json' }
  const init: RequestInit = { method, headers, credentials: 'same-origin', signal: opts.signal }
  if (opts.body !== undefined) {
    headers['Content-Type'] = 'application/json'
    init.body = JSON.stringify(opts.body)
  }
  let response: Response
  try {
    response = await fetch(apiUrl(path, opts.query), init)
  } catch (cause) {
    if (cause instanceof DOMException && cause.name === 'AbortError') throw cause
    throw new ApiError(0, { code: 'network_error', message: 'Cannot reach the tmon server' })
  }
  const payload = await parseBody(response)
  return toResult<T>(response, payload, opts)
}

export async function get<T>(path: string, query?: QueryParams, signal?: AbortSignal): Promise<ApiResult<T>> {
  return request<T>('GET', path, { query, signal })
}

export async function getData<T>(path: string, query?: QueryParams): Promise<T> {
  return (await request<T>('GET', path, { query })).data
}

export async function send<T>(method: Exclude<HttpMethod, 'GET'>, path: string, body?: unknown): Promise<T> {
  return (await request<T>(method, path, { body })).data
}
