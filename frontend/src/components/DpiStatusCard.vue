<script setup lang="ts">
import { computed } from 'vue'
import { NAlert, NCard, NDescriptions, NDescriptionsItem, NTag } from 'naive-ui'
import {
  DPI_DROP_SHARE_WARN,
  DPI_STATE_INFO,
  DPI_TROUBLESHOOTING_GUIDE,
  WARN_ACCENT_COLOR,
} from '@/constants'
import type { DpiEnabled, DpiStatus } from '@/api/types'
import { formatBytes, formatNumber, formatPercent } from '@/utils/format'
import { formatUptime } from '@/utils/time'
import StatTile from '@/components/StatTile.vue'
import GuideLink from '@/components/GuideLink.vue'

const props = defineProps<{ dpi?: DpiStatus }>()

const d = computed<DpiEnabled | null>(() => (props.dpi?.enabled ? props.dpi : null))

const state = computed(() => DPI_STATE_INFO[d.value?.state ?? 'disabled'])

const engine = computed(() => {
  const v = d.value
  if (!v?.engine) return '—'
  return v.impl_version ? `${v.engine} · tmon-dpi ${v.impl_version}` : v.engine
})

const classified = computed(() => {
  const c = d.value?.classifier
  const total = c ? c.classified_confident + c.classified_guess : 0
  return c && total > 0
    ? {
        value: formatPercent(c.classified_confident / total, 0),
        hint: `${formatNumber(total)} flows classified`,
      }
    : { value: '—', hint: 'no flows classified yet' }
})

const dropsWarn = computed(() => (d.value?.queue.drop_share_15m ?? 0) > DPI_DROP_SHARE_WARN)
</script>

<template>
  <NCard size="small" title="Application classification" data-testid="dpi-card">
    <template #header-extra>
      <NTag size="small" :type="state.type" data-testid="dpi-state">{{ state.label }}</NTag>
    </template>

    <div v-if="!d" class="muted">
      Application classification (nDPI) is off. Add a <code>[dpi]</code> section to the config to enable it.
      <div><GuideLink :guide="DPI_TROUBLESHOOTING_GUIDE" /></div>
    </div>

    <template v-else>
      <NAlert v-if="d.error" type="error" :bordered="false" class="dpi-error">
        {{ d.error }}
        <div><GuideLink :guide="DPI_TROUBLESHOOTING_GUIDE" /></div>
      </NAlert>
      <div class="grid-3 tiles">
        <StatTile
          label="Packets inspected"
          :value="formatNumber(d.classifier.packets_in)"
          :hint="`${formatNumber(d.classifier.flows_active)} active flows`"
          data-testid="dpi-packets"
        />
        <StatTile
          label="Confident"
          :value="classified.value"
          :hint="classified.hint"
          data-testid="dpi-classified"
        />
        <StatTile
          label="Dropped (15 min)"
          :value="formatPercent(d.queue.drop_share_15m)"
          :hint="`${formatNumber(d.queue.dropped_queue + d.queue.dropped_down)} packets`"
          :accent="dropsWarn ? WARN_ACCENT_COLOR : undefined"
          :data-warn="String(dropsWarn)"
          data-testid="dpi-drops"
        />
      </div>
      <NDescriptions :column="2" size="small" label-placement="left">
        <NDescriptionsItem label="Engine">
          <span data-testid="dpi-engine">{{ engine }}</span>
        </NDescriptionsItem>
        <NDescriptionsItem label="Uptime">
          <span data-testid="dpi-uptime">{{ formatUptime(d.uptime_secs) }}</span>
        </NDescriptionsItem>
        <NDescriptionsItem label="Restarts (10 min / total)">
          <span data-testid="dpi-restarts"
            >{{ formatNumber(d.restarts_10m) }} / {{ formatNumber(d.restarts_total) }}</span
          >
        </NDescriptionsItem>
        <NDescriptionsItem label="Memory">
          {{ formatBytes(d.classifier.mem_bytes) }} of {{ formatBytes(d.classifier.mem_limit_bytes) }}
        </NDescriptionsItem>
        <NDescriptionsItem label="Evicted">{{ formatNumber(d.classifier.evicted) }}</NDescriptionsItem>
        <NDescriptionsItem label="Invalid (IPC / app)">
          {{ formatNumber(d.ipc_invalid) }} / {{ formatNumber(d.app_invalid) }}
        </NDescriptionsItem>
      </NDescriptions>
      <div class="top-apps" data-testid="dpi-top-apps">
        <span class="muted">Top applications:</span>
        <template v-if="d.top_apps.length > 0">
          <NTag v-for="a in d.top_apps" :key="a.app" size="small" :bordered="false">
            {{ a.app }} · {{ formatNumber(a.flows) }}
          </NTag>
        </template>
        <span v-else class="muted">No applications classified yet</span>
      </div>
    </template>
  </NCard>
</template>

<style scoped>
.tiles,
.dpi-error {
  margin-bottom: 12px;
}
.top-apps {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  align-items: center;
  margin-top: 8px;
}
</style>
