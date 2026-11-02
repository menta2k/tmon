import { LOCAL_PRIVATE_LABEL, PROTOCOL_NAMES, UNKNOWN_LABEL, DIRECTION_LABELS } from '@/constants'
import type { LocalHost } from '@/api/types'
import { isPrivateIp } from './ip'

export { isPrivateIp } from './ip'

const BYTE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'] as const
const BIT_UNITS = ['bps', 'Kbps', 'Mbps', 'Gbps', 'Tbps'] as const
const STEP = 1000
const REGIONAL_INDICATOR_A = 0x1f1e6
const ISO2_RE = /^[A-Za-z]{2}$/

function scale(value: number, units: readonly string[], precision: number): string {
  if (!Number.isFinite(value)) return '—'
  const sign = value < 0 ? '-' : ''
  let v = Math.abs(value)
  let i = 0
  while (v >= STEP && i < units.length - 1) {
    v /= STEP
    i += 1
  }
  const digits = i === 0 ? 0 : precision
  return `${sign}${v.toFixed(digits)} ${units[i]}`
}

/** 1536 → "1.5 KB" (decimal SI units, as network tools do). */
export function formatBytes(bytes: number, precision = 1): string {
  return scale(bytes, BYTE_UNITS, precision)
}

/** Formats a rate given in **bytes per second** as bits per second. */
export function formatBps(bytesPerSec: number, precision = 1): string {
  return scale(bytesPerSec * 8, BIT_UNITS, precision)
}

export function formatNumber(value: number): string {
  if (!Number.isFinite(value)) return '—'
  return new Intl.NumberFormat('en-US').format(value)
}

export function formatPercent(share: number, digits = 1): string {
  if (!Number.isFinite(share)) return '—'
  return `${(share * 100).toFixed(digits)}%`
}

/** "US" → 🇺🇸; empty string for anything that is not an ISO-3166 alpha-2 code. */
export function countryFlag(iso2: string | null | undefined): string {
  if (!iso2 || !ISO2_RE.test(iso2)) return ''
  const upper = iso2.toUpperCase()
  return String.fromCodePoint(
    REGIONAL_INDICATOR_A + upper.charCodeAt(0) - 65,
    REGIONAL_INDICATOR_A + upper.charCodeAt(1) - 65,
  )
}

/** Country column text: "Local/Private" for private IPs, flag + code otherwise. */
export function remoteCountryLabel(ip: string, country: string | null | undefined): string {
  if (isPrivateIp(ip)) return LOCAL_PRIVATE_LABEL
  if (!country) return UNKNOWN_LABEL
  return `${countryFlag(country)} ${country.toUpperCase()}`.trim()
}

/** ASN column text: "Local/Private" for private IPs, "AS15169 Google" otherwise. */
export function remoteAsnLabel(ip: string, asn: number | null | undefined, asName?: string | null): string {
  if (isPrivateIp(ip)) return LOCAL_PRIVATE_LABEL
  if (asn === null || asn === undefined) return UNKNOWN_LABEL
  return asName ? `AS${asn} ${asName}` : `AS${asn}`
}

export function protocolName(proto: number | string): string {
  const n = typeof proto === 'number' ? proto : Number(proto)
  return PROTOCOL_NAMES[n] ?? String(proto)
}

export function directionLabel(direction: string): string {
  return DIRECTION_LABELS[direction] ?? direction
}

/** Display name per data-model: friendly_name ?? dhcp_hostname ?? ip. */
export function hostDisplayName(host: Pick<LocalHost, 'ip' | 'friendly_name' | 'dhcp_hostname'>): string {
  return host.friendly_name || host.dhcp_hostname || host.ip
}

/** "ip:port" with IPv6 bracketed; port 0 (non-TCP/UDP) omitted. */
export function formatEndpoint(ip: string, port: number | null | undefined): string {
  if (!port) return ip
  return ip.includes(':') ? `[${ip}]:${port}` : `${ip}:${port}`
}
