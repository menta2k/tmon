import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { FALLBACK_COLOR } from '@/constants'
import type { Category } from '@/api/types'
import { fetchCategories } from '@/api/config'

export const useCategoriesStore = defineStore('categories', () => {
  const items = ref<readonly Category[]>([])
  const loaded = ref(false)
  const loading = ref(false)
  let inflight: Promise<void> | null = null

  const byId = computed<ReadonlyMap<number, Category>>(() => new Map(items.value.map((c) => [c.id, c])))

  function colorFor(id: number | string | null | undefined): string {
    if (id === null || id === undefined) return FALLBACK_COLOR
    return byId.value.get(Number(id))?.color ?? FALLBACK_COLOR
  }

  function nameFor(id: number | string | null | undefined): string {
    if (id === null || id === undefined) return '—'
    return byId.value.get(Number(id))?.name ?? `#${id}`
  }

  async function doLoad(): Promise<void> {
    loading.value = true
    try {
      items.value = await fetchCategories()
      loaded.value = true
    } finally {
      loading.value = false
      inflight = null
    }
  }

  /** Loads categories (deduplicating concurrent calls). Errors propagate to the caller. */
  function load(force = false): Promise<void> {
    if (loaded.value && !force) return Promise.resolve()
    inflight ??= doLoad()
    return inflight
  }

  function replace(next: readonly Category[]): void {
    items.value = [...next]
    loaded.value = true
  }

  return { items, loaded, loading, byId, colorFor, nameFor, load, replace }
})
