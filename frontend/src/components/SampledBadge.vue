<script setup lang="ts">
import { NTag, NTooltip } from 'naive-ui'
import { SAMPLED_BADGE_TEXT } from '@/constants'

defineProps<{ show: boolean }>()
</script>

<template>
  <NTooltip v-if="show">
    <template #trigger>
      <NTag size="small" type="warning" round data-testid="sampled-badge">{{ SAMPLED_BADGE_TEXT }}</NTag>
    </template>
    The exporter samples packets; volumes are scaled by the sampling rate and are estimates.
  </NTooltip>
</template>
