<script setup lang="ts">
import { computed, ref } from 'vue'
import { NButton, NCheckbox, NForm, NFormItem, NInput, NSelect, NAlert, NTag } from 'naive-ui'
import { CONFIDENCE_INFO, DIRECTIONS } from '@/constants'
import type { Direction, Rule, RuleTestFlow, RuleTestResult } from '@/api/types'
import { testRule } from '@/api/config'
import { useNotify } from '@/composables/useNotify'
import { directionLabel } from '@/utils/format'
import {
  validateAppName,
  validateAsn,
  validateIp,
  validateIso2,
  validatePort,
  validateProto,
  type ValidationResult,
} from '@/utils/validators'
import CategoryChip from './CategoryChip.vue'

const props = defineProps<{ rules: readonly Rule[] }>()
const notify = useNotify()

interface TestForm {
  proto: string
  local_ip: string
  local_port: string
  remote_ip: string
  remote_port: string
  country: string
  asn: string
  remote_host: string
  direction: Direction | null
  /** Classifier fields (feature 003). */
  app: string
  app_category: string
  app_guessed: boolean
}

const form = ref<TestForm>({
  proto: '',
  local_ip: '',
  local_port: '',
  remote_ip: '',
  remote_port: '',
  country: '',
  asn: '',
  remote_host: '',
  direction: null,
  app: '',
  app_category: '',
  app_guessed: false,
})
const result = ref<RuleTestResult | null>(null)
const running = ref(false)

const VALIDATORS: Partial<Record<keyof TestForm, (v: string) => ValidationResult>> = {
  proto: validateProto,
  local_ip: validateIp,
  local_port: validatePort,
  remote_ip: validateIp,
  remote_port: validatePort,
  country: validateIso2,
  asn: validateAsn,
  app: validateAppName,
  app_category: validateAppName,
}

const errors = computed<Record<string, string>>(() => {
  const out: Record<string, string> = {}
  for (const [key, validate] of Object.entries(VALIDATORS)) {
    const value = String(form.value[key as keyof TestForm] ?? '').trim()
    const err = value ? validate(value) : null
    if (err) out[key] = err
  }
  return out
})

const confidence = computed(() =>
  result.value?.confidence ? CONFIDENCE_INFO[result.value.confidence] : null,
)
const matchedRule = computed(() => props.rules.find((r) => r.id === result.value?.rule_id) ?? null)
const directionOptions = DIRECTIONS.map((d) => ({ label: directionLabel(d), value: d }))

function set<K extends keyof TestForm>(key: K, value: TestForm[K]): void {
  form.value = { ...form.value, [key]: value }
}

/** Classifier fields; `app_confident` is only sent alongside an app or app category. */
function appFields(f: TestForm): RuleTestFlow {
  const app = f.app.trim()
  const category = f.app_category.trim()
  if (!app && !category) return {}
  return {
    ...(app ? { app } : {}),
    ...(category ? { app_category: category } : {}),
    app_confident: !f.app_guessed,
  }
}

function toFlow(f: TestForm): RuleTestFlow {
  const num = (v: string): number | undefined => (v.trim() ? Number(v) : undefined)
  const str = (v: string): string | undefined => v.trim() || undefined
  const flow: RuleTestFlow = {
    proto: num(f.proto),
    local_ip: str(f.local_ip),
    local_port: num(f.local_port),
    remote_ip: str(f.remote_ip),
    remote_port: num(f.remote_port),
    country: str(f.country)?.toUpperCase(),
    asn: num(f.asn),
    remote_host: str(f.remote_host),
    direction: f.direction ?? undefined,
  }
  const defined = Object.fromEntries(Object.entries(flow).filter(([, v]) => v !== undefined))
  return { ...defined, ...appFields(f) } as RuleTestFlow
}

async function run(): Promise<void> {
  if (Object.keys(errors.value).length > 0) {
    notify.error(new Error(Object.values(errors.value)[0]))
    return
  }
  running.value = true
  try {
    result.value = await testRule(toFlow(form.value))
  } catch (err) {
    notify.error(err, 'Rule test')
  } finally {
    running.value = false
  }
}
</script>

<template>
  <div class="rule-test" data-testid="rule-test-panel">
    <p class="muted">Dry-run a flow against the saved rule set.</p>
    <NForm label-placement="top" size="small" @submit.prevent="run">
      <div class="grid-3">
        <NFormItem
          v-for="key in [
            'proto',
            'local_ip',
            'local_port',
            'remote_ip',
            'remote_port',
            'country',
            'asn',
            'remote_host',
          ] as const"
          :key="key"
          :label="key"
          :feedback="errors[key] ?? ''"
          :validation-status="errors[key] ? 'error' : undefined"
        >
          <NInput :value="form[key]" clearable @update:value="(v: string) => set(key, v ?? '')" />
        </NFormItem>
        <NFormItem label="direction">
          <NSelect
            :value="form.direction"
            :options="directionOptions"
            clearable
            @update:value="(v: Direction | null) => set('direction', v)"
          />
        </NFormItem>
        <NFormItem
          v-for="key in ['app', 'app_category'] as const"
          :key="key"
          :label="key"
          :feedback="errors[key] ?? ''"
          :validation-status="errors[key] ? 'error' : undefined"
        >
          <NInput
            :value="form[key]"
            clearable
            :placeholder="key === 'app' ? 'BitTorrent' : 'Download'"
            :data-testid="key === 'app' ? 'rule-test-app' : 'rule-test-app-category'"
            @update:value="(v: string) => set(key, v ?? '')"
          />
        </NFormItem>
        <NFormItem label="app confidence">
          <NCheckbox
            :checked="form.app_guessed"
            data-testid="rule-test-app-guessed"
            @update:checked="(v: boolean) => set('app_guessed', v)"
          >
            guessed by classifier
          </NCheckbox>
        </NFormItem>
      </div>
      <NButton type="primary" attr-type="submit" :loading="running" size="small">Test</NButton>
    </NForm>
    <NAlert
      v-if="result"
      type="info"
      :bordered="false"
      class="rule-test__result"
      data-testid="rule-test-result"
    >
      Category: <CategoryChip :id="result.category_id" />
      <span v-if="result.rule_id !== null"> · rule: {{ matchedRule?.name ?? `#${result.rule_id}` }}</span>
      <span v-else> · no rule matched (fallback)</span>
      <template v-if="confidence">
        ·
        <NTag
          size="small"
          :type="confidence.type"
          :bordered="false"
          :title="confidence.hint"
          data-testid="rule-test-confidence"
          >{{ confidence.label }}</NTag
        >
      </template>
    </NAlert>
  </div>
</template>

<style scoped>
.rule-test__result {
  margin-top: 10px;
}
</style>
