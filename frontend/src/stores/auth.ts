import { defineStore } from 'pinia'
import { ref } from 'vue'
import { ApiError, request } from '@/api/client'
import { login as apiLogin, logout as apiLogout } from '@/api/system'
import type { Category } from '@/api/types'
import { useCategoriesStore } from './categories'

/**
 * Session state. The `tmon_session` cookie is HttpOnly, so authentication is probed
 * with a cheap authenticated call (`GET /categories`, which the UI needs anyway).
 */
export const useAuthStore = defineStore('auth', () => {
  const authenticated = ref<boolean | null>(null)

  async function ensure(): Promise<boolean> {
    if (authenticated.value) return true
    try {
      const { data } = await request<Category[]>('GET', '/categories', { skipAuthRedirect: true })
      useCategoriesStore().replace(data)
      authenticated.value = true
    } catch (err) {
      // Non-auth failures (e.g. server down) still let the page render and show its own errors.
      authenticated.value = !(err instanceof ApiError && err.isUnauthorized)
    }
    return authenticated.value
  }

  async function login(password: string): Promise<void> {
    await apiLogin(password)
    authenticated.value = true
  }

  async function logout(): Promise<void> {
    try {
      await apiLogout()
    } finally {
      authenticated.value = false
    }
  }

  function markLoggedOut(): void {
    authenticated.value = false
  }

  return { authenticated, ensure, login, logout, markLoggedOut }
})
