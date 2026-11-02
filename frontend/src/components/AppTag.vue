<script setup lang="ts">
import { computed } from 'vue'
import { NTag } from 'naive-ui'
import { APP_GUESSED_HINT } from '@/constants'

const props = defineProps<{ app?: string | null; confident?: boolean | null }>()

const sure = computed(() => props.confident === true)
</script>

<template>
  <NTag
    v-if="app"
    size="tiny"
    round
    :bordered="!sure"
    :type="sure ? 'info' : 'default'"
    :class="['app-tag', { 'app-tag--dashed': !sure }]"
    :title="sure ? undefined : APP_GUESSED_HINT"
    :data-confident="String(sure)"
    data-testid="app-tag"
  >
    {{ app }}
  </NTag>
</template>

<style scoped>
.app-tag {
  vertical-align: middle;
}
.app-tag--dashed :deep(.n-tag__border) {
  border-style: dashed;
}
</style>
