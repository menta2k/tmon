/** Conversion between the RuleMatch API object and the string-list editor form. */
import { DIRECTIONS, RULE_APP_VALUES_MAX } from '@/constants'
import type { Direction, PortSpec, RuleMatch } from '@/api/types'
import {
  validateAppName,
  validateAsn,
  validateCidr,
  validateHostnameGlob,
  validateIfIndex,
  validateIso2,
  validateLocalHost,
  validatePortSpec,
  validateProto,
  type ValidationResult,
} from './validators'
import { isIP } from './ip'

export type RuleMatchField = keyof RuleMatch

/** Every RuleMatch field as a list of raw strings (as typed by the user). */
export type RuleMatchForm = Readonly<Record<RuleMatchField, readonly string[]>>

export const RULE_MATCH_FIELDS: readonly RuleMatchField[] = [
  'proto',
  'remote_port',
  'local_port',
  'remote_cidr',
  'local_host',
  'asn',
  'country',
  'interface',
  'hostname',
  'direction',
  'app',
  'app_category',
]

export const RULE_MATCH_LABELS: Readonly<Record<RuleMatchField, string>> = {
  proto: 'Protocol',
  remote_port: 'Remote port',
  local_port: 'Local port',
  remote_cidr: 'Remote IP / CIDR',
  local_host: 'Local host (IP, CIDR or MAC)',
  asn: 'ASN',
  country: 'Country (ISO2)',
  interface: 'Interface (ifIndex)',
  hostname: 'Remote hostname glob',
  direction: 'Direction',
  app: 'Application (classifier)',
  app_category: 'Application category (classifier)',
}

/** Fields whose list length the backend caps at RULE_APP_VALUES_MAX. */
const COUNT_LIMITED: ReadonlySet<RuleMatchField> = new Set<RuleMatchField>(['app', 'app_category'])

function validateDirection(v: string): ValidationResult {
  return (DIRECTIONS as readonly string[]).includes(v) ? null : `"${v}" is not a direction`
}

/** Remote CIDR also accepts a bare IP (treated as /32 or /128 by the backend). */
function validateRemote(v: string): ValidationResult {
  return isIP(v) ? null : validateCidr(v)
}

const VALIDATORS: Readonly<Record<RuleMatchField, (v: string) => ValidationResult>> = {
  proto: validateProto,
  remote_port: validatePortSpec,
  local_port: validatePortSpec,
  remote_cidr: validateRemote,
  local_host: validateLocalHost,
  asn: validateAsn,
  country: validateIso2,
  interface: validateIfIndex,
  hostname: validateHostnameGlob,
  direction: validateDirection,
  app: validateAppName,
  app_category: validateAppName,
}

export function emptyRuleMatchForm(): RuleMatchForm {
  return Object.fromEntries(RULE_MATCH_FIELDS.map((f) => [f, []])) as unknown as RuleMatchForm
}

export function ruleMatchToForm(match: RuleMatch): RuleMatchForm {
  return Object.fromEntries(
    RULE_MATCH_FIELDS.map((f) => [f, (match[f] ?? []).map((v: string | number) => String(v))]),
  ) as unknown as RuleMatchForm
}

export function validateRuleMatchField(field: RuleMatchField, values: readonly string[]): ValidationResult {
  if (COUNT_LIMITED.has(field) && values.length > RULE_APP_VALUES_MAX)
    return `At most ${RULE_APP_VALUES_MAX} entries`
  for (const v of values) {
    const err = VALIDATORS[field](v.trim())
    if (err) return err
  }
  return null
}

/** Returns per-field errors (empty object when valid), plus a top-level `_` error when no criterion is set. */
export function validateRuleMatchForm(form: RuleMatchForm): Record<string, string> {
  const errors: Record<string, string> = {}
  for (const field of RULE_MATCH_FIELDS) {
    const err = validateRuleMatchField(field, form[field])
    if (err) errors[field] = err
  }
  const anySet = RULE_MATCH_FIELDS.some((f) => form[f].length > 0)
  return anySet ? errors : { ...errors, _: 'Set at least one match criterion' }
}

function toPortSpec(v: string): PortSpec {
  return v.includes('-') ? v : Number(v)
}

/** Builds the API RuleMatch from a (validated) form, omitting empty fields. */
export function formToRuleMatch(form: RuleMatchForm): RuleMatch {
  const trim = (f: RuleMatchField): string[] => form[f].map((v) => v.trim()).filter((v) => v !== '')
  const nums = (f: RuleMatchField): number[] => trim(f).map(Number)
  const entries: [RuleMatchField, unknown[]][] = [
    ['proto', nums('proto')],
    ['remote_port', trim('remote_port').map(toPortSpec)],
    ['local_port', trim('local_port').map(toPortSpec)],
    ['remote_cidr', trim('remote_cidr')],
    ['local_host', trim('local_host').map((v) => v.toLowerCase())],
    ['asn', nums('asn')],
    ['country', trim('country').map((v) => v.toUpperCase())],
    ['interface', nums('interface')],
    ['hostname', trim('hostname').map((v) => v.toLowerCase())],
    ['direction', trim('direction') as Direction[]],
    ['app', trim('app')],
    ['app_category', trim('app_category')],
  ]
  return Object.fromEntries(entries.filter(([, v]) => v.length > 0)) as RuleMatch
}

/** One-line summary for the rules table, e.g. "proto 17 · remote_port 27015-27030". */
export function summarizeRuleMatch(match: RuleMatch): string {
  return RULE_MATCH_FIELDS.filter((f) => (match[f]?.length ?? 0) > 0)
    .map((f) => `${f} ${(match[f] ?? []).join(', ')}`)
    .join(' · ')
}
