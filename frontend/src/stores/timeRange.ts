import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

export type PresetKey = '15m' | '1h' | '24h' | '7d' | '30d'
export type RangeKey = PresetKey | 'custom'

export interface CustomRange {
  /** epoch ms */
  from: number
  /** epoch ms */
  to: number
}

export const TIME_PRESETS: readonly { key: PresetKey; label: string }[] = [
  { key: '15m', label: 'Last 15 min' },
  { key: '1h', label: 'Last 1 h' },
  { key: '24h', label: 'Last 24 h' },
  { key: '7d', label: 'Last 7 d' },
  { key: '30d', label: 'Last 30 d' },
]

export const DEFAULT_PRESET: PresetKey = '15m'

export interface RangeParams {
  from: string
  to: string
}

export const useTimeRangeStore = defineStore('timeRange', () => {
  const key = ref<RangeKey>(DEFAULT_PRESET)
  const custom = ref<CustomRange | null>(null)

  const params = computed<RangeParams>(() => {
    if (key.value === 'custom' && custom.value) {
      return { from: new Date(custom.value.from).toISOString(), to: new Date(custom.value.to).toISOString() }
    }
    const preset = key.value === 'custom' ? DEFAULT_PRESET : key.value
    return { from: `now-${preset}`, to: 'now' }
  })

  const isLive = computed(() => key.value !== 'custom')

  const label = computed(() => {
    if (key.value !== 'custom') return TIME_PRESETS.find((p) => p.key === key.value)?.label ?? key.value
    return 'Custom range'
  })

  function setPreset(preset: PresetKey): void {
    key.value = preset
    custom.value = null
  }

  /** Throws when the range is inverted so the picker can show the error. */
  function setCustom(from: number, to: number): void {
    if (!Number.isFinite(from) || !Number.isFinite(to) || from >= to) {
      throw new Error('The start of the range must be before its end')
    }
    custom.value = { from, to }
    key.value = 'custom'
  }

  return { key, custom, params, isLive, label, setPreset, setCustom }
})
