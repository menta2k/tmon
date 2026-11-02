import type { BreakdownItem, Confidence } from '@/api/types'

/** Dashboard applications view: `rule` = categorization rules, `app` = classifier apps (feature 003). */
export type ApplicationsMode = 'rule' | 'app'

/** Confidence levels that do not really identify the service. */
const WEAK: ReadonlySet<Confidence> = new Set<Confidence>(['guessed', 'none'])

/** Share (0..1) of traffic categorized by port/protocol only or not at all. */
export function guessedShare(items: readonly BreakdownItem[]): number {
  return items.filter((i) => WEAK.has(i.confidence ?? 'none')).reduce((sum, i) => sum + i.share, 0)
}
