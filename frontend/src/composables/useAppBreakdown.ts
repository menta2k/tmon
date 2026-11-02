import { computed, ref, watch, type Ref } from 'vue'
import { UNSUPPORTED_FILTER_RANGE } from '@/constants'
import { fetchBreakdown } from '@/api/analytics'
import { fetchStatus } from '@/api/system'
import { ApiError } from '@/api/client'
import type { QueryParams } from '@/api/query'
import type { ApiResult, BreakdownItem } from '@/api/types'
import { useAsyncData, type AsyncData } from './useAsyncData'

export interface AppBreakdown extends AsyncData<BreakdownItem[]> {
  /** The range is beyond raw retention (`unsupported_filter_range`). */
  rangeUnsupported: Ref<boolean>
  /** `/status.dpi.enabled`; null until known (or on servers without the key). */
  dpiEnabled: Ref<boolean | null>
}

const EMPTY: ApiResult<BreakdownItem[]> = { data: [], meta: {} }

/**
 * The dashboard's `by=app` breakdown (feature 003). Loads only while `active` is true;
 * a non-raw range is reported through `rangeUnsupported` instead of as an error.
 */
export function useAppBreakdown(
  params: Ref<QueryParams>,
  active: Ref<boolean>,
  refresh: { refreshMs?: number; refreshEnabled?: Ref<boolean> } = {},
): AppBreakdown {
  const rangeUnsupported = ref(false)
  const dpiEnabled = ref<boolean | null>(null)

  async function fetcher(): Promise<ApiResult<BreakdownItem[]>> {
    try {
      const result = await fetchBreakdown('app', params.value)
      rangeUnsupported.value = false
      return result
    } catch (err) {
      if (!(err instanceof ApiError && err.code === UNSUPPORTED_FILTER_RANGE)) throw err
      rangeUnsupported.value = true
      return EMPTY
    }
  }

  async function loadDpiState(): Promise<void> {
    if (dpiEnabled.value !== null) return
    try {
      const dpi = (await fetchStatus()).dpi
      dpiEnabled.value = dpi ? dpi.enabled : null
    } catch {
      // The hint is optional; the breakdown request reports its own errors.
      dpiEnabled.value = null
    }
  }

  const data = useAsyncData(fetcher, {
    immediate: false,
    refreshMs: refresh.refreshMs,
    refreshEnabled: computed(() => active.value && (refresh.refreshEnabled?.value ?? true)),
  })

  watch(
    [active, params],
    ([on]) => {
      if (!on) return
      void data.reload()
      void loadDpiState()
    },
    { deep: true },
  )

  return { ...data, rangeUnsupported, dpiEnabled }
}
