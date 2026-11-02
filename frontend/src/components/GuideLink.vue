<script setup lang="ts">
import type { GuideRef } from '@/constants'

defineProps<{ guide: GuideRef }>()
</script>

<template>
  <a v-if="guide.href" :href="guide.href" target="_blank" rel="noopener noreferrer" class="guide">{{
    guide.label
  }}</a>
  <span v-else class="guide muted">{{ guide.label }}</span>
</template>

<style scoped>
.guide {
  font-size: 12px;
}
</style>
