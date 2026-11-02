<script setup lang="ts">
import { computed, h, onMounted, ref, shallowRef } from 'vue'
import {
  NAlert,
  NButton,
  NCard,
  NDataTable,
  NPopconfirm,
  NSpace,
  NSwitch,
  NTabPane,
  NTabs,
  type DataTableColumns,
} from 'naive-ui'
import type { Category, Rule } from '@/api/types'
import { deleteRule, fetchRules, reorderRules, updateRule } from '@/api/config'
import { useCategoriesStore } from '@/stores/categories'
import { useNotify } from '@/composables/useNotify'
import { errorMessage } from '@/utils/errors'
import { formatNumber } from '@/utils/format'
import { byPriorityDesc, moveItem } from '@/utils/order'
import { summarizeRuleMatch } from '@/utils/ruleMatch'
import CategoryChip from '@/components/CategoryChip.vue'
import CategoryEditor from '@/components/CategoryEditor.vue'
import ReapplyButton from '@/components/ReapplyButton.vue'
import RuleEditor from '@/components/RuleEditor.vue'
import RuleTestPanel from '@/components/RuleTestPanel.vue'

const categories = useCategoriesStore()
const notify = useNotify()
const rules = shallowRef<readonly Rule[]>([])
const loading = ref(false)
const loadError = ref<string | null>(null)
const editorRule = ref<Rule | null>(null)
const editorOpen = ref(false)
const categoryEditing = ref<Category | null>(null)
const categoryOpen = ref(false)
const dragFrom = ref<number | null>(null)

const userRules = computed(() => byPriorityDesc(rules.value.filter((r) => r.origin === 'user')))
const builtinRules = computed(() => byPriorityDesc(rules.value.filter((r) => r.origin === 'builtin')))

async function load(): Promise<void> {
  loading.value = true
  try {
    rules.value = await fetchRules()
    loadError.value = null
  } catch (err) {
    loadError.value = errorMessage(err, 'Loading rules')
  } finally {
    loading.value = false
  }
}

async function reloadCategories(): Promise<void> {
  try {
    await categories.load(true)
  } catch (err) {
    notify.error(err, 'Loading categories')
  }
}

function openEditor(rule: Rule | null): void {
  editorRule.value = rule
  editorOpen.value = true
}

async function toggle(rule: Rule, enabled: boolean): Promise<void> {
  rules.value = rules.value.map((r) => (r.id === rule.id ? { ...r, enabled } : r))
  const ok = await notify.run(() => updateRule(rule.id, { enabled }), undefined, 'Updating rule')
  if (!ok) rules.value = rules.value.map((r) => (r.id === rule.id ? { ...r, enabled: rule.enabled } : r))
}

async function remove(rule: Rule): Promise<void> {
  const ok = await notify.run(() => deleteRule(rule.id), 'Rule deleted', 'Deleting rule')
  if (ok) rules.value = rules.value.filter((r) => r.id !== rule.id)
}

async function applyOrder(ordered: readonly Rule[]): Promise<void> {
  const ids = ordered.map((r) => r.id)
  await notify.run(() => reorderRules(ids), 'Rule order saved', 'Reordering rules')
  await load()
}

function move(index: number, delta: number): void {
  void applyOrder(moveItem(userRules.value, index, index + delta))
}

function userRowProps(_row: Rule, index: number): Record<string, unknown> {
  return {
    draggable: true,
    'data-testid': 'user-rule-row',
    onDragstart: (ev: DragEvent) => {
      dragFrom.value = index
      ev.dataTransfer?.setData('text/plain', String(index))
    },
    onDragover: (ev: DragEvent) => ev.preventDefault(),
    onDrop: (ev: DragEvent) => {
      ev.preventDefault()
      const from = dragFrom.value
      dragFrom.value = null
      if (from !== null && from !== index) void applyOrder(moveItem(userRules.value, from, index))
    },
  }
}

function baseColumns(): DataTableColumns<Rule> {
  return [
    { title: 'Name', key: 'name', ellipsis: { tooltip: true } },
    { title: 'Category', key: 'category_id', render: (r) => h(CategoryChip, { id: r.category_id }) },
    {
      title: 'Match',
      key: 'match',
      ellipsis: { tooltip: true },
      render: (r) => h('span', { class: 'mono' }, summarizeRuleMatch(r.match)),
    },
    {
      title: 'Hits 24h',
      key: 'hits_24h',
      align: 'right',
      width: 90,
      render: (r) => formatNumber(r.hits_24h),
    },
    {
      title: 'On',
      key: 'enabled',
      width: 60,
      render: (r) =>
        h(NSwitch, { size: 'small', value: r.enabled, 'onUpdate:value': (v: boolean) => void toggle(r, v) }),
    },
  ]
}

function userActions(r: Rule, index: number): ReturnType<typeof h> {
  return h(NSpace, { size: 4, wrap: false }, () => [
    h(
      NButton,
      { size: 'tiny', quaternary: true, disabled: index === 0, onClick: () => move(index, -1) },
      () => '↑',
    ),
    h(
      NButton,
      {
        size: 'tiny',
        quaternary: true,
        disabled: index === userRules.value.length - 1,
        onClick: () => move(index, 1),
      },
      () => '↓',
    ),
    h(NButton, { size: 'tiny', onClick: () => openEditor(r) }, () => 'Edit'),
    h(
      NPopconfirm,
      { onPositiveClick: () => void remove(r) },
      {
        trigger: () => h(NButton, { size: 'tiny', type: 'error', ghost: true }, () => 'Delete'),
        default: () => `Delete rule "${r.name}"?`,
      },
    ),
  ])
}

const userColumns = computed<DataTableColumns<Rule>>(() => [
  {
    title: '',
    key: 'drag',
    width: 28,
    render: () => h('span', { class: 'drag-handle', title: 'Drag to reorder' }, '⋮⋮'),
  },
  ...baseColumns(),
  { title: '', key: 'actions', width: 210, render: (r, i) => userActions(r, i) },
])
const builtinColumns = baseColumns()

const categoryColumns: DataTableColumns<Category> = [
  { title: 'Name', key: 'name', render: (c) => h(CategoryChip, { id: c.id }) },
  { title: 'Colour', key: 'color', className: 'mono' },
  { title: 'Description', key: 'description', ellipsis: { tooltip: true } },
  { title: 'Built-in', key: 'builtin', width: 80, render: (c) => (c.builtin ? 'yes' : '') },
  {
    title: '',
    key: 'actions',
    width: 70,
    render: (c) => h(NButton, { size: 'tiny', onClick: () => openCategory(c) }, () => 'Edit'),
  },
]

function openCategory(c: Category | null): void {
  categoryEditing.value = c
  categoryOpen.value = true
}

onMounted(() => {
  void load()
  void reloadCategories()
})
</script>

<template>
  <div class="stack">
    <div class="page-toolbar">
      <span class="page-toolbar__title">Categorisation</span>
      <ReapplyButton @done="load" />
    </div>
    <NAlert v-if="loadError" type="error" :bordered="false">{{ loadError }}</NAlert>
    <NTabs type="line" animated>
      <NTabPane name="rules" tab="Rules">
        <div class="stack">
          <NCard size="small" title="Your rules (highest priority first, override built-ins)">
            <template #header-extra>
              <NButton size="small" type="primary" data-testid="rule-new" @click="openEditor(null)"
                >New rule</NButton
              >
            </template>
            <NDataTable
              size="small"
              :columns="userColumns"
              :data="userRules"
              :loading="loading"
              :row-key="(r: Rule) => r.id"
              :row-props="userRowProps"
              :scroll-x="900"
              data-testid="user-rules-table"
            />
          </NCard>
          <NCard size="small" title="Built-in rules">
            <NDataTable
              size="small"
              :columns="builtinColumns"
              :data="builtinRules"
              :loading="loading"
              :row-key="(r: Rule) => r.id"
              :scroll-x="760"
              :max-height="420"
            />
          </NCard>
        </div>
      </NTabPane>
      <NTabPane name="categories" tab="Categories">
        <NCard size="small" title="Categories (built-ins can be renamed or recoloured, not deleted)">
          <template #header-extra>
            <NButton size="small" type="primary" data-testid="category-new" @click="openCategory(null)"
              >New category</NButton
            >
          </template>
          <NDataTable
            size="small"
            :columns="categoryColumns"
            :data="[...categories.items]"
            :row-key="(c: Category) => c.id"
          />
        </NCard>
      </NTabPane>
      <NTabPane name="test" tab="Test">
        <NCard size="small">
          <RuleTestPanel :rules="rules" />
        </NCard>
      </NTabPane>
    </NTabs>
    <RuleEditor v-model:show="editorOpen" :rule="editorRule" @saved="load" />
    <CategoryEditor
      v-model:show="categoryOpen"
      :category="categoryEditing"
      @saved="reloadCategories"
      @deleted="reloadCategories"
    />
  </div>
</template>

<style scoped>
:deep(.drag-handle) {
  cursor: grab;
  opacity: 0.5;
  user-select: none;
}
</style>
