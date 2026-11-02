/** Server-side sort of the Flows table: maps NDataTable sorter state to `sort`/`order` params. */

export type FlowSortKey = 'time' | 'bytes' | 'packets'

export interface FlowSort {
  readonly key: FlowSortKey
  readonly desc: boolean
}

/** NDataTable column sort state. */
export type ColumnSortOrder = 'ascend' | 'descend' | false

export interface SorterState {
  readonly columnKey: string | number
  readonly order: ColumnSortOrder
}

export const DEFAULT_FLOW_SORT: FlowSort = { key: 'time', desc: true }

/** Table column key ↔ API sort key. */
const COLUMN_TO_KEY: Readonly<Record<string, FlowSortKey>> = { ts: 'time', bytes: 'bytes', packets: 'packets' }

/** Column keys the table can sort by. */
export const SORTABLE_FLOW_COLUMNS: readonly string[] = Object.keys(COLUMN_TO_KEY)

export function flowSortFromSorter(state: SorterState | null | undefined): FlowSort {
  const key = state ? COLUMN_TO_KEY[String(state.columnKey)] : undefined
  if (!state || !key || state.order === false) return DEFAULT_FLOW_SORT
  return { key, desc: state.order === 'descend' }
}

/** Query params for a sort; the default (newest first) sends none. */
export function flowSortParams(sort: FlowSort): { sort?: FlowSortKey; order?: 'asc' | 'desc' } {
  if (sort.key === DEFAULT_FLOW_SORT.key && sort.desc === DEFAULT_FLOW_SORT.desc) return {}
  return { sort: sort.key, order: sort.desc ? 'desc' : 'asc' }
}

/** Sort indicator for a column. */
export function sortOrderFor(sort: FlowSort, columnKey: string): ColumnSortOrder {
  if (COLUMN_TO_KEY[columnKey] !== sort.key) return false
  return sort.desc ? 'descend' : 'ascend'
}
