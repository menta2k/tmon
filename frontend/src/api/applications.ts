import { getData } from './client'
import type { AppCatalogEntry } from './types'

/** Classifier application catalog, sorted by name; `[]` when DPI is off or not running. */
export const fetchAppCatalog = (): Promise<AppCatalogEntry[]> =>
  getData<AppCatalogEntry[]>('/applications/catalog')
