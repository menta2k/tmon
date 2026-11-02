<script setup lang="ts">
import { computed } from 'vue'
import { NEmpty, NSpin } from 'naive-ui'
import type { TimeseriesSeries } from '@/api/types'
import { useThemeStore } from '@/stores/theme'
import { durationSeconds } from '@/utils/time'
import { VChart } from './echarts'
import { buildTimeseriesOption, type ColorMap } from './options'

interface Props {
  series: readonly TimeseriesSeries[]
  /** Bucket width as returned by the API (seconds or duration string). */
  step: number | string
  colors?: ColorMap
  loading?: boolean
  height?: string
}

const props = withDefaults(defineProps<Props>(), { colors: () => ({}), loading: false, height: '300px' })
const theme = useThemeStore()

const stepSeconds = computed(() => {
  const s = durationSeconds(props.step)
  return Number.isFinite(s) && s > 0 ? s : 1
})

const hasData = computed(() => props.series.some((s) => s.points.length > 0))

const option = computed(() =>
  buildTimeseriesOption({
    series: props.series,
    stepSeconds: stepSeconds.value,
    colors: props.colors,
    textColor: theme.mode === 'dark' ? '#cbd5e1' : '#334155',
  }),
)
</script>

<template>
  <div class="ts-chart" :style="{ height }" data-testid="timeseries-chart">
    <NSpin :show="loading" class="ts-chart__spin">
      <VChart v-if="hasData" :option="option" :theme="theme.mode === 'dark' ? 'dark' : ''" autoresize />
      <NEmpty v-else description="No traffic in this range" class="ts-chart__empty" />
    </NSpin>
  </div>
</template>

<style scoped>
.ts-chart {
  width: 100%;
  min-height: 160px;
}
.ts-chart__spin,
.ts-chart__spin :deep(.n-spin-content) {
  height: 100%;
}
.ts-chart__empty {
  height: 100%;
  justify-content: center;
}
</style>
