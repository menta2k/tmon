// T064 — DpiStatusCard, DPI warning guides and the Status view (feature 003).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import DpiStatusCard from '@/components/DpiStatusCard.vue'
import WarningBanner from '@/components/WarningBanner.vue'
import StatusView from '@/views/StatusView.vue'
import {
  DPI_DROP_SHARE_WARN,
  DPI_STATE_INFO,
  DPI_TROUBLESHOOTING_GUIDE,
  WARNING_GUIDES,
  warningGuide,
} from '@/constants'
import { DPI_STATES, type DpiEnabled, type DpiState } from '@/api/types'
import { useWarningsStore } from '@/stores/warnings'
import { formatUptime } from '@/utils/time'
import { dpiRunning, resetApiMocks, status, system } from './apiMocks'
import { mountWithApp, ok } from './helpers'
import { MockEventSource } from './eventSourceMock'

vi.mock('@/api/analytics', async () => (await import('./apiMocks')).analytics)
vi.mock('@/api/config', async () => (await import('./apiMocks')).config)
vi.mock('@/api/system', async () => (await import('./apiMocks')).system)

const tid = (w: ReturnType<typeof mount>, id: string) => w.find(`[data-testid="${id}"]`)
const card = (dpi: DpiEnabled | { enabled: false } | undefined) => mount(DpiStatusCard, { props: { dpi } })

const DPI_CODES = ['dpi_not_found', 'dpi_incompatible', 'dpi_failed', 'dpi_restarting', 'dpi_dropping']

describe('DpiStatusCard', () => {
  it('renders the running classifier counters', () => {
    const w = card(dpiRunning)
    expect(tid(w, 'dpi-card').exists()).toBe(true)
    expect(tid(w, 'dpi-state').text()).toBe('running')
    expect(tid(w, 'dpi-engine').text()).toContain('nDPI 4.10')
    expect(tid(w, 'dpi-engine').text()).toContain('0.3.0')
    expect(tid(w, 'dpi-uptime').text()).toContain('1h 2m')
    expect(tid(w, 'dpi-packets').text()).toContain('123,456')
    expect(tid(w, 'dpi-classified').text()).toContain('90%')
    expect(tid(w, 'dpi-drops').text()).toContain('1.0%')
    expect(tid(w, 'dpi-drops').attributes('data-warn')).toBe('false')
    expect(tid(w, 'dpi-restarts').text()).toContain('0')
    expect(tid(w, 'dpi-restarts').text()).toContain('1')
    const apps = tid(w, 'dpi-top-apps').text()
    expect(apps).toContain('YouTube')
    expect(apps).toContain('40')
  })

  it.each(DPI_STATES.filter((s) => s !== 'disabled'))('renders the %s state', (state: DpiState) => {
    const w = card({ ...dpiRunning, state })
    expect(tid(w, 'dpi-state').text()).toBe(DPI_STATE_INFO[state].label)
  })

  it('highlights drops above the threshold', () => {
    const share = DPI_DROP_SHARE_WARN + 0.01
    const w = card({ ...dpiRunning, queue: { ...dpiRunning.queue, drop_share_15m: share } })
    expect(tid(w, 'dpi-drops').attributes('data-warn')).toBe('true')
    expect(tid(w, 'dpi-drops').text()).toContain('6.0%')
  })

  it('shows the last error and placeholders before Hello', () => {
    const w = card({
      ...dpiRunning,
      state: 'failed',
      engine: null,
      impl_version: null,
      uptime_secs: null,
      error: 'exited with status 139',
      top_apps: [],
      classifier: { ...dpiRunning.classifier, classified_confident: 0, classified_guess: 0 },
    })
    expect(w.text()).toContain('exited with status 139')
    expect(tid(w, 'dpi-engine').text()).toContain('—')
    expect(tid(w, 'dpi-uptime').text()).toContain('—')
    expect(tid(w, 'dpi-classified').text()).toContain('—')
    expect(tid(w, 'dpi-top-apps').text()).toContain('No applications')
  })

  it('shows a disabled state with the guide', () => {
    const w = card({ enabled: false })
    expect(tid(w, 'dpi-state').text()).toBe('disabled')
    expect(w.text()).toContain(DPI_TROUBLESHOOTING_GUIDE.label)
    expect(tid(w, 'dpi-packets').exists()).toBe(false)
  })

  it('treats a missing dpi object as disabled', () => {
    expect(tid(card(undefined), 'dpi-state').text()).toBe('disabled')
  })
})

describe('formatUptime', () => {
  it.each([
    [null, '—'],
    [-1, '—'],
    [45, '45s'],
    [754, '12m'],
    [3725, '1h 2m'],
    [90_000, '1d 1h'],
  ])('%s → %s', (secs: number | null, text: string) => {
    expect(formatUptime(secs)).toBe(text)
  })
})

describe('DPI warnings', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.stubGlobal('EventSource', MockEventSource)
  })
  afterEach(() => vi.unstubAllGlobals())

  it.each(DPI_CODES)('links %s to docs/dpi.md troubleshooting', (code: string) => {
    expect(warningGuide(code)).toBe(WARNING_GUIDES[code])
    expect(warningGuide(code)?.label).toContain('docs/dpi.md')
  })

  it('shows the guide next to a DPI warning in the banner', () => {
    useWarningsStore().setPolled([{ code: 'dpi_dropping', message: 'The classifier cannot keep up' }])
    const w = mount(WarningBanner)
    expect(tid(w, 'warning-guide').text()).toContain(DPI_TROUBLESHOOTING_GUIDE.label)
    w.unmount()
  })
})

describe('StatusView DPI card', () => {
  beforeEach(() => {
    resetApiMocks()
    vi.stubGlobal('EventSource', MockEventSource)
  })
  afterEach(() => {
    document.body.innerHTML = ''
    vi.unstubAllGlobals()
  })

  it('shows the card beside the capture card', async () => {
    system.fetchStatusWithMeta.mockImplementation(() => Promise.resolve(ok({ ...status, dpi: dpiRunning })))
    const { wrapper } = await mountWithApp(StatusView)
    expect(wrapper.find('[data-testid="capture-card"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="dpi-state"]').text()).toBe('running')
    wrapper.unmount()
  })
})
