import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { Direction } from '@/api/types'
import type { QueryParams } from '@/api/query'

export interface Filters {
  host: readonly string[]
  category: readonly number[]
  proto: readonly number[]
  direction: readonly Direction[]
  country: readonly string[]
  asn: readonly number[]
  exporter: readonly number[]
}

export type FilterKey = keyof Filters

export const EMPTY_FILTERS: Readonly<Filters> = Object.freeze({
  host: [],
  category: [],
  proto: [],
  direction: [],
  country: [],
  asn: [],
  exporter: [],
})

export const useFiltersStore = defineStore('filters', () => {
  /** Always replaced, never mutated in place. */
  const filters = ref<Readonly<Filters>>(EMPTY_FILTERS)

  const params = computed<QueryParams>(() => {
    const entries = Object.entries(filters.value).filter(([, v]) => (v as readonly unknown[]).length > 0)
    return Object.fromEntries(entries) as QueryParams
  })

  const activeCount = computed(() =>
    Object.values(filters.value).reduce((n, v) => n + (v as readonly unknown[]).length, 0),
  )

  function setFilter<K extends FilterKey>(key: K, values: Filters[K]): void {
    filters.value = { ...filters.value, [key]: [...new Set<Filters[K][number]>(values)] }
  }

  function addValue<K extends FilterKey>(key: K, value: Filters[K][number]): void {
    const current = filters.value[key] as readonly Filters[K][number][]
    if (current.includes(value)) return
    filters.value = { ...filters.value, [key]: [...current, value] }
  }

  function removeValue<K extends FilterKey>(key: K, value: Filters[K][number]): void {
    const current = filters.value[key] as readonly Filters[K][number][]
    filters.value = { ...filters.value, [key]: current.filter((v) => v !== value) }
  }

  function clear(): void {
    filters.value = EMPTY_FILTERS
  }

  return { filters, params, activeCount, setFilter, addValue, removeValue, clear }
})
