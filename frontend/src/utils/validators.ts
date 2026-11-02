/**
 * Input validators. Each returns `null` when valid or a user-facing error message.
 */
import {
  APP_NAME_MAX,
  ASN_MAX,
  CATEGORY_DESCRIPTION_MAX,
  CATEGORY_NAME_MAX,
  IFINDEX_MAX,
  PORT_MAX,
  PORT_MIN,
  PROTO_MAX,
  RULE_NAME_MAX,
} from '@/constants'
import { isCIDR, isIP } from './ip'

export type ValidationResult = string | null

const MAC_RE = /^[0-9a-f]{2}(:[0-9a-f]{2}){5}$/i
const ISO2_RE = /^[A-Za-z]{2}$/
const COLOR_RE = /^#[0-9a-f]{6}$/i
const UINT_RE = /^\d+$/
const PORT_RANGE_RE = /^(\d+)-(\d+)$/
const HOSTNAME_GLOB_RE = /^[a-z0-9*?._-]+$/i
const PRINTABLE_ASCII_RE = /^[\x20-\x7e]*$/
const HTTP_URL_RE = /^https?:\/\/[^\s/$.?#][^\s]*$/i

export function validateIp(value: string): ValidationResult {
  return isIP(value.trim()) ? null : `"${value}" is not a valid IP address`
}

export function validateCidr(value: string): ValidationResult {
  return isCIDR(value.trim()) ? null : `"${value}" is not a valid CIDR (e.g. 192.168.0.0/16)`
}

export function validateMac(value: string): ValidationResult {
  return MAC_RE.test(value.trim()) ? null : `"${value}" is not a valid MAC (aa:bb:cc:dd:ee:ff)`
}

export function validateIso2(value: string): ValidationResult {
  return ISO2_RE.test(value.trim()) ? null : `"${value}" is not a two-letter country code`
}

export function validateColor(value: string): ValidationResult {
  return COLOR_RE.test(value.trim()) ? null : 'Colour must be #RRGGBB'
}

export function validateUintInRange(
  value: string | number,
  min: number,
  max: number,
  what: string,
): ValidationResult {
  const text = String(value).trim()
  if (!UINT_RE.test(text)) return `${what} must be a whole number`
  const n = Number(text)
  return n >= min && n <= max ? null : `${what} must be between ${min} and ${max}`
}

export function validatePort(value: string | number): ValidationResult {
  return validateUintInRange(value, PORT_MIN, PORT_MAX, 'Port')
}

/** Accepts `443` or `27015-27030` (a ≤ b, both 0–65535). */
export function validatePortSpec(value: string | number): ValidationResult {
  const text = String(value).trim()
  const range = PORT_RANGE_RE.exec(text)
  if (!range) return validatePort(text)
  const a = Number(range[1])
  const b = Number(range[2])
  if (a > PORT_MAX || b > PORT_MAX) return `Port range "${text}" exceeds ${PORT_MAX}`
  return a <= b ? null : `Port range "${text}" must have start ≤ end`
}

export function validateProto(value: string | number): ValidationResult {
  return validateUintInRange(value, 0, PROTO_MAX, 'Protocol')
}

export function validateAsn(value: string | number): ValidationResult {
  return validateUintInRange(value, 0, ASN_MAX, 'ASN')
}

export function validateIfIndex(value: string | number): ValidationResult {
  return validateUintInRange(value, 0, IFINDEX_MAX, 'Interface index')
}

export function validateLocalHost(value: string): ValidationResult {
  const v = value.trim()
  if (isIP(v) || isCIDR(v) || MAC_RE.test(v)) return null
  return `"${value}" must be an IP, CIDR or MAC address`
}

export function validateHostnameGlob(value: string): ValidationResult {
  return HOSTNAME_GLOB_RE.test(value.trim()) ? null : `"${value}" is not a valid hostname pattern`
}

/** Classifier application / category name: 1–48 printable ASCII characters, not blank. */
export function validateAppName(value: string): ValidationResult {
  if (!PRINTABLE_ASCII_RE.test(value)) return `"${value}" must use printable ASCII characters only`
  const len = value.trim().length
  return len >= 1 && value.length <= APP_NAME_MAX
    ? null
    : `Application name must be 1–${APP_NAME_MAX} characters`
}

export function validateName(value: string, max: number, what = 'Name'): ValidationResult {
  const len = value.trim().length
  return len >= 1 && len <= max ? null : `${what} must be 1–${max} characters`
}

export function validateCategoryName(value: string): ValidationResult {
  return validateName(value, CATEGORY_NAME_MAX)
}

export function validateRuleName(value: string): ValidationResult {
  return validateName(value, RULE_NAME_MAX)
}

export function validateDescription(value: string): ValidationResult {
  return value.length <= CATEGORY_DESCRIPTION_MAX
    ? null
    : `Description must be at most ${CATEGORY_DESCRIPTION_MAX} characters`
}

export function validateHttpUrl(value: string): ValidationResult {
  return HTTP_URL_RE.test(value.trim()) ? null : 'URL must start with http:// or https://'
}

/** Runs a validator over every list item and returns the first error, if any. */
export function validateEach(
  values: readonly string[],
  validator: (v: string) => ValidationResult,
): ValidationResult {
  for (const v of values) {
    const err = validator(v)
    if (err) return err
  }
  return null
}

/** Naive UI form-rule adapter: wraps a validator into `{ validator, trigger }`. */
export function formRule(
  validator: (v: string) => ValidationResult,
  trigger: string[] = ['input', 'blur'],
): { validator: (_rule: unknown, value: string) => true | Error; trigger: string[] } {
  return {
    trigger,
    validator: (_rule, value) => {
      const err = validator(value ?? '')
      return err ? new Error(err) : true
    },
  }
}
