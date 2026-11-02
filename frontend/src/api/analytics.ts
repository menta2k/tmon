import { get } from './client'
import { mergeParams, type QueryParams } from './query'
import type {
  ApiResult,
  BreakdownBy,
  BreakdownItem,
  FlowRecord,
  GroupBy,
  HostDetail,
  TimeseriesResponse,
  TopHost,
  TopKeyedItem,
  TopKeyedKind,
  TopRemote,
} from './types'
import { apiUrl } from './client'

export function fetchTimeseries(
  groupBy: GroupBy,
  params: QueryParams,
): Promise<ApiResult<TimeseriesResponse>> {
  return get<TimeseriesResponse>('/traffic/timeseries', mergeParams(params, { group_by: groupBy }))
}

export function fetchBreakdown(by: BreakdownBy, params: QueryParams): Promise<ApiResult<BreakdownItem[]>> {
  return get<BreakdownItem[]>('/traffic/breakdown', mergeParams(params, { by }))
}

export function fetchTopHosts(params: QueryParams): Promise<ApiResult<TopHost[]>> {
  return get<TopHost[]>('/top/hosts', params)
}

export function fetchTopRemotes(params: QueryParams): Promise<ApiResult<TopRemote[]>> {
  return get<TopRemote[]>('/top/remotes', params)
}

export function fetchTopKeyed(kind: TopKeyedKind, params: QueryParams): Promise<ApiResult<TopKeyedItem[]>> {
  return get<TopKeyedItem[]>(`/top/${kind}`, params)
}

export function fetchHostDetail(ip: string, params: QueryParams): Promise<ApiResult<HostDetail>> {
  return get<HostDetail>(`/hosts/${encodeURIComponent(ip)}`, params)
}

export function fetchFlows(params: QueryParams): Promise<ApiResult<FlowRecord[]>> {
  return get<FlowRecord[]>('/flows', params)
}

/** URL for the CSV export using the same filter params as the table (no paging). */
export function flowsExportUrl(params: QueryParams): string {
  const { page: _page, limit: _limit, ...rest } = params
  return apiUrl('/flows/export.csv', rest)
}
