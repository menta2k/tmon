import { defineStore } from 'pinia'
import { ref, watch } from 'vue'
import { THEME_STORAGE_KEY } from '@/constants'

export type ThemeMode = 'light' | 'dark'

function readStored(): ThemeMode | null {
  try {
    const v = localStorage.getItem(THEME_STORAGE_KEY)
    return v === 'light' || v === 'dark' ? v : null
  } catch {
    return null
  }
}

function systemPrefersDark(): boolean {
  return typeof window !== 'undefined' && !!window.matchMedia?.('(prefers-color-scheme: dark)').matches
}

export const useThemeStore = defineStore('theme', () => {
  const mode = ref<ThemeMode>(readStored() ?? (systemPrefersDark() ? 'dark' : 'light'))

  watch(mode, (m) => {
    try {
      localStorage.setItem(THEME_STORAGE_KEY, m)
    } catch {
      // Storage unavailable (private mode): the choice simply is not remembered.
    }
  })

  function toggle(): void {
    mode.value = mode.value === 'dark' ? 'light' : 'dark'
  }

  return { mode, toggle }
})
