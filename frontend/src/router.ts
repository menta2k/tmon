import { createRouter, createWebHistory, type RouteRecordRaw, type Router } from 'vue-router'
import { setUnauthorizedHandler } from '@/api/client'
import { useAuthStore } from '@/stores/auth'

declare module 'vue-router' {
  interface RouteMeta {
    public?: boolean
    title?: string
  }
}

export const routes: RouteRecordRaw[] = [
  {
    path: '/login',
    name: 'login',
    component: () => import('@/views/LoginView.vue'),
    meta: { public: true, title: 'Log in' },
  },
  {
    path: '/',
    name: 'dashboard',
    component: () => import('@/views/DashboardView.vue'),
    meta: { title: 'Dashboard' },
  },
  {
    path: '/hosts/top',
    name: 'top-hosts',
    component: () => import('@/views/TopHostsView.vue'),
    meta: { title: 'Top hosts' },
  },
  {
    path: '/hosts/:ip',
    name: 'host-detail',
    component: () => import('@/views/HostDetailView.vue'),
    props: true,
    meta: { title: 'Host' },
  },
  {
    path: '/remotes',
    name: 'remotes',
    component: () => import('@/views/RemotesView.vue'),
    meta: { title: 'Remotes' },
  },
  {
    path: '/flows',
    name: 'flows',
    component: () => import('@/views/FlowsView.vue'),
    meta: { title: 'Flows' },
  },
  {
    path: '/rules',
    name: 'rules',
    component: () => import('@/views/RulesView.vue'),
    meta: { title: 'Rules' },
  },
  {
    path: '/hosts',
    name: 'hosts',
    component: () => import('@/views/HostsView.vue'),
    meta: { title: 'Host names' },
  },
  {
    path: '/status',
    name: 'status',
    component: () => import('@/views/StatusView.vue'),
    meta: { title: 'Status' },
  },
  {
    path: '/settings',
    name: 'settings',
    component: () => import('@/views/SettingsView.vue'),
    meta: { title: 'Settings' },
  },
  { path: '/:pathMatch(.*)*', redirect: '/' },
]

export function createAppRouter(): Router {
  const router = createRouter({ history: createWebHistory(), routes })

  router.beforeEach(async (to) => {
    if (to.meta.public) return true
    const ok = await useAuthStore().ensure()
    return ok ? true : { name: 'login', query: { redirect: to.fullPath } }
  })

  router.afterEach((to) => {
    document.title = to.meta.title ? `${to.meta.title} · tmon` : 'tmon'
  })

  setUnauthorizedHandler(() => {
    useAuthStore().markLoggedOut()
    const current = router.currentRoute.value
    if (current.name !== 'login') void router.push({ name: 'login', query: { redirect: current.fullPath } })
  })

  return router
}
