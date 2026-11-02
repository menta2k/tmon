<script setup lang="ts">
import { computed, ref } from 'vue'
import { NDatePicker, NSelect, NSpace } from 'naive-ui'
import { storeToRefs } from 'pinia'
import { TIME_PRESETS, useTimeRangeStore, type PresetKey, type RangeKey } from '@/stores/timeRange'
import { useNotify } from '@/composables/useNotify'

const store = useTimeRangeStore()
const { key, custom } = storeToRefs(store)
const notify = useNotify()
const pendingCustom = ref(false)

const options = [
  ...TIME_PRESETS.map((p) => ({ label: p.label, value: p.key })),
  { label: 'Custom…', value: 'custom' },
]

const selected = computed<RangeKey>(() => (pendingCustom.value ? 'custom' : key.value))
const showPicker = computed(() => selected.value === 'custom')
const pickerValue = computed<[number, number] | null>(() =>
  custom.value ? [custom.value.from, custom.value.to] : null,
)

function onSelect(value: RangeKey): void {
  if (value === 'custom') {
    pendingCustom.value = true
    return
  }
  pendingCustom.value = false
  store.setPreset(value as PresetKey)
}

function onCustom(value: [number, number] | null): void {
  if (!value) return
  try {
    store.setCustom(value[0], value[1])
    pendingCustom.value = false
  } catch (err) {
    notify.error(err)
  }
}
</script>

<template>
  <NSpace :size="6" :wrap="false" align="center" class="trp">
    <NSelect
      :value="selected"
      :options="options"
      size="small"
      class="trp__select"
      data-testid="time-range-select"
      @update:value="onSelect"
    />
    <NDatePicker
      v-if="showPicker"
      type="datetimerange"
      size="small"
      clearable
      :value="pickerValue"
      :is-date-disabled="(ts: number) => ts > Date.now()"
      data-testid="time-range-custom"
      @update:value="onCustom"
    />
  </NSpace>
</template>

<style scoped>
.trp__select {
  width: 140px;
}
</style>
