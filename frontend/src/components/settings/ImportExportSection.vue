<script setup lang="ts">
import { computed, ref } from 'vue'
import { NAlert, NButton, NSpace, NTable } from 'naive-ui'
import { exportSettings, importSettings } from '@/api/config'
import type { SettingsBundle } from '@/api/types'
import { useNotify } from '@/composables/useNotify'
import { downloadJson, readJsonFile } from '@/utils/download'
import { errorMessage, fieldErrors } from '@/utils/errors'

const emit = defineEmits<{ imported: [] }>()
const notify = useNotify()
const exporting = ref(false)
const importing = ref(false)
const importError = ref<string | null>(null)
const importFieldErrors = ref<Readonly<Record<string, string>>>({})
const fileInput = ref<HTMLInputElement | null>(null)

const fieldRows = computed(() => Object.entries(importFieldErrors.value))

async function doExport(): Promise<void> {
  exporting.value = true
  try {
    const bundle = await exportSettings()
    downloadJson(bundle, `tmon-settings-${new Date().toISOString().slice(0, 10)}.json`)
  } catch (err) {
    notify.error(err, 'Export')
  } finally {
    exporting.value = false
  }
}

function isBundle(value: unknown): value is SettingsBundle {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

async function onFile(ev: Event): Promise<void> {
  const input = ev.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  importing.value = true
  importError.value = null
  importFieldErrors.value = {}
  try {
    const parsed = await readJsonFile(file)
    if (!isBundle(parsed)) throw new Error('The file does not contain a tmon settings bundle')
    await importSettings(parsed)
    notify.success('Settings imported')
    emit('imported')
  } catch (err) {
    importError.value = errorMessage(err, 'Import rejected')
    importFieldErrors.value = fieldErrors(err)
  } finally {
    importing.value = false
  }
}
</script>

<template>
  <div class="stack" data-testid="import-export-section">
    <p class="muted">
      Categories, rules, host names, networks and settings (no secrets). Imports are validated in full and
      applied atomically.
    </p>
    <NSpace>
      <NButton size="small" :loading="exporting" data-testid="settings-export" @click="doExport"
        >Download JSON</NButton
      >
      <NButton size="small" :loading="importing" @click="fileInput?.click()">Upload JSON…</NButton>
      <input
        ref="fileInput"
        type="file"
        accept="application/json,.json"
        hidden
        data-testid="settings-import-file"
        @change="onFile"
      />
    </NSpace>
    <NAlert v-if="importError" type="error" :bordered="false" data-testid="import-error">{{
      importError
    }}</NAlert>
    <NTable v-if="fieldRows.length > 0" size="small" :single-line="false">
      <thead>
        <tr>
          <th>Field</th>
          <th>Problem</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="[field, msg] in fieldRows" :key="field">
          <td class="mono">{{ field }}</td>
          <td>{{ msg }}</td>
        </tr>
      </tbody>
    </NTable>
  </div>
</template>
