import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiError, apiUrl, getData, request, send, setUnauthorizedHandler } from '@/api/client'
import { buildQuery, mergeParams } from '@/api/query'
import * as analytics from '@/api/analytics'
import * as config from '@/api/config'
import * as system from '@/api/system'

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

const fetchMock = vi.fn<typeof fetch>()

beforeEach(() => {
  fetchMock.mockReset()
  vi.stubGlobal('fetch', fetchMock)
})
afterEach(() => {
  vi.unstubAllGlobals()
  setUnauthorizedHandler(null)
})

describe('query building', () => {
  it('repeats array params and skips empties', () => {
    expect(
      buildQuery({ host: ['1.2.3.4', '5.6.7.8'], from: 'now-15m', x: undefined, y: null, z: '', n: 0 }),
    ).toBe('host=1.2.3.4&host=5.6.7.8&from=now-15m&n=0')
    // `app=` (empty) means "unclassified" and is the one empty value kept.
    expect(buildQuery({ app: ['', 'YouTube'], host: [''], z: '' })).toBe('app=&app=YouTube')
    expect(apiUrl('/flows', { page: 2 })).toBe('/api/v1/flows?page=2')
    expect(apiUrl('/status')).toBe('/api/v1/status')
  })
  it('merges params without mutating inputs', () => {
    const a = { a: 1 }
    const merged = mergeParams(a, { b: 2 }, { a: 3 })
    expect(merged).toEqual({ a: 3, b: 2 })
    expect(a).toEqual({ a: 1 })
  })
})

describe('request', () => {
  it('unwraps data and meta from the envelope', async () => {
    fetchMock.mockResolvedValue(
      jsonResponse({ ok: true, data: [1, 2], error: null, meta: { sampled: true, total: 2 } }),
    )
    const res = await request<number[]>('GET', '/x', { query: { host: ['a', 'b'] } })
    expect(res).toEqual({ data: [1, 2], meta: { sampled: true, total: 2 } })
    const [url, init] = fetchMock.mock.calls[0]!
    expect(url).toBe('/api/v1/x?host=a&host=b')
    expect(init?.credentials).toBe('same-origin')
  })

  it('sends JSON bodies', async () => {
    fetchMock.mockResolvedValue(jsonResponse({ ok: true, data: { id: 1 }, error: null }))
    await send('POST', '/rules', { name: 'r' })
    const init = fetchMock.mock.calls[0]![1]!
    expect(init.method).toBe('POST')
    expect(init.body).toBe('{"name":"r"}')
    expect((init.headers as Record<string, string>)['Content-Type']).toBe('application/json')
  })

  it('throws ApiError with code, message and fields', async () => {
    fetchMock.mockResolvedValue(
      jsonResponse(
        {
          ok: false,
          data: null,
          error: { code: 'validation_failed', message: 'bad', fields: { 'remote_cidr[0]': 'invalid CIDR' } },
        },
        400,
      ),
    )
    const err = await request('POST', '/rules').catch((e: unknown) => e)
    expect(err).toBeInstanceOf(ApiError)
    expect((err as ApiError).code).toBe('validation_failed')
    expect((err as ApiError).status).toBe(400)
    expect((err as ApiError).fields).toEqual({ 'remote_cidr[0]': 'invalid CIDR' })
  })

  it('calls the unauthorized handler on 401 unless skipped', async () => {
    const handler = vi.fn()
    setUnauthorizedHandler(handler)
    fetchMock.mockImplementation(async () =>
      jsonResponse({ ok: false, data: null, error: { code: 'unauthorized', message: 'no' } }, 401),
    )
    await expect(getData('/status')).rejects.toBeInstanceOf(ApiError)
    expect(handler).toHaveBeenCalledTimes(1)
    await expect(request('POST', '/auth/login', { skipAuthRedirect: true })).rejects.toMatchObject({
      isUnauthorized: true,
    })
    expect(handler).toHaveBeenCalledTimes(1)
  })

  it('default 401 handler redirects to /login', async () => {
    const assign = vi.fn()
    vi.stubGlobal('location', { ...window.location, pathname: '/flows', assign })
    fetchMock.mockResolvedValue(new Response('', { status: 401 }))
    await expect(getData('/status')).rejects.toMatchObject({ code: 'unauthorized' })
    expect(assign).toHaveBeenCalledWith('/login')
  })

  it('handles non-JSON and empty error bodies', async () => {
    fetchMock.mockResolvedValueOnce(new Response('<html>oops</html>', { status: 502 }))
    await expect(getData('/x')).rejects.toMatchObject({ code: 'invalid_response' })
    fetchMock.mockResolvedValueOnce(new Response('', { status: 500 }))
    await expect(getData('/x')).rejects.toMatchObject({ code: 'http_error', status: 500 })
  })

  it('accepts a 204/empty success body and raw (non-envelope) JSON', async () => {
    fetchMock.mockResolvedValueOnce(new Response(null, { status: 204 }))
    await expect(getData('/x')).resolves.toBeNull()
    fetchMock.mockResolvedValueOnce(jsonResponse({ categories: [] }))
    await expect(getData('/settings/export')).resolves.toEqual({ categories: [] })
  })

  it('maps network failures to a friendly ApiError', async () => {
    fetchMock.mockRejectedValue(new TypeError('Failed to fetch'))
    await expect(getData('/x')).rejects.toMatchObject({ code: 'network_error', status: 0 })
  })

  it('rethrows aborts untouched', async () => {
    fetchMock.mockRejectedValue(new DOMException('aborted', 'AbortError'))
    await expect(getData('/x')).rejects.toBeInstanceOf(DOMException)
  })
})

describe('endpoint wrappers', () => {
  beforeEach(() => {
    fetchMock.mockImplementation(async () => jsonResponse({ ok: true, data: {}, error: null }))
  })

  const lastCall = (): [string, RequestInit | undefined] => {
    const c = fetchMock.mock.calls.at(-1)!
    return [String(c[0]), c[1]]
  }

  it('builds analytics URLs', async () => {
    await analytics.fetchTimeseries('category', { from: 'now-1h', to: 'now' })
    expect(lastCall()[0]).toBe('/api/v1/traffic/timeseries?from=now-1h&to=now&group_by=category')
    await analytics.fetchBreakdown('proto', { host: ['1.1.1.1'] })
    expect(lastCall()[0]).toBe('/api/v1/traffic/breakdown?host=1.1.1.1&by=proto')
    await analytics.fetchTopHosts({ limit: 20 })
    expect(lastCall()[0]).toBe('/api/v1/top/hosts?limit=20')
    await analytics.fetchTopRemotes({})
    expect(lastCall()[0]).toBe('/api/v1/top/remotes')
    await analytics.fetchTopKeyed('countries', {})
    expect(lastCall()[0]).toBe('/api/v1/top/countries')
    await analytics.fetchHostDetail('fd00::1', { from: 'now-15m' })
    expect(lastCall()[0]).toBe('/api/v1/hosts/fd00%3A%3A1?from=now-15m')
    await analytics.fetchFlows({ page: 1, limit: 50 })
    expect(lastCall()[0]).toBe('/api/v1/flows?page=1&limit=50')
    expect(analytics.flowsExportUrl({ host: ['a'], page: 2, limit: 50, port: 443 })).toBe(
      '/api/v1/flows/export.csv?host=a&port=443',
    )
  })

  it('builds configuration requests', async () => {
    const calls: [() => Promise<unknown>, string, string][] = [
      [() => config.fetchCategories(), 'GET', '/api/v1/categories'],
      [
        () => config.createCategory({ name: 'a', color: '#000000', description: '' }),
        'POST',
        '/api/v1/categories',
      ],
      [() => config.updateCategory(3, { name: 'b' }), 'PATCH', '/api/v1/categories/3'],
      [() => config.deleteCategory(3), 'DELETE', '/api/v1/categories/3'],
      [() => config.fetchRules(), 'GET', '/api/v1/rules'],
      [
        () => config.createRule({ name: 'r', category_id: 1, enabled: true, match: { proto: [17] } }),
        'POST',
        '/api/v1/rules',
      ],
      [() => config.updateRule(5, { enabled: false }), 'PATCH', '/api/v1/rules/5'],
      [() => config.deleteRule(5), 'DELETE', '/api/v1/rules/5'],
      [() => config.reorderRules([3, 1]), 'POST', '/api/v1/rules/reorder'],
      [() => config.testRule({ proto: 17 }), 'POST', '/api/v1/rules/test'],
      [() => config.reapplyRules(), 'POST', '/api/v1/rules/reapply'],
      [() => config.fetchJob('j1'), 'GET', '/api/v1/jobs/j1'],
      [() => config.fetchHosts({ page: 1 }), 'GET', '/api/v1/hosts?page=1'],
      [() => config.updateHost('10.0.0.1', { friendly_name: 'x' }), 'PATCH', '/api/v1/hosts/10.0.0.1'],
      [() => config.fetchNetworks(), 'GET', '/api/v1/networks'],
      [() => config.saveNetworks([{ cidr: '10.0.0.0/8', label: '' }]), 'PUT', '/api/v1/networks'],
      [() => config.fetchExporters(), 'GET', '/api/v1/exporters'],
      [() => config.updateExporter(2, { name: 'r' }), 'PATCH', '/api/v1/exporters/2'],
      [() => config.fetchSettings(), 'GET', '/api/v1/settings'],
      [() => config.updateSettings({ storage: { disk_cap_gb: 5 } }), 'PATCH', '/api/v1/settings'],
      [() => config.exportSettings(), 'GET', '/api/v1/settings/export'],
      [() => config.importSettings({}), 'POST', '/api/v1/settings/import'],
      [() => config.testRouterConnection({ url: 'https://r' }), 'POST', '/api/v1/settings/router/test'],
      [() => system.fetchStatus(), 'GET', '/api/v1/status'],
      [() => system.fetchStatusWithMeta(), 'GET', '/api/v1/status'],
      [() => system.login('pw'), 'POST', '/api/v1/auth/login'],
      [() => system.logout(), 'POST', '/api/v1/auth/logout'],
    ]
    for (const [call, method, url] of calls) {
      await call()
      const [gotUrl, init] = lastCall()
      expect([init?.method, gotUrl]).toEqual([method, url])
    }
    await config.reorderRules([3, 1])
    expect(lastCall()[1]?.body).toBe('{"ids":[3,1]}')
    await config.testRule({ proto: 17 })
    expect(lastCall()[1]?.body).toBe('{"flow":{"proto":17}}')
  })
})
