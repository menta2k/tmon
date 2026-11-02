import {
  getCurrentScope,
  onScopeDispose,
  ref,
  shallowRef,
  watch,
  type Ref,
  type ShallowRef,
  type WatchSource,
} from 'vue'
import type { ApiMeta, ApiResult } from '@/api/types'
import { errorMessage, isAbortError } from '@/utils/errors'

export interface AsyncDataOptions {
  /** Reload whenever any of these change. */
  watch?: WatchSource[]
  /** Auto-refresh interval in ms (only while `refreshEnabled` is true). */
  refreshMs?: number
  refreshEnabled?: Ref<boolean>
  immediate?: boolean
}

export interface AsyncData<T> {
  data: ShallowRef<T | null>
  meta: ShallowRef<ApiMeta>
  loading: Ref<boolean>
  error: Ref<string | null>
  reload: () => Promise<void>
}

/**
 * Loads data through `fetcher`, cancelling stale requests, exposing a user-friendly
 * error string, and optionally re-fetching on an interval or when sources change.
 */
export function useAsyncData<T>(
  fetcher: (signal: AbortSignal) => Promise<ApiResult<T>>,
  options: AsyncDataOptions = {},
): AsyncData<T> {
  const data = shallowRef<T | null>(null)
  const meta = shallowRef<ApiMeta>({})
  const loading = ref(false)
  const error = ref<string | null>(null)
  let controller: AbortController | null = null
  let timer: ReturnType<typeof setInterval> | null = null

  async function reload(): Promise<void> {
    controller?.abort()
    const current = new AbortController()
    controller = current
    loading.value = true
    try {
      const result = await fetcher(current.signal)
      if (current.signal.aborted) return
      data.value = result.data
      meta.value = result.meta
      error.value = null
    } catch (err) {
      if (isAbortError(err) || current.signal.aborted) return
      error.value = errorMessage(err)
    } finally {
      if (controller === current) loading.value = false
    }
  }

  if (options.watch && options.watch.length > 0) {
    watch(options.watch, () => void reload(), { deep: true })
  }

  if (options.refreshMs) {
    const ms = options.refreshMs
    timer = setInterval(() => {
      if (options.refreshEnabled?.value ?? true) void reload()
    }, ms)
  }

  if (getCurrentScope()) {
    onScopeDispose(() => {
      controller?.abort()
      if (timer) clearInterval(timer)
    })
  }

  if (options.immediate ?? true) void reload()

  return { data, meta, loading, error, reload }
}
