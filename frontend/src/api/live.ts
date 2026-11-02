import { getCurrentScope, onScopeDispose, ref, shallowRef, type Ref, type ShallowRef } from 'vue'
import { LIVE_BACKOFF_BASE_MS, LIVE_BACKOFF_MAX_MS, LIVE_HISTORY_SIZE, LIVE_PATH } from '@/constants'
import type { LiveTick, Warning } from './types'

export type LiveState = 'idle' | 'connecting' | 'open' | 'reconnecting'

export interface LiveHandlers {
  onTick: (tick: LiveTick) => void
  onWarning: (warning: Warning) => void
  onState: (state: LiveState) => void
}

export interface LiveConnection {
  start: () => void
  stop: () => void
}

export interface LiveOptions {
  url?: string
  baseDelayMs?: number
  maxDelayMs?: number
}

/** Exponential backoff: base · 2^attempt, capped. */
export function backoffDelay(
  attempt: number,
  base = LIVE_BACKOFF_BASE_MS,
  max = LIVE_BACKOFF_MAX_MS,
): number {
  return Math.min(max, base * 2 ** Math.max(0, attempt))
}

function parseJson<T>(raw: string): T | null {
  try {
    return JSON.parse(raw) as T
  } catch {
    return null
  }
}

function isTick(v: unknown): v is LiveTick {
  return typeof v === 'object' && v !== null && 'bytes_per_sec' in v && 'ts' in v
}

function isWarning(v: unknown): v is Warning {
  return typeof v === 'object' && v !== null && 'code' in v && 'message' in v
}

/** Builds an SSE listener that parses JSON and forwards only payloads passing `guard`. */
function typedListener<T>(guard: (v: unknown) => v is T, forward: (v: T) => void): EventListener {
  return (ev: Event) => {
    const data = parseJson<unknown>((ev as MessageEvent<string>).data)
    if (guard(data)) forward(data)
  }
}

/**
 * Low-level SSE connection to `/api/v1/live` that reconnects with exponential backoff.
 * The browser's built-in EventSource retry is disabled by closing on error.
 */
export function createLiveConnection(handlers: LiveHandlers, options: LiveOptions = {}): LiveConnection {
  const url = options.url ?? LIVE_PATH
  let source: EventSource | null = null
  let timer: ReturnType<typeof setTimeout> | null = null
  let attempt = 0
  let stopped = true

  const onTickEvent = typedListener(isTick, handlers.onTick)
  const onWarningEvent = typedListener(isWarning, handlers.onWarning)

  function scheduleReconnect(): void {
    source?.close()
    source = null
    if (stopped) return
    handlers.onState('reconnecting')
    timer = setTimeout(connect, backoffDelay(attempt, options.baseDelayMs, options.maxDelayMs))
    attempt += 1
  }

  function connect(): void {
    timer = null
    if (stopped) return
    handlers.onState(attempt === 0 ? 'connecting' : 'reconnecting')
    source = new EventSource(url, { withCredentials: false })
    source.addEventListener('open', () => {
      attempt = 0
      handlers.onState('open')
    })
    source.addEventListener('tick', onTickEvent)
    source.addEventListener('warning', onWarningEvent)
    source.addEventListener('error', scheduleReconnect)
  }

  return {
    start(): void {
      if (!stopped) return
      stopped = false
      attempt = 0
      connect()
    },
    stop(): void {
      stopped = true
      if (timer) clearTimeout(timer)
      timer = null
      source?.close()
      source = null
      handlers.onState('idle')
    },
  }
}

/** A warning from the stream plus the time (epoch ms) it was received. */
export interface LiveWarning extends Warning {
  at: number
}

export interface UseLive {
  state: Ref<LiveState>
  latest: ShallowRef<LiveTick | null>
  history: ShallowRef<readonly LiveTick[]>
  warnings: ShallowRef<readonly LiveWarning[]>
  start: () => void
  stop: () => void
}

/** Reactive live stream. State is replaced immutably on every event. */
export function useLive(options: LiveOptions = {}): UseLive {
  const state = ref<LiveState>('idle')
  const latest = shallowRef<LiveTick | null>(null)
  const history = shallowRef<readonly LiveTick[]>([])
  const warnings = shallowRef<readonly LiveWarning[]>([])

  const connection = createLiveConnection(
    {
      onTick: (tick) => {
        latest.value = tick
        history.value = [...history.value, tick].slice(-LIVE_HISTORY_SIZE)
      },
      onWarning: (w) => {
        warnings.value = [...warnings.value.filter((x) => x.code !== w.code), { ...w, at: Date.now() }]
      },
      onState: (s) => {
        state.value = s
      },
    },
    options,
  )

  if (getCurrentScope()) onScopeDispose(connection.stop)

  return { state, latest, history, warnings, start: connection.start, stop: connection.stop }
}
