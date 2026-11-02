<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NButton, NForm, NFormItem, NInput, NInputNumber, NModal, NSelect, NSpace } from 'naive-ui'
import { COUNT_DIRECTIONS, HOST_NAME_MAX, SAMPLING_MAX } from '@/constants'
import type { CountDirection, ExporterPatch, ExporterStatus } from '@/api/types'
import { updateExporter } from '@/api/config'
import { useNotify } from '@/composables/useNotify'
import { validateName, validateUintInRange } from '@/utils/validators'

const props = defineProps<{ show: boolean; exporter: ExporterStatus | null }>()
const emit = defineEmits<{ 'update:show': [value: boolean]; saved: [] }>()
const notify = useNotify()

interface Form {
  name: string
  sampling_override: number | null
  count_direction: CountDirection
}
const form = ref<Form>({ name: '', sampling_override: null, count_direction: 'both' })
const saving = ref(false)

watch(
  () => [props.show, props.exporter] as const,
  ([show, e]) => {
    if (!show || !e) return
    form.value = {
      name: e.name ?? '',
      sampling_override: e.sampling_override,
      count_direction: e.count_direction,
    }
  },
  { immediate: true },
)

const nameError = computed(() =>
  form.value.name.trim() ? validateName(form.value.name, HOST_NAME_MAX) : null,
)
const samplingError = computed(() =>
  form.value.sampling_override === null
    ? null
    : validateUintInRange(form.value.sampling_override, 1, SAMPLING_MAX, 'Sampling rate'),
)
const directionOptions = COUNT_DIRECTIONS.map((d) => ({ label: d, value: d }))

function set<K extends keyof Form>(key: K, value: Form[K]): void {
  form.value = { ...form.value, [key]: value }
}

async function save(): Promise<void> {
  if (!props.exporter || nameError.value || samplingError.value) return
  const patch: ExporterPatch = {
    name: form.value.name.trim() || null,
    sampling_override: form.value.sampling_override,
    count_direction: form.value.count_direction,
  }
  saving.value = true
  const id = props.exporter.id
  const ok = await notify.run(() => updateExporter(id, patch), 'Exporter updated', 'Saving exporter')
  saving.value = false
  if (!ok) return
  emit('saved')
  emit('update:show', false)
}
</script>

<template>
  <NModal
    :show="show"
    preset="card"
    :title="`Exporter ${exporter?.address ?? ''}`"
    :style="{ maxWidth: '420px', width: '96vw' }"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="top" size="small" @submit.prevent="save">
      <NFormItem
        label="Name"
        :feedback="nameError ?? ''"
        :validation-status="nameError ? 'error' : undefined"
      >
        <NInput :value="form.name" placeholder="e.g. hAP ax3" @update:value="(v: string) => set('name', v)" />
      </NFormItem>
      <NFormItem
        label="Sampling override (1-in-N, empty = advertised)"
        :feedback="samplingError ?? ''"
        :validation-status="samplingError ? 'error' : undefined"
      >
        <NInputNumber
          :value="form.sampling_override"
          :min="1"
          :max="SAMPLING_MAX"
          :precision="0"
          clearable
          @update:value="(v: number | null) => set('sampling_override', v)"
        />
      </NFormItem>
      <NFormItem label="Count direction (avoid double counting)">
        <NSelect
          :value="form.count_direction"
          :options="directionOptions"
          @update:value="(v: CountDirection) => set('count_direction', v)"
        />
      </NFormItem>
      <NSpace justify="end">
        <NButton @click="emit('update:show', false)">Cancel</NButton>
        <NButton type="primary" attr-type="submit" :loading="saving">Save</NButton>
      </NSpace>
    </NForm>
  </NModal>
</template>
