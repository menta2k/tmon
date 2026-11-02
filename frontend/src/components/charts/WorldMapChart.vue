<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { NAlert, NSpin } from 'naive-ui'
import type { TopKeyedItem } from '@/api/types'
import { useThemeStore } from '@/stores/theme'
import { errorMessage } from '@/utils/errors'
import { VChart } from './echarts'
import { buildWorldMapOption } from './options'
import { ensureWorldMap } from './worldMap'

interface Props {
  /** Country rows: `key` is the ISO-3166 alpha-2 code. */
  items: readonly TopKeyedItem[]
  height?: string
}

const props = withDefaults(defineProps<Props>(), { height: '360px' })
const theme = useThemeStore()
const ready = ref(false)
const loadError = ref<string | null>(null)

const option = computed(() => buildWorldMapOption(props.items, theme.mode === 'dark' ? '#cbd5e1' : '#334155'))

onMounted(async () => {
  try {
    await ensureWorldMap()
    ready.value = true
  } catch (err) {
    loadError.value = errorMessage(err, 'World map unavailable')
  }
})
</script>

<template>
  <div class="world-map" :style="{ height }" data-testid="world-map">
    <NAlert v-if="loadError" type="warning" :bordered="false">{{ loadError }}</NAlert>
    <NSpin v-else :show="!ready" class="world-map__spin">
      <VChart v-if="ready" :option="option" :theme="theme.mode === 'dark' ? 'dark' : ''" autoresize />
    </NSpin>
  </div>
</template>

<style scoped>
.world-map {
  width: 100%;
}
.world-map__spin,
.world-map__spin :deep(.n-spin-content) {
  height: 100%;
}
</style>
