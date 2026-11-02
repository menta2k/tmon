<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { NSelect, type SelectOption } from 'naive-ui'
import { useAppCatalogStore } from '@/stores/appCatalog'
import { useNotify } from '@/composables/useNotify'
import { validateAppName } from '@/utils/validators'
import { APP_UNCLASSIFIED_LABEL } from '@/constants'

/**
 * Multi-select of classifier application names: catalog suggestions plus free
 * text. With `allowUnclassified`, an "Unclassified" choice (value `''`, i.e.
 * flows without an app) is offered first.
 */
interface Props {
  value: readonly string[]
  placeholder?: string
  allowUnclassified?: boolean
}
const props = withDefaults(defineProps<Props>(), {
  placeholder: 'Application',
  allowUnclassified: false,
})
const emit = defineEmits<{ 'update:value': [value: string[]] }>()

const catalog = useAppCatalogStore()
const notify = useNotify()

const options = computed<SelectOption[]>(() => {
  const extra = props.value.filter((v) => v !== '' && !catalog.apps.includes(v))
  const names = [...catalog.apps, ...extra].map((v) => ({ label: v, value: v }))
  return props.allowUnclassified ? [{ label: APP_UNCLASSIFIED_LABEL, value: '' }, ...names] : names
})

/** Emits only valid names; invalid entries are dropped with a toast. */
function update(raw: unknown[]): void {
  const valid: string[] = []
  for (const value of raw) {
    const text = String(value).trim()
    if (text === '' && props.allowUnclassified) {
      if (!valid.includes(text)) valid.push(text)
      continue
    }
    const err = validateAppName(text)
    if (err) notify.error(new Error(err))
    else if (!valid.includes(text)) valid.push(text)
  }
  emit('update:value', valid)
}

onMounted(() => void catalog.load())
</script>

<template>
  <NSelect
    :value="[...value]"
    :options="options"
    multiple
    filterable
    tag
    clearable
    size="small"
    :placeholder="placeholder"
    max-tag-count="responsive"
    :title="catalog.error ?? undefined"
    @update:value="update"
  />
</template>
