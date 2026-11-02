import { computed, type ComputedRef } from 'vue'
import { storeToRefs } from 'pinia'
import { mergeParams, type QueryParams } from '@/api/query'
import { useTimeRangeStore } from '@/stores/timeRange'
import { useFiltersStore } from '@/stores/filters'

/** Common analytics query params: the time range plus all active filters. */
export function useRangeFilters(): { params: ComputedRef<QueryParams>; rangeOnly: ComputedRef<QueryParams> } {
  const { params: range } = storeToRefs(useTimeRangeStore())
  const { params: filters } = storeToRefs(useFiltersStore())
  const params = computed(() => mergeParams(range.value as unknown as QueryParams, filters.value))
  const rangeOnly = computed(() => ({ ...range.value }) as QueryParams)
  return { params, rangeOnly }
}
