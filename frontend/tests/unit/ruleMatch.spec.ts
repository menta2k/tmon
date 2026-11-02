import { describe, expect, it } from 'vitest'
import {
  emptyRuleMatchForm,
  formToRuleMatch,
  ruleMatchToForm,
  summarizeRuleMatch,
  validateRuleMatchField,
  validateRuleMatchForm,
} from '@/utils/ruleMatch'
import { byPriorityDesc, moveItem } from '@/utils/order'

describe('rule match form', () => {
  it('requires at least one criterion', () => {
    expect(validateRuleMatchForm(emptyRuleMatchForm())).toEqual({ _: 'Set at least one match criterion' })
  })

  it('round-trips a match', () => {
    const match = { proto: [17], remote_port: ['27015-27030', 443] as (string | number)[], country: ['DE'] }
    const form = ruleMatchToForm(match)
    expect(form.remote_port).toEqual(['27015-27030', '443'])
    expect(validateRuleMatchForm(form)).toEqual({})
    expect(formToRuleMatch(form)).toEqual({ proto: [17], remote_port: ['27015-27030', 443], country: ['DE'] })
  })

  it('normalises case and drops empty fields', () => {
    const form = {
      ...emptyRuleMatchForm(),
      country: ['de'],
      hostname: ['*.Example.COM'],
      local_host: ['AA:BB:CC:DD:EE:FF'],
      direction: ['up'],
    }
    expect(formToRuleMatch(form)).toEqual({
      country: ['DE'],
      hostname: ['*.example.com'],
      local_host: ['aa:bb:cc:dd:ee:ff'],
      direction: ['up'],
    })
  })

  it('reports per-field errors', () => {
    const form = {
      ...emptyRuleMatchForm(),
      remote_port: ['30-10'],
      remote_cidr: ['1.2.3.4', '10.0.0.0/40'],
      direction: ['sideways'],
    }
    const errors = validateRuleMatchForm(form)
    expect(errors.remote_port).toMatch(/start ≤ end/)
    expect(errors.remote_cidr).toMatch(/CIDR/)
    expect(errors.direction).toMatch(/not a direction/)
    expect(validateRuleMatchField('remote_cidr', ['203.0.113.0/24', '8.8.8.8'])).toBeNull()
  })

  it('summarises a match', () => {
    expect(summarizeRuleMatch({ proto: [17], remote_port: ['27015-27030'] })).toBe(
      'proto 17 · remote_port 27015-27030',
    )
    expect(summarizeRuleMatch({})).toBe('')
  })
})

describe('order helpers', () => {
  it('moves items immutably', () => {
    const src = ['a', 'b', 'c', 'd'] as const
    expect(moveItem(src, 0, 2)).toEqual(['b', 'c', 'a', 'd'])
    expect(moveItem(src, 3, 0)).toEqual(['d', 'a', 'b', 'c'])
    expect(moveItem(src, 1, 1)).toEqual(['a', 'b', 'c', 'd'])
    expect(moveItem(src, 1, 9)).toEqual(['a', 'b', 'c', 'd'])
    expect(src).toEqual(['a', 'b', 'c', 'd'])
  })
  it('sorts by priority desc without mutating', () => {
    const src = [{ priority: 1 }, { priority: 3 }, { priority: 2 }]
    expect(byPriorityDesc(src).map((r) => r.priority)).toEqual([3, 2, 1])
    expect(src[0]?.priority).toBe(1)
  })
})
