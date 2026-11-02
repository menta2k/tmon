<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import {
  NConfigProvider,
  NDialogProvider,
  NGlobalStyle,
  NMessageProvider,
  NNotificationProvider,
  darkTheme,
  type GlobalThemeOverrides,
} from 'naive-ui'
import { useThemeStore } from '@/stores/theme'
import AppShell from '@/components/AppShell.vue'

const route = useRoute()
const theme = useThemeStore()

const naiveTheme = computed(() => (theme.mode === 'dark' ? darkTheme : null))
const overrides: GlobalThemeOverrides = {
  common: {
    primaryColor: '#0e7490',
    primaryColorHover: '#0891b2',
    primaryColorPressed: '#155e75',
    borderRadius: '4px',
  },
}
const isPublic = computed(() => route.meta.public === true)
</script>

<template>
  <NConfigProvider :theme="naiveTheme" :theme-overrides="overrides">
    <NGlobalStyle />
    <NMessageProvider placement="top-right">
      <NNotificationProvider>
        <NDialogProvider>
          <RouterView v-if="isPublic" />
          <AppShell v-else>
            <RouterView />
          </AppShell>
        </NDialogProvider>
      </NNotificationProvider>
    </NMessageProvider>
  </NConfigProvider>
</template>
