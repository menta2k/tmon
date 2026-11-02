import { describe, expect, it } from 'vitest'
import {
  countryFlag,
  directionLabel,
  formatBps,
  formatBytes,
  formatEndpoint,
  formatNumber,
  formatPercent,
  hostDisplayName,
  isPrivateIp,
  protocolName,
  remoteAsnLabel,
  remoteCountryLabel,
} from '@/utils/format'

describe('formatBytes', () => {
  it.each([
    [0, '0 B'],
    [999, '999 B'],
    [1500, '1.5 KB'],
    [2_500_000, '2.5 MB'],
    [3.2e9, '3.2 GB'],
    [7e15, '7.0 PB'],
    [-1500, '-1.5 KB'],
  ])('%d → %s', (input, expected) => {
    expect(formatBytes(input)).toBe(expected)
  })

  it('returns a dash for non-finite input', () => {
    expect(formatBytes(Number.NaN)).toBe('—')
    expect(formatBytes(Infinity)).toBe('—')
  })
})

describe('formatBps', () => {
  it('converts bytes/s to bits/s', () => {
    expect(formatBps(125)).toBe('1.0 Kbps')
    expect(formatBps(12_500_000)).toBe('100.0 Mbps')
    expect(formatBps(10)).toBe('80 bps')
    expect(formatBps(125_000_000, 0)).toBe('1 Gbps')
  })
})

describe('numbers and percentages', () => {
  it('formats with separators', () => {
    expect(formatNumber(1234567)).toBe('1,234,567')
    expect(formatNumber(Number.NaN)).toBe('—')
  })
  it('formats shares', () => {
    expect(formatPercent(0.1234)).toBe('12.3%')
    expect(formatPercent(1, 0)).toBe('100%')
    expect(formatPercent(Number.NaN)).toBe('—')
  })
})

describe('countryFlag', () => {
  it('maps ISO2 to regional indicators', () => {
    expect(countryFlag('US')).toBe('🇺🇸')
    expect(countryFlag('de')).toBe('🇩🇪')
  })
  it('returns empty for invalid input', () => {
    expect(countryFlag(null)).toBe('')
    expect(countryFlag(undefined)).toBe('')
    expect(countryFlag('USA')).toBe('')
    expect(countryFlag('1A')).toBe('')
  })
})

describe('isPrivateIp / Local/Private label', () => {
  it.each([
    '10.1.2.3',
    '172.16.0.1',
    '172.31.255.255',
    '192.168.88.1',
    '127.0.0.1',
    '100.64.1.1',
    '169.254.1.1',
    '224.0.0.1',
    '192.0.2.10',
    '198.51.100.53',
    '203.0.113.10',
  ])('%s is private', (ip) => {
    expect(isPrivateIp(ip)).toBe(true)
  })
  it.each([
    '8.8.8.8',
    '172.32.0.1',
    '1.1.1.1',
    '2001:4860:4860::8888',
    '100.128.0.1',
    '198.51.101.1',
    '203.0.114.1',
  ])('%s is public', (ip) => {
    expect(isPrivateIp(ip)).toBe(false)
  })
  it.each([
    'fd00::1',
    'fc12:3456::1',
    'fe80::1',
    '::1',
    '::',
    'ff02::1',
    '::ffff:192.168.1.1',
    '2001:db8::443',
  ])('%s (IPv6) is private', (ip) => {
    expect(isPrivateIp(ip)).toBe(true)
  })
  it('is false for garbage', () => {
    expect(isPrivateIp('not-an-ip')).toBe(false)
  })

  it('labels private remotes as Local/Private', () => {
    expect(remoteCountryLabel('192.168.1.5', null)).toBe('Local/Private')
    expect(remoteAsnLabel('fd00::5', 123, 'X')).toBe('Local/Private')
  })
  it('labels public remotes with flag, country and ASN', () => {
    expect(remoteCountryLabel('8.8.8.8', 'us')).toBe('🇺🇸 US')
    expect(remoteCountryLabel('8.8.8.8', null)).toBe('Unknown')
    expect(remoteAsnLabel('8.8.8.8', 15169, 'GOOGLE')).toBe('AS15169 GOOGLE')
    expect(remoteAsnLabel('8.8.8.8', 15169)).toBe('AS15169')
    expect(remoteAsnLabel('8.8.8.8', null)).toBe('Unknown')
  })
})

describe('misc labels', () => {
  it('names protocols', () => {
    expect(protocolName(6)).toBe('TCP')
    expect(protocolName('17')).toBe('UDP')
    expect(protocolName(253)).toBe('253')
  })
  it('names directions', () => {
    expect(directionLabel('down')).toBe('Download')
    expect(directionLabel('weird')).toBe('weird')
  })
  it('builds host display names per data-model precedence', () => {
    expect(hostDisplayName({ ip: '1.2.3.4', friendly_name: 'NAS', dhcp_hostname: 'nas-01' })).toBe('NAS')
    expect(hostDisplayName({ ip: '1.2.3.4', friendly_name: null, dhcp_hostname: 'nas-01' })).toBe('nas-01')
    expect(hostDisplayName({ ip: '1.2.3.4', friendly_name: null, dhcp_hostname: null })).toBe('1.2.3.4')
  })
  it('formats endpoints', () => {
    expect(formatEndpoint('1.2.3.4', 443)).toBe('1.2.3.4:443')
    expect(formatEndpoint('2001:db8::1', 443)).toBe('[2001:db8::1]:443')
    expect(formatEndpoint('1.2.3.4', 0)).toBe('1.2.3.4')
  })
})
