import { defineStore } from 'pinia'
import { computed, ref, shallowRef } from 'vue'
import { STATUS_POLL_MS } from '@/constants'
import type { Warning } from '@/api/types'
import { fetchStatus } from '@/api/system'
import { useLiveStore } from './live'

export function warningKey(w: Warning): string {
  return `${w.code}|${w.message}`
}

/** Combines `/status` warnings (polled every 30 s) with `event: warning` from the live stream. */
export const useWarningsStore = defineStore('warnings', () => {
  const live = useLiveStore()
  const polled = shallowRef<readonly Warning[]>([])
  const dismissed = shallowRef<ReadonlySet<string>>(new Set())
  const lastError = ref<string | null>(null)
  /** Live warnings received before the last successful poll started are superseded by it. */
  const liveCutoff = ref(0)
  let timer: ReturnType<typeof setInterval> | null = null

  const all = computed<readonly Warning[]>(() => {
    const merged = new Map<string, Warning>()
    for (const w of polled.value) merged.set(warningKey(w), w)
    for (const w of live.warnings) {
      if (w.at >= liveCutoff.value) merged.set(warningKey(w), { code: w.code, message: w.message })
    }
    return [...merged.values()]
  })

  const visible = computed(() => all.value.filter((w) => !dismissed.value.has(warningKey(w))))

  async function poll(): Promise<void> {
    const startedAt = Date.now()
    try {
      const status = await fetchStatus()
      polled.value = status.warnings ?? []
      liveCutoff.value = startedAt
      lastError.value = null
    } catch (err) {
      lastError.value = err instanceof Error ? err.message : 'Status unavailable'
    }
  }

  function start(): void {
    if (timer) return
    void poll()
    timer = setInterval(() => void poll(), STATUS_POLL_MS)
  }

  function stop(): void {
    if (timer) clearInterval(timer)
    timer = null
  }

  function dismiss(w: Warning): void {
    dismissed.value = new Set([...dismissed.value, warningKey(w)])
  }

  /** Lets the Status view push a fresh result without waiting for the next poll. */
  function setPolled(next: readonly Warning[]): void {
    polled.value = [...next]
  }

  return { all, visible, lastError, poll, start, stop, dismiss, setPolled }
})
