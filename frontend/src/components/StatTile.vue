<script setup lang="ts">
import { NCard } from 'naive-ui'

defineProps<{ label: string; value: string; hint?: string; accent?: string }>()
</script>

<template>
  <NCard size="small" class="stat-tile" :style="accent ? { borderTop: `3px solid ${accent}` } : undefined">
    <div class="stat-tile__label">{{ label }}</div>
    <div class="stat-tile__value">{{ value }}</div>
    <div v-if="hint" class="stat-tile__hint">{{ hint }}</div>
  </NCard>
</template>

<style scoped>
.stat-tile__label {
  font-size: 12px;
  opacity: 0.7;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.stat-tile__value {
  font-size: 24px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
.stat-tile__hint {
  font-size: 12px;
  opacity: 0.6;
}
</style>
