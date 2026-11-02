import { get, getData, request } from './client'
import type { ApiResult, StatusResponse } from './types'

export async function login(password: string): Promise<void> {
  await request<unknown>('POST', '/auth/login', { body: { password }, skipAuthRedirect: true })
}

export async function logout(): Promise<void> {
  await request<unknown>('POST', '/auth/logout', { skipAuthRedirect: true })
}

export function fetchStatus(): Promise<StatusResponse> {
  return getData<StatusResponse>('/status')
}

export function fetchStatusWithMeta(): Promise<ApiResult<StatusResponse>> {
  return get<StatusResponse>('/status')
}
