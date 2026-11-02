<script setup lang="ts">
import { computed, h, ref, watch } from 'vue'
import {
  NAlert,
  NButton,
  NCard,
  NDataTable,
  NDescriptions,
  NDescriptionsItem,
  NProgress,
  NTag,
  type DataTableColumns,
} from 'naive-ui'
import { STATUS_VIEW_REFRESH_MS, STORAGE_ERROR_RATIO, STORAGE_WARN_RATIO } from '@/constants'
import { fetchStatusWithMeta } from '@/api/system'
import type { CollectorStatus, ExporterStatus } from '@/api/types'
import { useAsyncData } from '@/composables/useAsyncData'
import { useWarningsStore } from '@/stores/warnings'
import { formatBytes, formatNumber } from '@/utils/format'
import { formatAgo, formatDateTime } from '@/utils/time'
import ExporterEditor from '@/components/ExporterEditor.vue'
import CaptureStatusCard from '@/components/CaptureStatusCard.vue'
import DpiStatusCard from '@/components/DpiStatusCard.vue'

const warnings = useWarningsStore()
const status = useAsyncData(() => fetchStatusWithMeta(), { refreshMs: STATUS_VIEW_REFRESH_MS })
const editing = ref<ExporterStatus | null>(null)
const editorOpen = ref(false)

watch(status.data, (s) => {
  if (s) warnings.setPolled(s.warnings ?? [])
})

const s = computed(() => status.data.value)
const sumValues = (rec: Record<string, number> | undefined): number =>
  Object.values(rec ?? {}).reduce((a, b) => a + b, 0)

const storageRatio = computed(() => {
  const st = s.value?.storage
  return st && st.cap_bytes > 0 ? st.disk_bytes / st.cap_bytes : 0
})
const storageStatus = computed(() =>
  storageRatio.value >= STORAGE_ERROR_RATIO
    ? 'error'
    : storageRatio.value >= STORAGE_WARN_RATIO
      ? 'warning'
      : 'success',
)
const drops = computed(() =>
  Object.entries(s.value?.ingest.dropped_total ?? {}).map(([reason, count]) => ({ reason, count })),
)

const stateType = (state: ExporterStatus['state']): 'success' | 'error' | 'default' =>
  state === 'active' ? 'success' : state === 'stalled' ? 'error' : 'default'

function openEditor(e: ExporterStatus): void {
  editing.value = e
  editorOpen.value = true
}

const exporterColumns: DataTableColumns<ExporterStatus> = [
  {
    title: 'Exporter',
    key: 'address',
    render: (e) => [
      h('div', { class: 'mono' }, e.address),
      e.name ? h('div', { class: 'muted' }, e.name) : null,
    ],
  },
  {
    title: 'State',
    key: 'state',
    render: (e) => h(NTag, { size: 'small', type: stateType(e.state) }, () => e.state),
  },
  { title: 'Protocol', key: 'protocol', render: (e) => `${e.protocol} (domain ${e.domain_id})` },
  { title: 'Flows/s', key: 'flows_per_sec', align: 'right', render: (e) => e.flows_per_sec.toFixed(1) },
  { title: 'Last seen', key: 'last_seen', render: (e) => formatAgo(e.last_seen) },
  {
    title: 'Sampling',
    key: 'sampling_rate',
    render: (e) => (e.sampling_override ? `1:${e.sampling_override} (override)` : `1:${e.sampling_rate}`),
  },
  { title: 'Count dir', key: 'count_direction' },
  { title: 'Received', key: 'received', align: 'right', render: (e) => formatNumber(e.received) },
  { title: 'Dropped', key: 'dropped', align: 'right', render: (e) => formatNumber(sumValues(e.dropped)) },
  {
    title: 'Tmpl missing',
    key: 'template_missing',
    align: 'right',
    render: (e) => formatNumber(e.template_missing),
  },
  { title: 'Parse errors', key: 'parse_errors', align: 'right', render: (e) => formatNumber(e.parse_errors) },
  {
    title: '',
    key: 'edit',
    width: 70,
    render: (e) => h(NButton, { size: 'tiny', onClick: () => openEditor(e) }, () => 'Edit'),
  },
]

const collectorColumns: DataTableColumns<CollectorStatus> = [
  { title: 'Protocol', key: 'proto' },
  { title: 'Port', key: 'port' },
  {
    title: 'Listening',
    key: 'listening',
    render: (c) =>
      h(NTag, { size: 'small', type: c.listening ? 'success' : 'error' }, () => (c.listening ? 'yes' : 'no')),
  },
]
</script>

<template>
  <div class="stack" data-testid="status-view">
    <div class="page-toolbar">
      <span class="page-toolbar__title">Collection status</span>
      <NButton size="small" :loading="status.loading.value" @click="status.reload">Refresh</NButton>
    </div>
    <NAlert v-if="status.error.value" type="error" :bordered="false">{{ status.error.value }}</NAlert>

    <NCard size="small" title="Exporters">
      <NDataTable
        size="small"
        :columns="exporterColumns"
        :data="s?.exporters ?? []"
        :row-key="(e: ExporterStatus) => e.id"
        :scroll-x="1200"
        data-testid="exporters-table"
      />
    </NCard>

    <div class="grid-3">
      <NCard size="small" title="Ingest">
        <NDescriptions :column="1" size="small" label-placement="left">
          <NDescriptionsItem label="Flows / s">
            <span data-testid="ingest-fps">{{ (s?.ingest.flows_per_sec ?? 0).toFixed(1) }}</span>
          </NDescriptionsItem>
          <NDescriptionsItem v-for="d in drops" :key="d.reason" :label="`Dropped: ${d.reason}`">
            <span :data-testid="`drop-${d.reason}`">{{ formatNumber(d.count) }}</span>
          </NDescriptionsItem>
        </NDescriptions>
        <div v-if="drops.length === 0" class="muted">No drops</div>
      </NCard>

      <NCard size="small" title="Storage">
        <div class="storage">
          <NProgress
            type="circle"
            :percentage="Math.min(100, Math.round(storageRatio * 100))"
            :status="storageStatus"
          />
          <div>
            <div>
              {{ formatBytes(s?.storage.disk_bytes ?? 0) }} of {{ formatBytes(s?.storage.cap_bytes ?? 0) }}
            </div>
            <div class="muted">Oldest raw: {{ formatDateTime(s?.storage.oldest_raw) }}</div>
          </div>
        </div>
      </NCard>

      <NCard size="small" title="Enrichment">
        <NDescriptions :column="1" size="small" label-placement="left">
          <NDescriptionsItem label="GeoIP database">{{
            s?.enrichment.geoip_db_date ?? 'missing'
          }}</NDescriptionsItem>
          <NDescriptionsItem label="Router integration">
            {{ s?.enrichment.router_integration.enabled ? 'enabled' : 'disabled' }}
          </NDescriptionsItem>
          <NDescriptionsItem v-if="s?.enrichment.router_integration.enabled" label="Last OK">
            {{ formatAgo(s?.enrichment.router_integration.last_ok) }}
          </NDescriptionsItem>
        </NDescriptions>
        <NAlert v-if="s?.enrichment.router_integration.error" type="warning" :bordered="false">
          {{ s.enrichment.router_integration.error }}
        </NAlert>
      </NCard>
    </div>

    <div class="grid-2">
      <CaptureStatusCard :capture="s?.capture" />
      <DpiStatusCard :dpi="s?.dpi" />
    </div>

    <NCard size="small" title="Collectors">
      <NDataTable size="small" :columns="collectorColumns" :data="s?.collectors ?? []" />
    </NCard>

    <ExporterEditor v-model:show="editorOpen" :exporter="editing" @saved="status.reload" />
  </div>
</template>

<style scoped>
.storage {
  display: flex;
  align-items: center;
  gap: 16px;
}
</style>
