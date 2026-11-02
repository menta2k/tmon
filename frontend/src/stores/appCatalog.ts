import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { AppCatalogEntry } from '@/api/types'
import { fetchAppCatalog } from '@/api/applications'
import { errorMessage } from '@/utils/errors'

const sortedUnique = (values: readonly string[]): string[] =>
  [...new Set(values.filter((v) => v !== ''))].sort((a, b) => a.localeCompare(b))

/**
 * Classifier application catalog (feature 003). It only feeds pickers, so a load failure
 * is recorded in `error` (shown as a hint) rather than thrown; free text still works.
 */
export const useAppCatalogStore = defineStore('appCatalog', () => {
  const items = ref<readonly AppCatalogEntry[]>([])
  const loaded = ref(false)
  const error = ref<string | null>(null)
  let inflight: Promise<void> | null = null

  const apps = computed(() => sortedUnique(items.value.map((e) => e.app)))
  const categories = computed(() => sortedUnique(items.value.map((e) => e.category)))

  async function doLoad(): Promise<void> {
    try {
      items.value = await fetchAppCatalog()
      loaded.value = true
      error.value = null
    } catch (err) {
      error.value = errorMessage(err, 'Loading the application catalog')
    } finally {
      inflight = null
    }
  }

  /** Loads the catalog once (deduplicating concurrent calls). */
  function load(force = false): Promise<void> {
    if (loaded.value && !force) return Promise.resolve()
    inflight ??= doLoad()
    return inflight
  }

  return { items, loaded, error, apps, categories, load }
})
