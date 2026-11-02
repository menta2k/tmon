import { describe, expect, it } from 'vitest'
import {
  formRule,
  validateAsn,
  validateCategoryName,
  validateCidr,
  validateColor,
  validateDescription,
  validateEach,
  validateHostnameGlob,
  validateHttpUrl,
  validateIfIndex,
  validateIp,
  validateIso2,
  validateLocalHost,
  validateMac,
  validatePort,
  validatePortSpec,
  validateProto,
  validateRuleName,
} from '@/utils/validators'
import { isCIDR, isIP, isIPv6, parseIPv6 } from '@/utils/ip'

describe('ip helpers', () => {
  it('parses IPv6 forms', () => {
    expect(parseIPv6('::')).toEqual([0, 0, 0, 0, 0, 0, 0, 0])
    expect(parseIPv6('2001:db8::1')).toEqual([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1])
    expect(parseIPv6('::ffff:1.2.3.4')).toEqual([0, 0, 0, 0, 0, 0xffff, 0x102, 0x304])
    expect(parseIPv6('fe80::1%eth0')).not.toBeNull()
    expect(parseIPv6('1:2:3:4:5:6:7:8')).not.toBeNull()
  })
  it('rejects invalid IPv6', () => {
    expect(parseIPv6('1::2::3')).toBeNull()
    expect(parseIPv6('1:2:3')).toBeNull()
    expect(parseIPv6('1:2:3:4:5:6:7:8::')).toBeNull()
    expect(parseIPv6('gggg::1')).toBeNull()
    expect(parseIPv6('::1.2.3.999')).toBeNull()
    expect(isIPv6('1.2.3.4')).toBe(false)
  })
  it('validates CIDRs', () => {
    expect(isCIDR('10.0.0.0/8')).toBe(true)
    expect(isCIDR('fc00::/7')).toBe(true)
    expect(isCIDR('10.0.0.0/33')).toBe(false)
    expect(isCIDR('fc00::/129')).toBe(false)
    expect(isCIDR('10.0.0.0')).toBe(false)
    expect(isCIDR('10.0.0.0/x')).toBe(false)
    expect(isCIDR('nope/8')).toBe(false)
    expect(isIP('256.1.1.1')).toBe(false)
  })
})

describe('validators', () => {
  it('IP and CIDR', () => {
    expect(validateIp('192.168.1.1')).toBeNull()
    expect(validateIp('2001:db8::1')).toBeNull()
    expect(validateIp('300.1.1.1')).toMatch(/not a valid IP/)
    expect(validateCidr('192.168.0.0/16')).toBeNull()
    expect(validateCidr('192.168.0.0')).toMatch(/CIDR/)
  })

  it('port specs a-b with a ≤ b', () => {
    expect(validatePortSpec(443)).toBeNull()
    expect(validatePortSpec('27015-27030')).toBeNull()
    expect(validatePortSpec('100-100')).toBeNull()
    expect(validatePortSpec('27030-27015')).toMatch(/start ≤ end/)
    expect(validatePortSpec('1-70000')).toMatch(/exceeds/)
    expect(validatePortSpec('65536')).toMatch(/between/)
    expect(validatePortSpec('abc')).toMatch(/whole number/)
    expect(validatePort('-1')).toMatch(/whole number/)
  })

  it('MAC, ISO2, colour', () => {
    expect(validateMac('aa:bb:cc:dd:ee:ff')).toBeNull()
    expect(validateMac('AA:BB:CC:DD:EE:FF')).toBeNull()
    expect(validateMac('aa-bb-cc-dd-ee-ff')).toMatch(/MAC/)
    expect(validateMac('aa:bb:cc:dd:ee')).toMatch(/MAC/)
    expect(validateIso2('US')).toBeNull()
    expect(validateIso2('USA')).toMatch(/two-letter/)
    expect(validateColor('#a1B2c3')).toBeNull()
    expect(validateColor('#abc')).toMatch(/#RRGGBB/)
    expect(validateColor('red')).toMatch(/#RRGGBB/)
  })

  it('names and lengths', () => {
    expect(validateCategoryName('Gaming')).toBeNull()
    expect(validateCategoryName('')).toMatch(/1–40/)
    expect(validateCategoryName('   ')).toMatch(/1–40/)
    expect(validateCategoryName('x'.repeat(41))).toMatch(/1–40/)
    expect(validateCategoryName('x'.repeat(40))).toBeNull()
    expect(validateRuleName('x'.repeat(80))).toBeNull()
    expect(validateRuleName('x'.repeat(81))).toMatch(/1–80/)
    expect(validateDescription('x'.repeat(200))).toBeNull()
    expect(validateDescription('x'.repeat(201))).toMatch(/200/)
  })

  it('numeric ranges', () => {
    expect(validateProto(17)).toBeNull()
    expect(validateProto(256)).toMatch(/between/)
    expect(validateAsn('4294967295')).toBeNull()
    expect(validateAsn('4294967296')).toMatch(/between/)
    expect(validateIfIndex('3')).toBeNull()
  })

  it('local host, hostname glob, URL', () => {
    expect(validateLocalHost('192.168.1.1')).toBeNull()
    expect(validateLocalHost('192.168.1.0/24')).toBeNull()
    expect(validateLocalHost('aa:bb:cc:dd:ee:ff')).toBeNull()
    expect(validateLocalHost('nas')).toMatch(/IP, CIDR or MAC/)
    expect(validateHostnameGlob('*.googlevideo.com')).toBeNull()
    expect(validateHostnameGlob('bad host')).toMatch(/hostname/)
    expect(validateHttpUrl('https://192.168.88.1')).toBeNull()
    expect(validateHttpUrl('ftp://x')).toMatch(/http/)
  })

  it('validateEach returns the first error', () => {
    expect(validateEach(['1', '2'], validatePort)).toBeNull()
    expect(validateEach(['1', 'x'], validatePort)).toMatch(/whole number/)
  })

  it('formRule adapts validators for Naive UI', () => {
    const rule = formRule(validateIso2)
    expect(rule.validator(null, 'US')).toBe(true)
    expect(rule.validator(null, 'USA')).toBeInstanceOf(Error)
    expect(rule.trigger).toEqual(['input', 'blur'])
  })
})
