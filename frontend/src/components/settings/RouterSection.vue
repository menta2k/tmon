<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NAlert, NButton, NForm, NFormItem, NInput, NInputNumber, NSelect, NSpace, NSwitch } from 'naive-ui'
import {
  HOST_NAME_MAX,
  POLL_SECS_MAX,
  POLL_SECS_MIN,
  ROUTER_KINDS,
  ROUTER_PLACEHOLDERS,
  type RouterKind,
} from '@/constants'
import type { RouterSettingsPatch, RouterTestResult, Settings } from '@/api/types'
import { testRouterConnection, updateSettings } from '@/api/config'
import { useNotify } from '@/composables/useNotify'
import { errorMessage } from '@/utils/errors'
import { validateHttpUrl, validateName, validateUintInRange } from '@/utils/validators'

const props = defineProps<{ settings: Settings }>()
const emit = defineEmits<{ saved: [settings: Settings] }>()
const notify = useNotify()

interface Form {
  kind: RouterKind
  enabled: boolean
  url: string
  username: string
  password: string
  verify_tls: boolean
  lease_poll_secs: number | null
  dns_poll_secs: number | null
}
const form = ref<Form>({
  kind: 'routeros',
  enabled: false,
  url: '',
  username: '',
  password: '',
  verify_tls: true,
  lease_poll_secs: null,
  dns_poll_secs: null,
})
const saving = ref(false)
const testing = ref(false)
const testResult = ref<RouterTestResult | null>(null)
const testError = ref<string | null>(null)

watch(
  () => props.settings.router,
  (r) => {
    form.value = { ...r, kind: r.kind ?? 'routeros', password: '' }
  },
  { immediate: true },
)

const pollErr = (v: number | null, what: string): string | null =>
  v === null ? 'Required' : validateUintInRange(v, POLL_SECS_MIN, POLL_SECS_MAX, what)

const errors = computed(() => {
  const f = form.value
  const check = f.enabled
  const out: Record<string, string | null> = {
    url: check || f.url ? validateHttpUrl(f.url) : null,
    username: check ? validateName(f.username, HOST_NAME_MAX, 'User') : null,
    lease_poll_secs: pollErr(f.lease_poll_secs, 'Lease poll interval'),
    dns_poll_secs: pollErr(f.dns_poll_secs, 'DNS poll interval'),
  }
  return out
})
const isOpenWrt = computed(() => form.value.kind === 'openwrt')
const placeholders = computed(() => ROUTER_PLACEHOLDERS[form.value.kind])

const hasErrors = computed(() => Object.values(errors.value).some((e) => e !== null))

function set<K extends keyof Form>(key: K, value: Form[K]): void {
  form.value = { ...form.value, [key]: value }
}

/** Password is write-only: only sent when the user typed a new one. */
function toPatch(f: Form): RouterSettingsPatch {
  return {
    kind: f.kind,
    enabled: f.enabled,
    url: f.url.trim(),
    username: f.username.trim(),
    verify_tls: f.verify_tls,
    lease_poll_secs: f.lease_poll_secs ?? undefined,
    dns_poll_secs: f.dns_poll_secs ?? undefined,
    ...(f.password ? { password: f.password } : {}),
  }
}

async function save(): Promise<void> {
  if (hasErrors.value) return
  saving.value = true
  try {
    const saved = await updateSettings({ router: toPatch(form.value) })
    form.value = { ...form.value, password: '' }
    notify.success('Router integration saved')
    emit('saved', saved)
  } catch (err) {
    notify.error(err, 'Saving router integration')
  } finally {
    saving.value = false
  }
}

async function test(): Promise<void> {
  if (errors.value.url) return
  testing.value = true
  testResult.value = null
  testError.value = null
  try {
    testResult.value = await testRouterConnection(toPatch(form.value))
  } catch (err) {
    testError.value = errorMessage(err, 'Connection test')
  } finally {
    testing.value = false
  }
}
</script>

<template>
  <NForm label-placement="top" size="small" data-testid="router-section" @submit.prevent="save">
    <p class="muted">Optional read-only access to the router to learn DHCP host names (and, on RouterOS, DNS cache entries).</p>
    <NFormItem label="Router type" :show-feedback="false">
      <NSelect
        :value="form.kind"
        :options="[...ROUTER_KINDS]"
        data-testid="router-kind"
        @update:value="(v: RouterKind) => set('kind', v)"
      />
    </NFormItem>
    <p v-if="isOpenWrt" class="muted" data-testid="router-openwrt-hint">
      OpenWrt: tmon logs in to <code>/ubus</code> and calls <code>luci-rpc getDHCPLeases</code> only. Grant
      the rpcd user just that method. OpenWrt has no DNS cache API; remote names come from packet capture.
    </p>
    <NFormItem label="Enabled" :show-feedback="false">
      <NSwitch :value="form.enabled" @update:value="(v: boolean) => set('enabled', v)" />
    </NFormItem>
    <div class="grid-2">
      <NFormItem
        label="Router URL"
        :feedback="errors.url ?? ''"
        :validation-status="errors.url ? 'error' : undefined"
      >
        <NInput
          :value="form.url"
          :placeholder="placeholders.url"
          @update:value="(v: string) => set('url', v)"
        />
      </NFormItem>
      <NFormItem
        label="User"
        :feedback="errors.username ?? ''"
        :validation-status="errors.username ? 'error' : undefined"
      >
        <NInput
          :value="form.username"
          :placeholder="placeholders.user"
          @update:value="(v: string) => set('username', v)"
        />
      </NFormItem>
      <NFormItem label="Password (write-only)">
        <NInput
          type="password"
          show-password-on="click"
          :value="form.password"
          :placeholder="settings.router.password_set ? '•••••• (unchanged)' : 'not set'"
          :input-props="{ autocomplete: 'new-password' }"
          @update:value="(v: string) => set('password', v)"
        />
      </NFormItem>
      <NFormItem label="Verify TLS certificate" :show-feedback="false">
        <NSwitch :value="form.verify_tls" @update:value="(v: boolean) => set('verify_tls', v)" />
      </NFormItem>
      <NFormItem
        label="DHCP lease poll (s)"
        :feedback="errors.lease_poll_secs ?? ''"
        :validation-status="errors.lease_poll_secs ? 'error' : undefined"
      >
        <NInputNumber
          :value="form.lease_poll_secs"
          :min="POLL_SECS_MIN"
          :max="POLL_SECS_MAX"
          :precision="0"
          @update:value="(v: number | null) => set('lease_poll_secs', v)"
        />
      </NFormItem>
      <NFormItem
        v-if="!isOpenWrt"
        label="DNS cache poll (s)"
        :feedback="errors.dns_poll_secs ?? ''"
        :validation-status="errors.dns_poll_secs ? 'error' : undefined"
      >
        <NInputNumber
          :value="form.dns_poll_secs"
          :min="POLL_SECS_MIN"
          :max="POLL_SECS_MAX"
          :precision="0"
          @update:value="(v: number | null) => set('dns_poll_secs', v)"
        />
      </NFormItem>
    </div>
    <NAlert
      v-if="testResult"
      :type="testResult.reachable ? 'success' : 'error'"
      :bordered="false"
      class="router__result"
    >
      {{
        testResult.reachable
          ? 'Router reachable, credentials accepted'
          : (testResult.error ?? 'Router not reachable')
      }}
    </NAlert>
    <NAlert v-if="testError" type="error" :bordered="false" class="router__result">{{ testError }}</NAlert>
    <NSpace>
      <NButton size="small" :loading="testing" :disabled="!!errors.url" @click="test"
        >Test connection</NButton
      >
      <NButton size="small" type="primary" attr-type="submit" :loading="saving" :disabled="hasErrors"
        >Save router settings</NButton
      >
    </NSpace>
  </NForm>
</template>

<style scoped>
.router__result {
  margin-bottom: 8px;
}
</style>
