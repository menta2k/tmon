<script setup lang="ts">
import { onUnmounted, ref } from 'vue'
import { NButton, NPopconfirm, NProgress } from 'naive-ui'
import { JOB_POLL_MS } from '@/constants'
import type { Job, JobId } from '@/api/types'
import { fetchJob, reapplyRules } from '@/api/config'
import { useNotify } from '@/composables/useNotify'

const emit = defineEmits<{ done: [] }>()
const notify = useNotify()
const job = ref<Job | null>(null)
const starting = ref(false)
let timer: ReturnType<typeof setTimeout> | null = null

function finish(final: Job): void {
  if (final.state === 'done') {
    notify.success('Re-categorisation finished')
    emit('done')
  } else {
    notify.error(new Error(final.error ?? 'The re-apply job failed'))
  }
}

async function poll(id: JobId): Promise<void> {
  try {
    const next = await fetchJob(id)
    job.value = next
    if (next.state === 'done' || next.state === 'failed') {
      finish(next)
      return
    }
    timer = setTimeout(() => void poll(id), JOB_POLL_MS)
  } catch (err) {
    job.value = null
    notify.error(err, 'Checking job progress')
  }
}

async function start(): Promise<void> {
  starting.value = true
  try {
    const { job_id } = await reapplyRules()
    job.value = { state: 'queued', progress: 0 }
    void poll(job_id)
  } catch (err) {
    notify.error(err, 'Starting re-apply')
  } finally {
    starting.value = false
  }
}

const running = (): boolean => job.value?.state === 'queued' || job.value?.state === 'running'

onUnmounted(() => {
  if (timer) clearTimeout(timer)
})
</script>

<template>
  <span class="reapply">
    <NPopconfirm @positive-click="start">
      <template #trigger>
        <NButton size="small" :loading="starting" :disabled="running()" data-testid="rules-reapply">
          Re-apply to history
        </NButton>
      </template>
      Re-categorise all raw flows still within retention and rebuild summaries?
    </NPopconfirm>
    <NProgress
      v-if="job && running()"
      type="line"
      :percentage="Math.round(job.progress * 100)"
      :show-indicator="true"
      class="reapply__progress"
      data-testid="reapply-progress"
    />
    <span v-else-if="job" class="muted" data-testid="reapply-state">{{ job.state }}</span>
  </span>
</template>

<style scoped>
.reapply {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}
.reapply__progress {
  width: 140px;
}
</style>
