<script setup lang="ts">
import { computed } from 'vue'
import { NAlert, NCard, NDescriptions, NDescriptionsItem, NTag } from 'naive-ui'
import { CAPTURE_NAMED_SHARE_WARN, CAPTURE_SETUP_GUIDE } from '@/constants'
import type { CaptureEnabled, CaptureStatus } from '@/api/types'
import { formatBytes, formatNumber } from '@/utils/format'
import { formatAgo } from '@/utils/time'
import StatTile from '@/components/StatTile.vue'
import GuideLink from '@/components/GuideLink.vue'

const props = defineProps<{ capture?: CaptureStatus }>()

const c = computed<CaptureEnabled | null>(() => (props.capture?.enabled ? props.capture : null))

const state = computed(() => {
  if (!c.value) return { label: 'disabled', type: 'default' as const }
  return c.value.listening
    ? { label: 'listening', type: 'success' as const }
    : { label: 'not listening', type: 'error' as const }
})

const sum = (o: Record<string, number>): number => Object.values(o).reduce((a, b) => a + b, 0)

const share = computed(() => {
  const v = c.value?.named_share_15m
  return v == null ? '—' : `${Math.round(v * 100)}%`
})
const shareAccent = computed(() => {
  const v = c.value?.named_share_15m
  return v != null && v < CAPTURE_NAMED_SHARE_WARN ? 'var(--warning-color, #f0a020)' : undefined
})

const names = computed(() => {
  const n = c.value?.names
  return n
    ? [
        { key: 'tls', label: 'TLS', value: n.tls },
        { key: 'quic', label: 'QUIC', value: n.quic },
        { key: 'http', label: 'HTTP', value: n.http },
        { key: 'dns', label: 'DNS', value: n.dns },
        { key: 'ech', label: 'ECH (outer name only)', value: n.ech },
      ]
    : []
})
</script>

<template>
  <NCard size="small" title="Packet capture" data-testid="capture-card">
    <template #header-extra>
      <NTag size="small" :type="state.type" data-testid="capture-state">{{ state.label }}</NTag>
    </template>

    <div v-if="!c" class="muted">
      Connection naming from router packet capture is off. Add a <code>[capture]</code> section to the
      config to enable it.
      <div><GuideLink :guide="CAPTURE_SETUP_GUIDE" /></div>
    </div>

    <template v-else>
      <NAlert v-if="c.error" type="error" :bordered="false">{{ c.error }}</NAlert>
      <div class="grid-3 tiles">
        <StatTile
          label="Datagrams"
          :value="formatNumber(c.datagrams.received)"
          :hint="`last ${formatAgo(c.last_datagram_at)} · max ${formatNumber(c.datagrams.max_len_15m)} B`"
          data-testid="capture-datagrams"
        />
        <StatTile
          label="Flows named (15 min)"
          :value="share"
          :accent="shareAccent"
          data-testid="capture-named-share"
        />
        <StatTile
          label="Tracked"
          :value="formatNumber(c.connections.tracked)"
          :hint="`${formatNumber(c.bindings.tracked)} DNS bindings · ${formatBytes(c.connections.reassembly_bytes)} buffered`"
        />
      </div>
      <NDescriptions :column="2" size="small" label-placement="left">
        <NDescriptionsItem label="Address">
          <span class="mono">{{ c.address }}</span>
        </NDescriptionsItem>
        <NDescriptionsItem v-for="n in names" :key="n.key" :label="n.label">
          <span :data-testid="`capture-names-${n.key}`">{{ formatNumber(n.value) }}</span>
        </NDescriptionsItem>
        <NDescriptionsItem label="Errors">
          <span data-testid="capture-errors">{{ formatNumber(sum(c.errors)) }}</span>
        </NDescriptionsItem>
        <NDescriptionsItem label="Evicted">
          <span data-testid="capture-evicted">{{
            formatNumber(c.connections.evicted + c.bindings.evicted)
          }}</span>
        </NDescriptionsItem>
        <NDescriptionsItem label="Dropped / rejected">
          {{ formatNumber(c.datagrams.dropped_backpressure) }} / {{ formatNumber(c.datagrams.rejected_source) }}
        </NDescriptionsItem>
        <NDescriptionsItem label="Unsupported / malformed">
          {{ formatNumber(c.datagrams.unsupported) }} / {{ formatNumber(c.datagrams.malformed) }}
        </NDescriptionsItem>
      </NDescriptions>
    </template>
  </NCard>
</template>

<style scoped>
.tiles {
  margin-bottom: 12px;
}
</style>
