export type QueryScalar = string | number | boolean
export type QueryValue = QueryScalar | readonly QueryScalar[] | null | undefined
export type QueryParams = Readonly<Record<string, QueryValue>>

/** Keys whose empty value is meaningful: `app=` selects unclassified flows. */
const KEEP_EMPTY: ReadonlySet<string> = new Set(['app'])

/**
 * Serialises params into a query string. Arrays become repeated keys
 * (`host=a&host=b`); null/undefined/empty values are omitted, except empty
 * values of the keys in `KEEP_EMPTY`.
 */
export function buildQuery(params: QueryParams): string {
  const search = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value === null || value === undefined) continue
    const values: readonly QueryScalar[] = Array.isArray(value) ? value : [value as QueryScalar]
    for (const v of values) {
      if (v === '' && !KEEP_EMPTY.has(key)) continue
      search.append(key, String(v))
    }
  }
  return search.toString()
}

/** Merges several param objects, later ones winning. Returns a new object. */
export function mergeParams(...parts: readonly QueryParams[]): QueryParams {
  return Object.assign({}, ...parts) as QueryParams
}
