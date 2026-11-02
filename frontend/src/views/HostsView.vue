<script setup lang="ts">
import { computed, h, ref, watch } from 'vue'
import { RouterLink } from 'vue-router'
import {
  NAlert,
  NButton,
  NCard,
  NDataTable,
  NInput,
  NSpace,
  type DataTableColumns,
  type PaginationProps,
} from 'naive-ui'
import { HOST_NAME_MAX, HOSTS_PAGE_SIZE, SEARCH_DEBOUNCE_MS } from '@/constants'
import type { HostPatch, LocalHost } from '@/api/types'
import { fetchHosts, updateHost } from '@/api/config'
import { useAsyncData } from '@/composables/useAsyncData'
import { useNotify } from '@/composables/useNotify'
import { formatAgo, formatDateTime } from '@/utils/time'
import { validateMac, validateName } from '@/utils/validators'

interface EditState {
  ip: string
  friendly_name: string
  mac: string
}

const notify = useNotify()
const search = ref('')
const appliedSearch = ref('')
const page = ref(1)
const editing = ref<EditState | null>(null)
const saving = ref(false)
let debounce: ReturnType<typeof setTimeout> | null = null

watch(search, (q) => {
  if (debounce) clearTimeout(debounce)
  debounce = setTimeout(() => {
    appliedSearch.value = q.trim()
    page.value = 1
  }, SEARCH_DEBOUNCE_MS)
})

const query = computed(() => ({
  page: page.value,
  limit: HOSTS_PAGE_SIZE,
  q: appliedSearch.value || undefined,
}))
const hosts = useAsyncData(() => fetchHosts(query.value), { watch: [query] })
const rows = computed<LocalHost[]>(() => [...(hosts.data.value ?? [])])

const nameError = computed(() =>
  editing.value && editing.value.friendly_name.trim()
    ? validateName(editing.value.friendly_name, HOST_NAME_MAX)
    : null,
)
const macError = computed(() =>
  editing.value && editing.value.mac.trim() ? validateMac(editing.value.mac) : null,
)

function startEdit(host: LocalHost): void {
  editing.value = { ip: host.ip, friendly_name: host.friendly_name ?? '', mac: host.mac ?? '' }
}

function setEdit<K extends keyof EditState>(key: K, value: string): void {
  if (editing.value) editing.value = { ...editing.value, [key]: value }
}

async function saveEdit(): Promise<void> {
  const e = editing.value
  if (!e || nameError.value || macError.value) return
  const patch: HostPatch = {
    friendly_name: e.friendly_name.trim() || null,
    mac: e.mac.trim().toLowerCase() || null,
  }
  saving.value = true
  try {
    const updated = await updateHost(e.ip, patch)
    hosts.data.value = (hosts.data.value ?? []).map((x) => (x.ip === updated.ip ? updated : x))
    editing.value = null
    notify.success('Host updated')
  } catch (err) {
    notify.error(err, 'Saving host')
  } finally {
    saving.value = false
  }
}

const isEditing = (row: LocalHost): boolean => editing.value?.ip === row.ip

function editInput(
  key: 'friendly_name' | 'mac',
  placeholder: string,
  error: string | null,
): ReturnType<typeof h> {
  return h(NInput, {
    size: 'tiny',
    value: editing.value?.[key] ?? '',
    placeholder,
    status: error ? 'error' : undefined,
    title: error ?? undefined,
    'data-testid': `host-edit-${key}`,
    'onUpdate:value': (v: string) => setEdit(key, v),
    onKeyup: (ev: KeyboardEvent) => {
      if (ev.key === 'Enter') void saveEdit()
      if (ev.key === 'Escape') editing.value = null
    },
  })
}

function actions(row: LocalHost): ReturnType<typeof h> {
  if (!isEditing(row)) {
    return h(
      NButton,
      { size: 'tiny', 'data-testid': 'host-edit', onClick: () => startEdit(row) },
      () => 'Edit',
    )
  }
  return h(NSpace, { size: 4, wrap: false }, () => [
    h(
      NButton,
      {
        size: 'tiny',
        type: 'primary',
        loading: saving.value,
        'data-testid': 'host-save',
        onClick: () => void saveEdit(),
      },
      () => 'Save',
    ),
    h(NButton, { size: 'tiny', onClick: () => (editing.value = null) }, () => 'Cancel'),
  ])
}

const columns = computed<DataTableColumns<LocalHost>>(() => [
  {
    title: 'IP',
    key: 'ip',
    className: 'mono',
    render: (r) => h(RouterLink, { to: { name: 'host-detail', params: { ip: r.ip } } }, () => r.ip),
  },
  {
    title: 'Friendly name',
    key: 'friendly_name',
    render: (r) =>
      isEditing(r) ? editInput('friendly_name', 'e.g. NAS', nameError.value) : (r.friendly_name ?? '—'),
  },
  { title: 'DHCP hostname', key: 'dhcp_hostname', render: (r) => r.dhcp_hostname ?? '—' },
  {
    title: 'MAC',
    key: 'mac',
    className: 'mono',
    render: (r) => (isEditing(r) ? editInput('mac', 'aa:bb:cc:dd:ee:ff', macError.value) : (r.mac ?? '—')),
  },
  { title: 'First seen', key: 'first_seen', render: (r) => formatDateTime(r.first_seen) },
  { title: 'Last seen', key: 'last_seen', render: (r) => formatAgo(r.last_seen) },
  { title: '', key: 'actions', width: 130, render: actions },
])

const pagination = computed<PaginationProps>(() => ({
  page: page.value,
  pageSize: HOSTS_PAGE_SIZE,
  itemCount: hosts.meta.value.total ?? rows.value.length,
  onUpdatePage: (p: number) => {
    page.value = p
  },
}))
</script>

<template>
  <div class="stack">
    <div class="page-toolbar">
      <span class="page-toolbar__title">Host names</span>
      <NInput
        v-model:value="search"
        size="small"
        clearable
        placeholder="Search IP, name or MAC"
        style="max-width: 260px"
        data-testid="hosts-search"
      />
    </div>
    <NAlert v-if="hosts.error.value" type="error" :bordered="false">{{ hosts.error.value }}</NAlert>
    <NAlert v-if="nameError || macError" type="warning" :bordered="false">{{ nameError ?? macError }}</NAlert>
    <NCard size="small">
      <NDataTable
        remote
        size="small"
        :columns="columns"
        :data="rows"
        :loading="hosts.loading.value"
        :pagination="pagination"
        :row-key="(r: LocalHost) => r.ip"
        :scroll-x="900"
        data-testid="hosts-table"
      />
    </NCard>
  </div>
</template>
