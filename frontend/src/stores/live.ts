import { defineStore } from 'pinia'
import { computed } from 'vue'
import { useLive } from '@/api/live'

/** App-wide live throughput feed (one SSE connection shared by all views). */
export const useLiveStore = defineStore('live', () => {
  const live = useLive()

  const connected = computed(() => live.state.value === 'open')
  const down = computed(() => live.latest.value?.bytes_per_sec.down ?? 0)
  const up = computed(() => live.latest.value?.bytes_per_sec.up ?? 0)

  return {
    state: live.state,
    latest: live.latest,
    history: live.history,
    warnings: live.warnings,
    connected,
    down,
    up,
    start: live.start,
    stop: live.stop,
  }
})
