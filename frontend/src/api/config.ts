import { get, getData, send } from './client'
import type {
  ApiResult,
  Category,
  CategoryInput,
  Exporter,
  ExporterPatch,
  HostPatch,
  Job,
  JobId,
  LocalHost,
  LocalNetwork,
  Rule,
  RuleInput,
  RouterSettingsPatch,
  RouterTestResult,
  RuleTestFlow,
  RuleTestResult,
  Settings,
  SettingsBundle,
  SettingsPatch,
} from './types'
import type { QueryParams } from './query'

// ---- categories ----
export const fetchCategories = (): Promise<Category[]> => getData<Category[]>('/categories')
export const createCategory = (input: CategoryInput): Promise<Category> =>
  send<Category>('POST', '/categories', input)
export const updateCategory = (id: number, patch: Partial<CategoryInput>): Promise<Category> =>
  send<Category>('PATCH', `/categories/${id}`, patch)
export const deleteCategory = (id: number): Promise<void> => send<void>('DELETE', `/categories/${id}`)

// ---- rules ----
export const fetchRules = (): Promise<Rule[]> => getData<Rule[]>('/rules')
export const createRule = (input: RuleInput): Promise<Rule> => send<Rule>('POST', '/rules', input)
export const updateRule = (id: number, patch: Partial<RuleInput>): Promise<Rule> =>
  send<Rule>('PATCH', `/rules/${id}`, patch)
export const deleteRule = (id: number): Promise<void> => send<void>('DELETE', `/rules/${id}`)
export const reorderRules = (ids: readonly number[]): Promise<void> =>
  send<void>('POST', '/rules/reorder', { ids })
export const testRule = (flow: RuleTestFlow): Promise<RuleTestResult> =>
  send<RuleTestResult>('POST', '/rules/test', { flow })
export const reapplyRules = (range: { from?: string; to?: string } = {}): Promise<{ job_id: JobId }> =>
  send<{ job_id: JobId }>('POST', '/rules/reapply', range)
export const fetchJob = (id: JobId): Promise<Job> => getData<Job>(`/jobs/${encodeURIComponent(String(id))}`)

// ---- hosts ----
export const fetchHosts = (params: QueryParams): Promise<ApiResult<LocalHost[]>> =>
  get<LocalHost[]>('/hosts', params)
export const updateHost = (ip: string, patch: HostPatch): Promise<LocalHost> =>
  send<LocalHost>('PATCH', `/hosts/${encodeURIComponent(ip)}`, patch)

// ---- networks ----
export const fetchNetworks = (): Promise<LocalNetwork[]> => getData<LocalNetwork[]>('/networks')
export const saveNetworks = (networks: readonly LocalNetwork[]): Promise<LocalNetwork[]> =>
  send<LocalNetwork[]>('PUT', '/networks', networks)

// ---- exporters ----
export const fetchExporters = (): Promise<Exporter[]> => getData<Exporter[]>('/exporters')
export const updateExporter = (id: number, patch: ExporterPatch): Promise<Exporter> =>
  send<Exporter>('PATCH', `/exporters/${id}`, patch)

// ---- settings ----
export const fetchSettings = (): Promise<Settings> => getData<Settings>('/settings')
export const updateSettings = (patch: SettingsPatch): Promise<Settings> =>
  send<Settings>('PATCH', '/settings', patch)
export const exportSettings = (): Promise<SettingsBundle> => getData<SettingsBundle>('/settings/export')
export const importSettings = (bundle: SettingsBundle): Promise<unknown> =>
  send<unknown>('POST', '/settings/import', bundle)
export const testRouterConnection = (router: RouterSettingsPatch): Promise<RouterTestResult> =>
  send<RouterTestResult>('POST', '/settings/router/test', router)
