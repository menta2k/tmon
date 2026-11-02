<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  NButton,
  NForm,
  NFormItem,
  NInput,
  NModal,
  NSelect,
  NSpace,
  NSwitch,
  NAlert,
  type SelectOption,
} from 'naive-ui'
import { DIRECTIONS, PROTOCOL_NAMES } from '@/constants'
import type { Rule, RuleInput } from '@/api/types'
import { createRule, updateRule } from '@/api/config'
import { useCategoriesStore } from '@/stores/categories'
import { useAppCatalogStore } from '@/stores/appCatalog'
import { useNotify } from '@/composables/useNotify'
import { directionLabel } from '@/utils/format'
import { fieldErrors } from '@/utils/errors'
import { validateRuleName } from '@/utils/validators'
import {
  RULE_MATCH_FIELDS,
  RULE_MATCH_LABELS,
  emptyRuleMatchForm,
  formToRuleMatch,
  ruleMatchToForm,
  validateRuleMatchField,
  validateRuleMatchForm,
  type RuleMatchField,
  type RuleMatchForm,
} from '@/utils/ruleMatch'

const props = defineProps<{ show: boolean; rule: Rule | null }>()
const emit = defineEmits<{ 'update:show': [value: boolean]; saved: [rule: Rule] }>()

const categories = useCategoriesStore()
const appCatalog = useAppCatalogStore()
const notify = useNotify()

const name = ref('')
const categoryId = ref<number | null>(null)
const enabled = ref(true)
const match = ref<RuleMatchForm>(emptyRuleMatchForm())
const serverErrors = ref<Readonly<Record<string, string>>>({})
const saving = ref(false)
const touched = ref(false)

const STATIC_OPTIONS: Partial<Record<RuleMatchField, SelectOption[]>> = {
  proto: Object.entries(PROTOCOL_NAMES).map(([n, label]) => ({ label: `${label} (${n})`, value: n })),
  direction: DIRECTIONS.map((d) => ({ label: directionLabel(d), value: d })),
}
const asOptions = (values: readonly string[]): SelectOption[] => values.map((v) => ({ label: v, value: v }))
/** Static presets plus the classifier catalog (search-as-you-type; free text still allowed). */
const PRESET_OPTIONS = computed<Partial<Record<RuleMatchField, SelectOption[]>>>(() => ({
  ...STATIC_OPTIONS,
  app: asOptions(appCatalog.apps),
  app_category: asOptions(appCatalog.categories),
}))
const PLACEHOLDERS: Readonly<Record<RuleMatchField, string>> = {
  proto: '6, 17 …',
  remote_port: '443 or 27015-27030',
  local_port: '22 or 8000-8100',
  remote_cidr: '203.0.113.0/24',
  local_host: '192.168.88.50 or aa:bb:cc:dd:ee:ff',
  asn: '2906',
  country: 'US',
  interface: '3',
  hostname: '*.googlevideo.com',
  direction: 'down / up',
  app: 'BitTorrent',
  app_category: 'VPN',
}

watch(
  () => [props.show, props.rule] as const,
  ([show, rule]) => {
    if (!show) return
    void appCatalog.load()
    name.value = rule?.name ?? ''
    categoryId.value = rule?.category_id ?? null
    enabled.value = rule?.enabled ?? true
    match.value = rule ? ruleMatchToForm(rule.match) : emptyRuleMatchForm()
    serverErrors.value = {}
    touched.value = false
  },
  { immediate: true },
)

const categoryOptions = computed(() => categories.items.map((c) => ({ label: c.name, value: c.id })))
const nameError = computed(() => (touched.value || name.value ? validateRuleName(name.value) : null))
const categoryError = computed(() =>
  touched.value && categoryId.value === null ? 'Choose a category' : null,
)
const matchErrors = computed(() => validateRuleMatchForm(match.value))

function fieldError(field: RuleMatchField): string | null {
  return validateRuleMatchField(field, match.value[field]) ?? serverErrors.value[`match.${field}`] ?? null
}

function optionsFor(field: RuleMatchField): SelectOption[] {
  const preset = PRESET_OPTIONS.value[field] ?? []
  const extra = match.value[field]
    .filter((v) => !preset.some((o) => o.value === v))
    .map((v) => ({ label: v, value: v }))
  return [...preset, ...extra]
}

function setField(field: RuleMatchField, values: (string | number)[]): void {
  match.value = { ...match.value, [field]: values.map((v) => String(v).trim()).filter((v) => v !== '') }
}

function isValid(): boolean {
  touched.value = true
  return (
    !validateRuleName(name.value) && categoryId.value !== null && Object.keys(matchErrors.value).length === 0
  )
}

async function save(): Promise<void> {
  if (!isValid() || categoryId.value === null) return
  const input: RuleInput = {
    name: name.value.trim(),
    category_id: categoryId.value,
    enabled: enabled.value,
    match: formToRuleMatch(match.value),
  }
  saving.value = true
  try {
    const saved = props.rule ? await updateRule(props.rule.id, input) : await createRule(input)
    notify.success(props.rule ? 'Rule updated' : 'Rule created')
    emit('saved', saved)
    emit('update:show', false)
  } catch (err) {
    serverErrors.value = fieldErrors(err)
    notify.error(err, 'Saving rule')
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <NModal
    :show="show"
    preset="card"
    :title="rule ? `Edit rule: ${rule.name}` : 'New rule'"
    class="rule-editor"
    :style="{ maxWidth: '720px', width: '96vw' }"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="top" size="small" @submit.prevent="save">
      <div class="grid-2">
        <NFormItem
          label="Name"
          :feedback="nameError ?? serverErrors.name ?? ''"
          :validation-status="nameError ? 'error' : undefined"
        >
          <NInput
            v-model:value="name"
            placeholder="e.g. Steam games"
            maxlength="80"
            data-testid="rule-name"
          />
        </NFormItem>
        <NFormItem
          label="Category"
          :feedback="categoryError ?? ''"
          :validation-status="categoryError ? 'error' : undefined"
        >
          <NSelect
            v-model:value="categoryId"
            :options="categoryOptions"
            filterable
            data-testid="rule-category"
          />
        </NFormItem>
      </div>
      <NFormItem label="Enabled" :show-feedback="false">
        <NSwitch v-model:value="enabled" />
      </NFormItem>
      <NAlert v-if="matchErrors._ && touched" type="warning" :bordered="false" class="rule-editor__hint">
        {{ matchErrors._ }}
      </NAlert>
      <div class="grid-2 rule-editor__match">
        <NFormItem
          v-for="field in RULE_MATCH_FIELDS"
          :key="field"
          :label="RULE_MATCH_LABELS[field]"
          :feedback="fieldError(field) ?? ''"
          :validation-status="fieldError(field) ? 'error' : undefined"
        >
          <NSelect
            :value="[...match[field]]"
            :options="optionsFor(field)"
            multiple
            filterable
            :tag="field !== 'direction'"
            clearable
            :show-arrow="(PRESET_OPTIONS[field]?.length ?? 0) > 0"
            :placeholder="PLACEHOLDERS[field]"
            :data-testid="`rule-match-${field}`"
            @update:value="(v: (string | number)[]) => setField(field, v)"
          />
        </NFormItem>
      </div>
      <NSpace justify="end">
        <NButton @click="emit('update:show', false)">Cancel</NButton>
        <NButton type="primary" attr-type="submit" :loading="saving" data-testid="rule-save">Save</NButton>
      </NSpace>
    </NForm>
  </NModal>
</template>

<style scoped>
.rule-editor__hint {
  margin-bottom: 8px;
}
.rule-editor__match {
  gap: 0 12px;
}
</style>
