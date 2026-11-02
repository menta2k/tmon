<script setup lang="ts">
import { computed, onMounted, shallowRef } from 'vue'
import { NButton, NSelect, type SelectOption } from 'naive-ui'
import { storeToRefs } from 'pinia'
import { DIRECTIONS, PROTOCOL_NAMES } from '@/constants'
import type { Direction, Exporter } from '@/api/types'
import { fetchExporters } from '@/api/config'
import { useFiltersStore, type FilterKey, type Filters } from '@/stores/filters'
import { useCategoriesStore } from '@/stores/categories'
import { useNotify } from '@/composables/useNotify'
import { directionLabel } from '@/utils/format'
import {
  validateAsn,
  validateIp,
  validateIso2,
  validateProto,
  type ValidationResult,
} from '@/utils/validators'

const props = withDefaults(defineProps<{ fields?: readonly FilterKey[] }>(), {
  fields: () => ['host', 'category', 'proto', 'direction', 'country', 'asn', 'exporter'],
})

const store = useFiltersStore()
const { filters, activeCount } = storeToRefs(store)
const categories = useCategoriesStore()
const notify = useNotify()
const exporters = shallowRef<readonly Exporter[]>([])

const shown = computed(() => new Set(props.fields))

const categoryOptions = computed<SelectOption[]>(() =>
  categories.items.map((c) => ({ label: c.name, value: c.id })),
)
const protoOptions: SelectOption[] = Object.entries(PROTOCOL_NAMES).map(([n, name]) => ({
  label: `${name} (${n})`,
  value: Number(n),
}))
const directionOptions: SelectOption[] = DIRECTIONS.map((d) => ({ label: directionLabel(d), value: d }))
const exporterOptions = computed<SelectOption[]>(() =>
  exporters.value.map((e) => ({ label: e.name ? `${e.name} (${e.address})` : e.address, value: e.id })),
)

const toOptions = (values: readonly (string | number)[]): SelectOption[] =>
  values.map((v) => ({ label: String(v), value: v }))

/** Validates free-text tag values; invalid entries are dropped with a toast. */
function accept<T>(
  raw: readonly unknown[],
  validate: (v: string) => ValidationResult,
  convert: (v: string) => T,
): T[] {
  const out: T[] = []
  for (const value of raw) {
    const text = String(value).trim()
    const err = validate(text)
    if (err) notify.error(new Error(err))
    else out.push(convert(text))
  }
  return out
}

function setHosts(values: unknown[]): void {
  store.setFilter(
    'host',
    accept(values, validateIp, (v) => v.toLowerCase()),
  )
}
function setCountries(values: unknown[]): void {
  store.setFilter(
    'country',
    accept(values, validateIso2, (v) => v.toUpperCase()),
  )
}
function setAsns(values: unknown[]): void {
  store.setFilter('asn', accept(values, validateAsn, Number))
}
function setProtos(values: unknown[]): void {
  store.setFilter('proto', accept(values, validateProto, Number))
}
function setSimple<K extends 'category' | 'direction' | 'exporter'>(key: K, values: Filters[K]): void {
  store.setFilter(key, values)
}

onMounted(async () => {
  if (shown.value.has('category')) {
    categories.load().catch((err: unknown) => notify.error(err, 'Loading categories'))
  }
  if (!shown.value.has('exporter')) return
  try {
    exporters.value = await fetchExporters()
  } catch (err) {
    notify.error(err, 'Loading exporters')
  }
})
</script>

<template>
  <div class="filter-bar" data-testid="filter-bar">
    <NSelect
      v-if="shown.has('host')"
      :value="[...filters.host]"
      :options="toOptions(filters.host)"
      multiple
      filterable
      tag
      clearable
      size="small"
      placeholder="Host IP"
      :show-arrow="false"
      data-testid="filter-host"
      @update:value="setHosts"
    />
    <NSelect
      v-if="shown.has('category')"
      :value="[...filters.category]"
      :options="categoryOptions"
      multiple
      filterable
      clearable
      size="small"
      placeholder="Category"
      max-tag-count="responsive"
      @update:value="(v: number[]) => setSimple('category', v)"
    />
    <NSelect
      v-if="shown.has('proto')"
      :value="[...filters.proto]"
      :options="protoOptions"
      multiple
      filterable
      tag
      clearable
      size="small"
      placeholder="Protocol"
      max-tag-count="responsive"
      data-testid="filter-proto"
      @update:value="setProtos"
    />
    <NSelect
      v-if="shown.has('direction')"
      :value="[...filters.direction]"
      :options="directionOptions"
      multiple
      clearable
      size="small"
      placeholder="Direction"
      max-tag-count="responsive"
      @update:value="(v: Direction[]) => setSimple('direction', v)"
    />
    <NSelect
      v-if="shown.has('country')"
      :value="[...filters.country]"
      :options="toOptions(filters.country)"
      multiple
      filterable
      tag
      clearable
      size="small"
      placeholder="Country (ISO2)"
      :show-arrow="false"
      @update:value="setCountries"
    />
    <NSelect
      v-if="shown.has('asn')"
      :value="[...filters.asn]"
      :options="toOptions(filters.asn)"
      multiple
      filterable
      tag
      clearable
      size="small"
      placeholder="ASN"
      :show-arrow="false"
      @update:value="setAsns"
    />
    <NSelect
      v-if="shown.has('exporter') && exporterOptions.length > 1"
      :value="[...filters.exporter]"
      :options="exporterOptions"
      multiple
      clearable
      size="small"
      placeholder="Exporter"
      @update:value="(v: number[]) => setSimple('exporter', v)"
    />
    <NButton v-if="activeCount > 0" size="small" quaternary @click="store.clear()">Clear filters</NButton>
  </div>
</template>

<style scoped>
.filter-bar {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: 6px;
  align-items: center;
}
</style>
