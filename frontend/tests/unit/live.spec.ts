import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { backoffDelay, createLiveConnection, useLive } from '@/api/live'
import { useLiveStore } from '@/stores/live'
import { MockEventSource } from './eventSourceMock'

const tick = {
  ts: '2024-01-01T00:00:00Z',
  bytes_per_sec: { down: 1000, up: 200 },
  by_category: [{ id: 1, bps: 800 }],
}

beforeEach(() => {
  vi.useFakeTimers()
  MockEventSource.reset()
  vi.stubGlobal('EventSource', MockEventSource)
})
afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('backoffDelay', () => {
  it('grows exponentially and caps', () => {
    expect(backoffDelay(0, 1000, 30000)).toBe(1000)
    expect(backoffDelay(3, 1000, 30000)).toBe(8000)
    expect(backoffDelay(10, 1000, 30000)).toBe(30000)
    expect(backoffDelay(-1, 1000, 30000)).toBe(1000)
  })
})

describe('createLiveConnection', () => {
  it('connects, parses events and reconnects with backoff', () => {
    const onTick = vi.fn()
    const onWarning = vi.fn()
    const onState = vi.fn()
    const conn = createLiveConnection({ onTick, onWarning, onState }, { baseDelayMs: 100, maxDelayMs: 1000 })
    conn.start()
    conn.start() // idempotent
    expect(MockEventSource.instances).toHaveLength(1)
    const first = MockEventSource.latest()
    expect(first.url).toBe('/api/v1/live')
    first.emit('open')
    expect(onState).toHaveBeenLastCalledWith('open')
    first.emit('tick', tick)
    first.emit('tick', 'not json')
    first.emit('tick', { nope: true })
    expect(onTick).toHaveBeenCalledTimes(1)
    first.emit('warning', { code: 'stalled', message: 'No flows' })
    first.emit('warning', { bad: 1 })
    expect(onWarning).toHaveBeenCalledTimes(1)

    first.emit('error')
    expect(first.closed).toBe(true)
    expect(onState).toHaveBeenLastCalledWith('reconnecting')
    vi.advanceTimersByTime(99)
    expect(MockEventSource.instances).toHaveLength(1)
    vi.advanceTimersByTime(1)
    expect(MockEventSource.instances).toHaveLength(2)
    MockEventSource.latest().emit('error')
    vi.advanceTimersByTime(200)
    expect(MockEventSource.instances).toHaveLength(3)

    conn.stop()
    expect(MockEventSource.latest().closed).toBe(true)
    expect(onState).toHaveBeenLastCalledWith('idle')
    MockEventSource.latest().emit('error')
    vi.advanceTimersByTime(5000)
    expect(MockEventSource.instances).toHaveLength(3)
  })

  it('stop cancels a pending reconnect', () => {
    const conn = createLiveConnection(
      { onTick: vi.fn(), onWarning: vi.fn(), onState: vi.fn() },
      { baseDelayMs: 100 },
    )
    conn.start()
    MockEventSource.latest().emit('error')
    conn.stop()
    vi.advanceTimersByTime(1000)
    expect(MockEventSource.instances).toHaveLength(1)
  })
})

describe('useLive', () => {
  it('replaces state immutably and stops with its scope', () => {
    const scope = effectScope()
    const live = scope.run(() => useLive())!
    live.start()
    const src = MockEventSource.latest()
    src.emit('open')
    const before = live.history.value
    src.emit('tick', tick)
    expect(live.latest.value).toEqual(tick)
    expect(live.history.value).not.toBe(before)
    expect(before).toHaveLength(0)
    src.emit('warning', { code: 'a', message: 'one' })
    src.emit('warning', { code: 'a', message: 'two' })
    expect(live.warnings.value.map((w) => w.message)).toEqual(['two'])
    expect(live.state.value).toBe('open')
    scope.stop()
    expect(src.closed).toBe(true)
  })

  it('caps the history length', () => {
    const live = useLive()
    live.start()
    const src = MockEventSource.latest()
    for (let i = 0; i < 130; i += 1) src.emit('tick', { ...tick, ts: i })
    expect(live.history.value).toHaveLength(120)
    expect(live.history.value[0]?.ts).toBe(10)
    live.stop()
  })
})

describe('live store', () => {
  it('exposes down/up and connection state', () => {
    setActivePinia(createPinia())
    const store = useLiveStore()
    expect(store.down).toBe(0)
    store.start()
    MockEventSource.latest().emit('open')
    MockEventSource.latest().emit('tick', tick)
    expect(store.connected).toBe(true)
    expect(store.down).toBe(1000)
    expect(store.up).toBe(200)
    store.stop()
    expect(store.connected).toBe(false)
  })
})
