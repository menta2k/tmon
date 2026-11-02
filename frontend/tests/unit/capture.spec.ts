import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import CaptureStatusCard from '@/components/CaptureStatusCard.vue'
import WarningBanner from '@/components/WarningBanner.vue'
import { CAPTURE_SETUP_GUIDE, WARNING_GUIDES, warningGuide } from '@/constants'
import { useWarningsStore } from '@/stores/warnings'
import type { CaptureEnabled } from '@/api/types'
import { MockEventSource } from './eventSourceMock'

vi.mock('@/api/system', async () => (await import('./apiMocks')).system)

const capture: CaptureEnabled = {
  enabled: true,
  listening: true,
  address: '0.0.0.0:37008',
  error: null,
  last_datagram_at: '2026-11-01T10:00:00Z',
  datagrams: { received: 12345, rejected_source: 2, dropped_backpressure: 0, unsupported: 7, malformed: 1, max_len_15m: 1518 },
  connections: { tracked: 420, evicted: 3, reassembly_bytes: 2048 },
  bindings: { tracked: 99, evicted: 0 },
  names: { tls: 300, quic: 120, http: 4, dns: 77, ech: 5 },
  errors: {
    tls_incomplete: 6,
    tls_parse: 0,
    quic_unsupported_version: 1,
    quic_decrypt: 0,
    quic_incomplete: 2,
    dns_parse: 0,
    name_invalid: 0,
  },
  named_share_15m: 0.83,
}

const tid = (w: ReturnType<typeof mount>, id: string) => w.find(`[data-testid="${id}"]`)

describe('CaptureStatusCard', () => {
  it('renders counters, names by source and the named share', () => {
    const w = mount(CaptureStatusCard, { props: { capture } })
    expect(tid(w, 'capture-state').text()).toBe('listening')
    expect(w.text()).toContain('0.0.0.0:37008')
    expect(tid(w, 'capture-datagrams').text()).toContain('12,345')
    expect(tid(w, 'capture-named-share').text()).toContain('83%')
    for (const [src, n] of [
      ['tls', '300'],
      ['quic', '120'],
      ['http', '4'],
      ['dns', '77'],
      ['ech', '5'],
    ]) {
      expect(tid(w, `capture-names-${src}`).text()).toBe(n)
    }
    expect(tid(w, 'capture-errors').text()).toContain('9')
    expect(tid(w, 'capture-evicted').text()).toContain('3')
  })

  it('shows a dash when there were no lookups', () => {
    const w = mount(CaptureStatusCard, { props: { capture: { ...capture, named_share_15m: null } } })
    expect(tid(w, 'capture-named-share').text()).toContain('—')
  })

  it('shows the bind error when not listening', () => {
    const w = mount(CaptureStatusCard, {
      props: { capture: { ...capture, listening: false, error: 'Address already in use' } },
    })
    expect(tid(w, 'capture-state').text()).toBe('not listening')
    expect(w.text()).toContain('Address already in use')
  })

  it('shows a disabled state with the setup hint', () => {
    const w = mount(CaptureStatusCard, { props: { capture: { enabled: false } } })
    expect(tid(w, 'capture-state').text()).toBe('disabled')
    expect(w.text()).toContain(CAPTURE_SETUP_GUIDE.label)
    expect(tid(w, 'capture-datagrams').exists()).toBe(false)
  })

  it('treats a missing capture object as disabled', () => {
    const w = mount(CaptureStatusCard, { props: { capture: undefined } })
    expect(tid(w, 'capture-state').text()).toBe('disabled')
  })
})

describe('capture warnings in the banner', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.stubGlobal('EventSource', MockEventSource)
  })
  afterEach(() => vi.unstubAllGlobals())

  it('maps every capture warning code to the setup guide', () => {
    for (const code of ['capture_silent', 'capture_large_packets_lost', 'capture_fastpath_suspected']) {
      expect(warningGuide(code)).toBe(WARNING_GUIDES[code])
      expect(warningGuide(code)?.label).toContain('docs/routeros.md')
    }
    expect(warningGuide('stalled')).toBeNull()
  })

  it('shows the guide reference next to a capture warning', async () => {
    const store = useWarningsStore()
    store.setPolled([
      { code: 'capture_fastpath_suspected', message: 'Exempt the first 6 KB from FastTrack.' },
      { code: 'stalled', message: 'No flows' },
    ])
    const w = mount(WarningBanner)
    const guides = w.findAll('[data-testid="warning-guide"]')
    expect(guides).toHaveLength(1)
    expect(guides[0]!.text()).toContain(WARNING_GUIDES.capture_fastpath_suspected!.label)
    w.unmount()
  })
})
