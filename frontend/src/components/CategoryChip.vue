<script setup lang="ts">
import { computed } from 'vue'
import { useCategoriesStore } from '@/stores/categories'

const props = defineProps<{ id: number | string | null | undefined }>()
const categories = useCategoriesStore()
const color = computed(() => categories.colorFor(props.id))
const name = computed(() => categories.nameFor(props.id))
</script>

<template>
  <span class="chip" :style="{ borderColor: color }" data-testid="category-chip">
    <span class="chip__dot" :style="{ background: color }" />{{ name }}
  </span>
</template>

<style scoped>
.chip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 0 7px;
  border: 1px solid;
  border-radius: 10px;
  font-size: 12px;
  line-height: 18px;
  white-space: nowrap;
}
.chip__dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}
</style>
