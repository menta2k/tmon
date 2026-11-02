<script setup lang="ts">
import { ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NAlert, NButton, NCard, NForm, NFormItem, NInput } from 'naive-ui'
import { ApiError } from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import { errorMessage } from '@/utils/errors'

const auth = useAuthStore()
const router = useRouter()
const route = useRoute()
const password = ref('')
const submitting = ref(false)
const error = ref<string | null>(null)

function loginError(err: unknown): string {
  if (err instanceof ApiError && err.isUnauthorized) return 'Wrong password'
  if (err instanceof ApiError && err.code === 'rate_limited')
    return 'Too many attempts. Wait a minute and try again.'
  return errorMessage(err)
}

function safeRedirect(): string {
  const r = route.query.redirect
  return typeof r === 'string' && r.startsWith('/') && !r.startsWith('//') ? r : '/'
}

async function submit(): Promise<void> {
  if (!password.value) {
    error.value = 'Enter the password'
    return
  }
  submitting.value = true
  error.value = null
  try {
    await auth.login(password.value)
    await router.replace(safeRedirect())
  } catch (err) {
    error.value = loginError(err)
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <div class="login">
    <NCard class="login__card" title="tmon" size="medium">
      <p class="muted">Network traffic monitor</p>
      <NForm @submit.prevent="submit">
        <NFormItem label="Password" :show-feedback="false">
          <NInput
            v-model:value="password"
            type="password"
            show-password-on="click"
            placeholder="Password"
            autofocus
            :input-props="{ autocomplete: 'current-password', name: 'password' }"
            data-testid="login-password"
          />
        </NFormItem>
        <NAlert v-if="error" type="error" :bordered="false" class="login__error" data-testid="login-error">
          {{ error }}
        </NAlert>
        <NButton
          type="primary"
          attr-type="submit"
          block
          :loading="submitting"
          class="login__submit"
          data-testid="login-submit"
        >
          Log in
        </NButton>
      </NForm>
    </NCard>
  </div>
</template>

<style scoped>
.login {
  min-height: 100vh;
  display: grid;
  place-items: center;
  padding: 16px;
}
.login__card {
  width: 100%;
  max-width: 360px;
}
.login__error {
  margin-top: 12px;
}
.login__submit {
  margin-top: 16px;
}
</style>
