<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NButton, NForm, NFormItem, NInputNumber } from 'naive-ui'
import { DISK_CAP_GB_MAX, RETENTION_DAYS_MAX } from '@/constants'
import type { Settings } from '@/api/types'
import { updateSettings } from '@/api/config'
import { useNotify } from '@/composables/useNotify'
import { validateUintInRange } from '@/utils/validators'

const props = defineProps<{ settings: Settings }>()
const emit = defineEmits<{ saved: [settings: Settings] }>()
const notify = useNotify()

interface Form {
  raw_days: number | null
  rollup_1m_days: number | null
  rollup_1h_days: number | null
  disk_cap_gb: number | null
}
const form = ref<Form>({ raw_days: null, rollup_1m_days: null, rollup_1h_days: null, disk_cap_gb: null })
const saving = ref(false)

watch(
  () => props.settings,
  (s) => {
    form.value = { ...s.retention, disk_cap_gb: s.storage.disk_cap_gb }
  },
  { immediate: true },
)

const FIELDS: readonly { key: keyof Form; label: string; max: number }[] = [
  { key: 'raw_days', label: 'Raw flows (days)', max: RETENTION_DAYS_MAX },
  { key: 'rollup_1m_days', label: '1-minute summaries (days)', max: RETENTION_DAYS_MAX },
  { key: 'rollup_1h_days', label: '1-hour summaries (days)', max: RETENTION_DAYS_MAX },
  { key: 'disk_cap_gb', label: 'Disk cap (GB)', max: DISK_CAP_GB_MAX },
]

const errors = computed<Partial<Record<keyof Form, string>>>(() => {
  const out: Partial<Record<keyof Form, string>> = {}
  for (const f of FIELDS) {
    const v = form.value[f.key]
    const err = v === null ? 'Required' : validateUintInRange(v, 1, f.max, f.label)
    if (err) out[f.key] = err
  }
  return out
})

function set(key: keyof Form, value: number | null): void {
  form.value = { ...form.value, [key]: value }
}

async function save(): Promise<void> {
  if (Object.keys(errors.value).length > 0) return
  const f = form.value
  saving.value = true
  try {
    const saved = await updateSettings({
      retention: {
        raw_days: f.raw_days!,
        rollup_1m_days: f.rollup_1m_days!,
        rollup_1h_days: f.rollup_1h_days!,
      },
      storage: { disk_cap_gb: f.disk_cap_gb! },
    })
    notify.success('Retention saved')
    emit('saved', saved)
  } catch (err) {
    notify.error(err, 'Saving retention')
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <NForm label-placement="top" size="small" data-testid="retention-section" @submit.prevent="save">
    <div class="grid-2">
      <NFormItem
        v-for="f in FIELDS"
        :key="f.key"
        :label="f.label"
        :feedback="errors[f.key] ?? ''"
        :validation-status="errors[f.key] ? 'error' : undefined"
      >
        <NInputNumber
          :value="form[f.key]"
          :min="1"
          :max="f.max"
          :precision="0"
          @update:value="(v: number | null) => set(f.key, v)"
        />
      </NFormItem>
    </div>
    <NButton size="small" type="primary" attr-type="submit" :loading="saving">Save retention</NButton>
  </NForm>
</template>
