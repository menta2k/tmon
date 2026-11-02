<script setup lang="ts">
import { onMounted, ref, shallowRef } from 'vue'
import { NAlert, NCard, NSpin } from 'naive-ui'
import type { Settings } from '@/api/types'
import { fetchSettings } from '@/api/config'
import { errorMessage } from '@/utils/errors'
import NetworksSection from '@/components/settings/NetworksSection.vue'
import RetentionSection from '@/components/settings/RetentionSection.vue'
import ImportExportSection from '@/components/settings/ImportExportSection.vue'
import RouterSection from '@/components/settings/RouterSection.vue'

const settings = shallowRef<Settings | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const networksKey = ref(0)

async function load(): Promise<void> {
  loading.value = true
  try {
    settings.value = await fetchSettings()
    error.value = null
  } catch (err) {
    error.value = errorMessage(err, 'Loading settings')
  } finally {
    loading.value = false
  }
}

function onSaved(next: Settings): void {
  settings.value = next
}

function onImported(): void {
  networksKey.value += 1
  void load()
}

onMounted(load)
</script>

<template>
  <div class="stack settings">
    <div class="page-toolbar"><span class="page-toolbar__title">Settings</span></div>
    <NAlert v-if="error" type="error" :bordered="false">{{ error }}</NAlert>
    <NCard size="small" title="Local networks">
      <NetworksSection :key="networksKey" />
    </NCard>
    <NSpin :show="loading && !settings">
      <div class="stack">
        <NCard size="small" title="Retention and disk cap">
          <RetentionSection v-if="settings" :settings="settings" @saved="onSaved" />
        </NCard>
        <NCard size="small" title="Router integration">
          <RouterSection v-if="settings" :settings="settings" @saved="onSaved" />
        </NCard>
      </div>
    </NSpin>
    <NCard size="small" title="Export / import">
      <ImportExportSection @imported="onImported" />
    </NCard>
  </div>
</template>

<style scoped>
.settings {
  max-width: 980px;
}
</style>
