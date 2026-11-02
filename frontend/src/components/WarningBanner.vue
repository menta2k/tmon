<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue'
import { NAlert } from 'naive-ui'
import { storeToRefs } from 'pinia'
import { useWarningsStore, warningKey } from '@/stores/warnings'
import { warningGuide } from '@/constants'
import GuideLink from '@/components/GuideLink.vue'

const store = useWarningsStore()
const { visible } = storeToRefs(store)

onMounted(() => store.start())
onUnmounted(() => store.stop())
</script>

<template>
  <div v-if="visible.length > 0" class="warning-banner" role="alert" data-testid="warning-banner">
    <NAlert
      v-for="w in visible"
      :key="warningKey(w)"
      type="warning"
      closable
      :bordered="false"
      :title="w.message"
      :data-code="w.code"
      @close="store.dismiss(w)"
    >
      <span v-if="warningGuide(w.code)" data-testid="warning-guide">
        <GuideLink :guide="warningGuide(w.code)!" />
      </span>
    </NAlert>
  </div>
</template>

<style scoped>
.warning-banner {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-bottom: 8px;
}
</style>
