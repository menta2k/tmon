import { defineComponent, h, type Component } from 'vue'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { NDialogProvider, NMessageProvider } from 'naive-ui'
import type { ApiResult } from '@/api/types'

export function ok<T>(data: T, meta: ApiResult<T>['meta'] = {}): ApiResult<T> {
  return { data, meta }
}

export interface Mounted {
  wrapper: VueWrapper
  router: Router
  pinia: Pinia
}

const Blank = defineComponent({ render: () => h('div') })

export function makeRouter(): Router {
  return createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', name: 'dashboard', component: Blank },
      { path: '/login', name: 'login', component: Blank },
      { path: '/hosts/:ip', name: 'host-detail', component: Blank },
      { path: '/hosts', name: 'hosts', component: Blank },
      { path: '/:p(.*)*', component: Blank },
    ],
  })
}

/** Mounts a component inside Naive UI providers with a fresh Pinia and memory router. */
export async function mountWithApp(
  component: Component,
  props: Record<string, unknown> = {},
  pinia: Pinia = createPinia(),
): Promise<Mounted> {
  setActivePinia(pinia)
  const router = makeRouter()
  await router.push('/')
  await router.isReady()
  const Root = defineComponent({
    render: () => h(NMessageProvider, null, () => h(NDialogProvider, null, () => h(component, props))),
  })
  const wrapper = mount(Root, { global: { plugins: [pinia, router] }, attachTo: document.body })
  await flushPromises()
  return { wrapper, router, pinia }
}
