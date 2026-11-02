import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import HostSourceTag from '@/components/HostSourceTag.vue'
import { NAME_SOURCE_INFO } from '@/constants'
import { NAME_SOURCES, type NameSource } from '@/api/types'

describe('HostSourceTag', () => {
  it.each(NAME_SOURCES)('renders the %s label with an explanation', (source: NameSource) => {
    const w = mount(HostSourceTag, { props: { source } })
    const tag = w.find('[data-testid="host-source"]')
    expect(tag.exists()).toBe(true)
    expect(tag.text()).toBe(NAME_SOURCE_INFO[source].label)
    expect(tag.attributes('title')).toBe(NAME_SOURCE_INFO[source].hint)
    expect(tag.attributes('data-source')).toBe(source)
  })

  it('renders nothing without a source', () => {
    expect(mount(HostSourceTag, { props: { source: null } }).find('[data-testid="host-source"]').exists()).toBe(false)
    expect(mount(HostSourceTag, { props: {} }).find('[data-testid="host-source"]').exists()).toBe(false)
  })

  it('has a distinct label for every source', () => {
    const labels = NAME_SOURCES.map((s) => NAME_SOURCE_INFO[s].label)
    expect(new Set(labels).size).toBe(NAME_SOURCES.length)
  })
})
