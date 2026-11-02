<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NButton, NColorPicker, NForm, NFormItem, NInput, NModal, NPopconfirm, NSpace } from 'naive-ui'
import { CATEGORY_DESCRIPTION_MAX, CATEGORY_NAME_MAX, FALLBACK_COLOR, OTHER_CATEGORY_ID } from '@/constants'
import type { Category, CategoryInput } from '@/api/types'
import { createCategory, deleteCategory, updateCategory } from '@/api/config'
import { useNotify } from '@/composables/useNotify'
import { fieldErrors } from '@/utils/errors'
import { validateCategoryName, validateColor, validateDescription } from '@/utils/validators'

const props = defineProps<{ show: boolean; category: Category | null }>()
const emit = defineEmits<{ 'update:show': [value: boolean]; saved: []; deleted: [] }>()
const notify = useNotify()

const form = ref<CategoryInput>({ name: '', color: FALLBACK_COLOR, description: '' })
const touched = ref(false)
const saving = ref(false)
const serverErrors = ref<Readonly<Record<string, string>>>({})

watch(
  () => [props.show, props.category] as const,
  ([show, c]) => {
    if (!show) return
    form.value = { name: c?.name ?? '', color: c?.color ?? FALLBACK_COLOR, description: c?.description ?? '' }
    touched.value = false
    serverErrors.value = {}
  },
  { immediate: true },
)

const errors = computed(() => ({
  name: touched.value || form.value.name ? validateCategoryName(form.value.name) : null,
  color: validateColor(form.value.color),
  description: validateDescription(form.value.description),
}))

const canDelete = computed(
  () => props.category !== null && !props.category.builtin && props.category.id !== OTHER_CATEGORY_ID,
)

function set<K extends keyof CategoryInput>(key: K, value: CategoryInput[K]): void {
  form.value = { ...form.value, [key]: value }
}

async function save(): Promise<void> {
  touched.value = true
  if (validateCategoryName(form.value.name) || errors.value.color || errors.value.description) return
  const input: CategoryInput = {
    ...form.value,
    name: form.value.name.trim(),
    color: form.value.color.toLowerCase(),
  }
  saving.value = true
  try {
    if (props.category) await updateCategory(props.category.id, input)
    else await createCategory(input)
    notify.success('Category saved')
    emit('saved')
    emit('update:show', false)
  } catch (err) {
    serverErrors.value = fieldErrors(err)
    notify.error(err, 'Saving category')
  } finally {
    saving.value = false
  }
}

async function remove(): Promise<void> {
  if (!props.category || !canDelete.value) return
  const ok = await notify.run(
    () => deleteCategory(props.category!.id),
    'Category deleted',
    'Deleting category',
  )
  if (!ok) return
  emit('deleted')
  emit('update:show', false)
}
</script>

<template>
  <NModal
    :show="show"
    preset="card"
    :title="category ? `Edit category: ${category.name}` : 'New category'"
    :style="{ maxWidth: '480px', width: '96vw' }"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="top" size="small" @submit.prevent="save">
      <NFormItem
        label="Name"
        :feedback="errors.name ?? serverErrors.name ?? ''"
        :validation-status="errors.name ? 'error' : undefined"
      >
        <NInput
          :value="form.name"
          :maxlength="CATEGORY_NAME_MAX"
          data-testid="category-name"
          @update:value="(v: string) => set('name', v)"
        />
      </NFormItem>
      <NFormItem
        label="Colour"
        :feedback="errors.color ?? ''"
        :validation-status="errors.color ? 'error' : undefined"
      >
        <NColorPicker
          :value="form.color"
          :modes="['hex']"
          :show-alpha="false"
          :swatches="['#3b82f6', '#10b981', '#f59e0b', '#ef4444', '#8b5cf6', '#06b6d4', '#ec4899', '#84cc16']"
          @update:value="(v: string) => set('color', v)"
        />
      </NFormItem>
      <NFormItem
        label="Description"
        :feedback="errors.description ?? ''"
        :validation-status="errors.description ? 'error' : undefined"
      >
        <NInput
          type="textarea"
          :value="form.description"
          :maxlength="CATEGORY_DESCRIPTION_MAX"
          show-count
          :autosize="{ minRows: 2, maxRows: 4 }"
          @update:value="(v: string) => set('description', v)"
        />
      </NFormItem>
      <NSpace justify="space-between">
        <NPopconfirm v-if="category" :disabled="!canDelete" @positive-click="remove">
          <template #trigger>
            <NButton type="error" ghost :disabled="!canDelete" data-testid="category-delete">Delete</NButton>
          </template>
          Delete this category? Rules pointing to it must be changed first.
        </NPopconfirm>
        <span v-else />
        <NSpace>
          <NButton @click="emit('update:show', false)">Cancel</NButton>
          <NButton type="primary" attr-type="submit" :loading="saving" data-testid="category-save"
            >Save</NButton
          >
        </NSpace>
      </NSpace>
    </NForm>
  </NModal>
</template>
