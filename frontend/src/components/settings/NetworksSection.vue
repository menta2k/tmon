<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { NAlert, NButton, NInput, NSpace } from 'naive-ui'
import { NETWORK_LABEL_MAX } from '@/constants'
import type { LocalNetwork } from '@/api/types'
import { fetchNetworks, saveNetworks } from '@/api/config'
import { useNotify } from '@/composables/useNotify'
import { errorMessage, fieldErrors } from '@/utils/errors'
import { validateCidr } from '@/utils/validators'

const notify = useNotify()
const rows = ref<readonly LocalNetwork[]>([])
const loadError = ref<string | null>(null)
const serverErrors = ref<Readonly<Record<string, string>>>({})
const saving = ref(false)

const rowErrors = computed(() =>
  rows.value.map(
    (r) =>
      validateCidr(r.cidr) ??
      (r.label.length > NETWORK_LABEL_MAX ? `Label must be at most ${NETWORK_LABEL_MAX} characters` : null),
  ),
)
const duplicate = computed(() => {
  const cidrs = rows.value.map((r) => r.cidr.trim()).filter((c) => c !== '')
  return cidrs.find((c, i) => cidrs.indexOf(c) !== i) ?? null
})
const hasErrors = computed(
  () => rowErrors.value.some((e) => e !== null) || duplicate.value !== null || rows.value.length === 0,
)

function update(index: number, patch: Partial<LocalNetwork>): void {
  rows.value = rows.value.map((r, i) => (i === index ? { ...r, ...patch } : r))
}
function add(): void {
  rows.value = [...rows.value, { cidr: '', label: '' }]
}
function remove(index: number): void {
  rows.value = rows.value.filter((_, i) => i !== index)
}

async function load(): Promise<void> {
  try {
    rows.value = await fetchNetworks()
    loadError.value = null
  } catch (err) {
    loadError.value = errorMessage(err, 'Loading local networks')
  }
}

async function save(): Promise<void> {
  if (hasErrors.value) return
  saving.value = true
  try {
    rows.value = await saveNetworks(rows.value.map((r) => ({ cidr: r.cidr.trim(), label: r.label.trim() })))
    serverErrors.value = {}
    notify.success('Local networks saved')
  } catch (err) {
    serverErrors.value = fieldErrors(err)
    notify.error(err, 'Saving local networks')
  } finally {
    saving.value = false
  }
}

onMounted(load)
</script>

<template>
  <div class="stack" data-testid="networks-section">
    <p class="muted">
      Addresses inside these ranges are "local"; they decide download/upload/internal direction.
    </p>
    <NAlert v-if="loadError" type="error" :bordered="false">{{ loadError }}</NAlert>
    <div v-for="(row, i) in rows" :key="i" class="net-row">
      <NInput
        :value="row.cidr"
        size="small"
        placeholder="192.168.88.0/24"
        :status="rowErrors[i] || serverErrors[`[${i}].cidr`] ? 'error' : undefined"
        @update:value="(v: string) => update(i, { cidr: v })"
      />
      <NInput
        :value="row.label"
        size="small"
        placeholder="Label"
        @update:value="(v: string) => update(i, { label: v })"
      />
      <NButton size="small" quaternary @click="remove(i)">✕</NButton>
      <div v-if="rowErrors[i] || serverErrors[`[${i}].cidr`]" class="net-row__error">
        {{ rowErrors[i] ?? serverErrors[`[${i}].cidr`] }}
      </div>
    </div>
    <NAlert v-if="duplicate" type="warning" :bordered="false">Duplicate network {{ duplicate }}</NAlert>
    <NSpace>
      <NButton size="small" @click="add">Add network</NButton>
      <NButton size="small" type="primary" :disabled="hasErrors" :loading="saving" @click="save"
        >Save networks</NButton
      >
    </NSpace>
  </div>
</template>

<style scoped>
.net-row {
  display: grid;
  grid-template-columns: minmax(140px, 1fr) minmax(120px, 1fr) auto;
  gap: 6px;
}
.net-row__error {
  grid-column: 1 / -1;
  color: #e5484d;
  font-size: 12px;
}
</style>
