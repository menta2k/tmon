import { getCurrentScope, onScopeDispose, ref, type Ref } from 'vue'
import { MOBILE_BREAKPOINT_PX } from '@/constants'

/** Reactive "viewport is narrower than the mobile breakpoint" flag. */
export function useIsMobile(): Ref<boolean> {
  const query =
    typeof window !== 'undefined' ? window.matchMedia?.(`(max-width: ${MOBILE_BREAKPOINT_PX}px)`) : undefined
  const isMobile = ref(query?.matches ?? false)
  if (!query) return isMobile
  const onChange = (ev: MediaQueryListEvent): void => {
    isMobile.value = ev.matches
  }
  query.addEventListener?.('change', onChange)
  if (getCurrentScope()) onScopeDispose(() => query.removeEventListener?.('change', onChange))
  return isMobile
}
