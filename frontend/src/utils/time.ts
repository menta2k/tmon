import type { Timestamp } from '@/api/types'

const DURATION_RE = /^(\d+(?:\.\d+)?)\s*(ms|s|m|h|d|w)?$/
const UNIT_SECONDS: Readonly<Record<string, number>> = {
  ms: 0.001,
  s: 1,
  m: 60,
  h: 3600,
  d: 86_400,
  w: 604_800,
}

/** Normalises an API timestamp (RFC3339 string, epoch seconds or epoch ms) to epoch ms. */
export function toMillis(ts: Timestamp): number {
  if (typeof ts === 'number') return ts < 1e12 ? ts * 1000 : ts
  if (/^\d+$/.test(ts)) return toMillis(Number(ts))
  return Date.parse(ts)
}

/** Parses a duration (`60`, `"60s"`, `"1m"`, `"1h"`) into seconds; NaN if unparseable. */
export function durationSeconds(step: number | string): number {
  if (typeof step === 'number') return step
  const match = DURATION_RE.exec(step.trim())
  if (!match) return Number.NaN
  return Number(match[1]) * (UNIT_SECONDS[match[2] ?? 's'] ?? 1)
}

export function formatDateTime(ts: Timestamp | null | undefined): string {
  if (ts === null || ts === undefined || ts === '') return '—'
  const ms = toMillis(ts)
  if (Number.isNaN(ms)) return String(ts)
  const d = new Date(ms)
  const pad = (n: number): string => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}

/** Human "x ago" for last-seen columns. */
export function formatAgo(ts: Timestamp | null | undefined, now: number = Date.now()): string {
  if (ts === null || ts === undefined || ts === '') return 'never'
  const ms = toMillis(ts)
  if (Number.isNaN(ms)) return String(ts)
  const secs = Math.max(0, Math.round((now - ms) / 1000))
  if (secs < 60) return `${secs}s ago`
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`
  if (secs < 86_400) return `${Math.floor(secs / 3600)}h ago`
  return `${Math.floor(secs / 86_400)}d ago`
}

const MINUTE_SECS = 60
const HOUR_SECS = 3600
const DAY_SECS = 86_400

/** Compact uptime: 45 → "45s", 754 → "12m", 3725 → "1h 2m", 90_000 → "1d 1h"; "—" when unknown. */
export function formatUptime(secs: number | null | undefined): string {
  if (secs === null || secs === undefined || !Number.isFinite(secs) || secs < 0) return '—'
  const s = Math.floor(secs)
  if (s < MINUTE_SECS) return `${s}s`
  if (s < HOUR_SECS) return `${Math.floor(s / MINUTE_SECS)}m`
  if (s < DAY_SECS) return `${Math.floor(s / HOUR_SECS)}h ${Math.floor((s % HOUR_SECS) / MINUTE_SECS)}m`
  return `${Math.floor(s / DAY_SECS)}d ${Math.floor((s % DAY_SECS) / HOUR_SECS)}h`
}
