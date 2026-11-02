<script setup lang="ts">
import { computed } from 'vue'
import { NTag } from 'naive-ui'
import { NAME_SOURCE_INFO } from '@/constants'
import type { NameSource } from '@/api/types'

const props = defineProps<{ source?: NameSource | null }>()

const info = computed(() => (props.source ? (NAME_SOURCE_INFO[props.source] ?? null) : null))
</script>

<template>
  <NTag
    v-if="info"
    size="tiny"
    :bordered="false"
    round
    class="host-source"
    :title="info.hint"
    :data-source="source"
    data-testid="host-source"
  >
    {{ info.label }}
  </NTag>
</template>

<style scoped>
.host-source {
  margin-left: 4px;
  vertical-align: middle;
}
</style>
