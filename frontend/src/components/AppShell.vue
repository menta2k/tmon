<script setup lang="ts">
import { computed, h, onMounted, onUnmounted, ref, watch, type Component } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import {
  NButton,
  NLayout,
  NLayoutContent,
  NLayoutHeader,
  NLayoutSider,
  NMenu,
  NTooltip,
  type MenuOption,
} from 'naive-ui'
import { useThemeStore } from '@/stores/theme'
import { useLiveStore } from '@/stores/live'
import { useAuthStore } from '@/stores/auth'
import { useIsMobile } from '@/composables/useIsMobile'
import { useNotify } from '@/composables/useNotify'
import { formatBps } from '@/utils/format'
import TimeRangePicker from './TimeRangePicker.vue'
import WarningBanner from './WarningBanner.vue'

const NAV: readonly { name: string; label: string; icon: string }[] = [
  { name: 'dashboard', label: 'Dashboard', icon: '◧' },
  { name: 'top-hosts', label: 'Top hosts', icon: '⇅' },
  { name: 'remotes', label: 'Remotes', icon: '◎' },
  { name: 'flows', label: 'Flows', icon: '≡' },
  { name: 'rules', label: 'Rules', icon: '⚑' },
  { name: 'hosts', label: 'Host names', icon: '✎' },
  { name: 'status', label: 'Status', icon: '♥' },
  { name: 'settings', label: 'Settings', icon: '⚙' },
]

const route = useRoute()
const router = useRouter()
const theme = useThemeStore()
const live = useLiveStore()
const auth = useAuthStore()
const notify = useNotify()
const isMobile = useIsMobile()
const collapsed = ref(isMobile.value)

watch(isMobile, (m) => {
  collapsed.value = m
})

const icon =
  (glyph: string): (() => ReturnType<typeof h>) =>
  () =>
    h('span', { class: 'nav-icon' }, glyph)
const menuOptions: MenuOption[] = NAV.map((item) => ({
  key: item.name,
  icon: icon(item.icon),
  label: () => h(RouterLink as Component, { to: { name: item.name } }, { default: () => item.label }),
}))

const activeKey = computed(() => (route.name === 'host-detail' ? 'top-hosts' : String(route.name ?? '')))
const liveText = computed(() => `↓ ${formatBps(live.down)}  ↑ ${formatBps(live.up)}`)

async function onLogout(): Promise<void> {
  try {
    await auth.logout()
  } catch (err) {
    notify.error(err, 'Logout')
  }
  await router.push({ name: 'login' })
}

function onMenuSelect(): void {
  if (isMobile.value) collapsed.value = true
}

onMounted(() => live.start())
onUnmounted(() => live.stop())
</script>

<template>
  <NLayout has-sider class="shell">
    <NLayoutSider
      bordered
      collapse-mode="width"
      :collapsed-width="isMobile ? 0 : 56"
      :width="190"
      :collapsed="collapsed"
      show-trigger="bar"
      :native-scrollbar="false"
      @update:collapsed="(v: boolean) => (collapsed = v)"
    >
      <div class="brand" :class="{ 'brand--collapsed': collapsed }">
        <span class="brand__logo">t</span><span v-if="!collapsed" class="brand__name">tmon</span>
      </div>
      <NMenu
        :value="activeKey"
        :options="menuOptions"
        :collapsed="collapsed"
        :collapsed-width="56"
        :collapsed-icon-size="18"
        @update:value="onMenuSelect"
      />
    </NLayoutSider>
    <NLayout>
      <NLayoutHeader bordered class="header">
        <NButton v-if="isMobile" quaternary size="small" aria-label="Menu" @click="collapsed = !collapsed"
          >☰</NButton
        >
        <TimeRangePicker />
        <NTooltip>
          <template #trigger>
            <span class="live mono" :class="{ 'live--off': !live.connected }" data-testid="live-indicator">
              <span class="live__dot" />{{ liveText }}
            </span>
          </template>
          {{ live.connected ? 'Live feed connected' : `Live feed ${live.state}` }}
        </NTooltip>
        <span class="header__spacer" />
        <NButton quaternary size="small" :aria-label="'Toggle theme'" @click="theme.toggle()">
          {{ theme.mode === 'dark' ? '☀' : '☾' }}
        </NButton>
        <NButton quaternary size="small" data-testid="logout" @click="onLogout">Log out</NButton>
      </NLayoutHeader>
      <NLayoutContent class="content" :native-scrollbar="false">
        <WarningBanner />
        <slot />
      </NLayoutContent>
    </NLayout>
  </NLayout>
</template>

<style scoped>
.shell {
  height: 100vh;
}
.brand {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 14px 16px;
  font-weight: 700;
  letter-spacing: 0.04em;
}
.brand--collapsed {
  justify-content: center;
  padding: 14px 0;
}
.brand__logo {
  display: inline-grid;
  place-items: center;
  width: 24px;
  height: 24px;
  border-radius: 5px;
  background: #0e7490;
  color: #fff;
}
.header {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 12px;
  flex-wrap: wrap;
}
.header__spacer {
  flex: 1;
}
.live {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  white-space: pre;
}
.live__dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #10b981;
}
.live--off .live__dot {
  background: #94a3b8;
}
.content {
  padding: 12px;
  height: calc(100vh - 46px);
}
:deep(.nav-icon) {
  font-size: 15px;
}
@media (max-width: 768px) {
  .content {
    padding: 8px;
  }
  .live {
    display: none;
  }
}
</style>
