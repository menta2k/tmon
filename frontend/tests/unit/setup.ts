import { vi } from 'vitest'

// jsdom lacks these browser APIs used by Naive UI / our composables.
if (!window.matchMedia) {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  })
}

class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
if (!('ResizeObserver' in window)) {
  Object.defineProperty(window, 'ResizeObserver', { writable: true, value: ResizeObserverStub })
}

// Charts never render in jsdom; replace VChart with a stub exposing its `option` prop.
vi.mock('@/components/charts/echarts', async () => {
  const { defineComponent, h } = await import('vue')
  const VChart = defineComponent({
    name: 'VChartStub',
    props: {
      option: { type: Object, default: () => ({}) },
      theme: { type: [String, Object], default: '' },
      autoresize: Boolean,
    },
    setup(props) {
      return () =>
        h('div', {
          class: 'vchart-stub',
          'data-series': JSON.stringify((props.option as { series?: unknown }).series ?? []),
        })
    },
  })
  return { VChart }
})
