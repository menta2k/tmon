<script setup lang="ts">
import { computed, ref } from 'vue'
import { NAlert, NCard, NRadioButton, NRadioGroup } from 'naive-ui'
import { storeToRefs } from 'pinia'
import { DASHBOARD_REFRESH_MS, DIRECTION_COLORS } from '@/constants'
import { fetchBreakdown, fetchTimeseries } from '@/api/analytics'
import { mergeParams } from '@/api/query'
import type { BreakdownItem } from '@/api/types'
import { useAsyncData } from '@/composables/useAsyncData'
import { useAppBreakdown } from '@/composables/useAppBreakdown'
import type { ApplicationsMode } from '@/utils/confidence'
import { useRangeFilters } from '@/composables/useRangeFilters'
import { useTimeRangeStore } from '@/stores/timeRange'
import { useCategoriesStore } from '@/stores/categories'
import { useLiveStore } from '@/stores/live'
import { formatBps, formatBytes } from '@/utils/format'
import FilterBar from '@/components/FilterBar.vue'
import SampledBadge from '@/components/SampledBadge.vue'
import StatTile from '@/components/StatTile.vue'
import TimeSeriesChart from '@/components/charts/TimeSeriesChart.vue'
import BreakdownChart from '@/components/charts/BreakdownChart.vue'
import ApplicationsTable from '@/components/ApplicationsTable.vue'

type DirectionToggle = 'all' | 'down' | 'up'

const { params } = useRangeFilters()
const { isLive } = storeToRefs(useTimeRangeStore())
const categories = useCategoriesStore()
const live = useLiveStore()
const direction = ref<DirectionToggle>('all')
const appsMode = ref<ApplicationsMode>('rule')

const tsParams = computed(() =>
  direction.value === 'all' ? params.value : mergeParams(params.value, { direction: direction.value }),
)

const loadOpts = { refreshMs: DASHBOARD_REFRESH_MS, refreshEnabled: isLive }

const timeseries = useAsyncData(() => fetchTimeseries('category', tsParams.value), {
  ...loadOpts,
  watch: [tsParams],
})
const byCategory = useAsyncData(() => fetchBreakdown('category', params.value), {
  ...loadOpts,
  watch: [params],
})
const byProto = useAsyncData(() => fetchBreakdown('proto', params.value), { ...loadOpts, watch: [params] })
const byDirection = useAsyncData(() => fetchBreakdown('direction', params.value), {
  ...loadOpts,
  watch: [params],
})
const byRule = useAsyncData(() => fetchBreakdown('rule', params.value), { ...loadOpts, watch: [params] })
const byApp = useAppBreakdown(
  params,
  computed(() => appsMode.value === 'app'),
  loadOpts,
)
const appItems = computed(() => (appsMode.value === 'app' ? byApp.data.value : byRule.data.value) ?? [])
const appLoading = computed(() => (appsMode.value === 'app' ? byApp.loading.value : byRule.loading.value))

const categoryColors = computed<Record<string, string>>(() =>
  Object.fromEntries(categories.items.map((c) => [String(c.id), c.color])),
)

const sampled = computed(() =>
  [timeseries, byCategory, byProto, byDirection, byRule].some((d) => d.meta.value.sampled === true),
)

const error = computed(
  () =>
    timeseries.error.value ??
    byCategory.error.value ??
    byProto.error.value ??
    byDirection.error.value ??
    byRule.error.value ??
    (appsMode.value === 'app' ? byApp.error.value : null),
)

const sumBytes = (items: readonly BreakdownItem[] | null): number =>
  (items ?? []).reduce((s, i) => s + i.bytes, 0)
const rangeTotal = computed(() => formatBytes(sumBytes(byCategory.data.value)))
const series = computed(() => timeseries.data.value?.series ?? [])
const step = computed(() => timeseries.data.value?.step ?? 1)
</script>

<template>
  <div class="stack" data-testid="dashboard">
    <div class="page-toolbar">
      <span class="page-toolbar__title">Dashboard</span>
      <SampledBadge :show="sampled" />
    </div>
    <FilterBar />
    <NAlert v-if="error" type="error" :bordered="false" data-testid="dashboard-error">{{ error }}</NAlert>

    <div class="grid-3">
      <StatTile
        label="Download (live)"
        :value="formatBps(live.down)"
        :accent="DIRECTION_COLORS.down"
        data-testid="tile-down"
      />
      <StatTile
        label="Upload (live)"
        :value="formatBps(live.up)"
        :accent="DIRECTION_COLORS.up"
        data-testid="tile-up"
      />
      <StatTile label="Total in range" :value="rangeTotal" hint="all categories" />
    </div>

    <NCard size="small">
      <template #header>Throughput by category</template>
      <template #header-extra>
        <NRadioGroup v-model:value="direction" size="small" data-testid="direction-toggle">
          <NRadioButton value="all">All</NRadioButton>
          <NRadioButton value="down">Download</NRadioButton>
          <NRadioButton value="up">Upload</NRadioButton>
        </NRadioGroup>
      </template>
      <TimeSeriesChart
        :series="series"
        :step="step"
        :colors="categoryColors"
        :loading="timeseries.loading.value"
      />
    </NCard>

    <div class="grid-3">
      <NCard size="small">
        <BreakdownChart
          title="Category"
          :items="byCategory.data.value ?? []"
          :colors="categoryColors"
          :loading="byCategory.loading.value"
        />
      </NCard>
      <NCard size="small">
        <BreakdownChart title="Protocol" :items="byProto.data.value ?? []" :loading="byProto.loading.value" />
      </NCard>
      <NCard size="small">
        <BreakdownChart
          title="Direction"
          :items="byDirection.data.value ?? []"
          :colors="DIRECTION_COLORS"
          :loading="byDirection.loading.value"
        />
      </NCard>
    </div>

    <NCard size="small">
      <template #header>Applications</template>
      <template #header-extra>
        <NRadioGroup v-model:value="appsMode" size="small" data-testid="applications-mode">
          <NRadioButton value="rule">By rule</NRadioButton>
          <NRadioButton value="app">By app</NRadioButton>
        </NRadioGroup>
      </template>
      <template v-if="appsMode === 'app'">
        <NAlert
          v-if="byApp.dpiEnabled.value === false"
          type="info"
          :bordered="false"
          class="apps-note"
          data-testid="dpi-disabled-hint"
        >
          This view requires application classification, which is turned off. Add a
          <code>[dpi]</code> section to the config to enable it.
        </NAlert>
        <NAlert
          v-if="byApp.rangeUnsupported.value"
          type="info"
          :bordered="false"
          class="apps-note"
          data-testid="app-range-unsupported"
        >
          Applications are only available for ranges within the raw-data retention. Pick a shorter time range.
        </NAlert>
      </template>
      <ApplicationsTable :items="appItems" :loading="appLoading" :mode="appsMode" />
    </NCard>
  </div>
</template>

<style scoped>
.apps-note {
  margin-bottom: 8px;
}
</style>
