/** Lazily builds and registers a world GeoJSON keyed by ISO-3166 alpha-2 codes. */
import { registerMap, getMap } from 'echarts/core'
import { WORLD_MAP_NAME } from './options'

let loading: Promise<void> | null = null

async function buildAndRegister(): Promise<void> {
  const [{ feature }, countries, topo] = await Promise.all([
    import('topojson-client'),
    import('i18n-iso-countries'),
    import('world-atlas/countries-110m.json'),
  ])
  const topology = (topo.default ?? topo) as unknown as Parameters<typeof feature>[0]
  const objects = (topology as unknown as { objects: Record<string, unknown> }).objects
  const collection = feature(topology, objects.countries as Parameters<typeof feature>[1]) as unknown as {
    type: 'FeatureCollection'
    features: { id?: string | number; properties: Record<string, unknown> }[]
  }
  const features = collection.features.map((f) => ({
    ...f,
    properties: {
      ...f.properties,
      name: countries.numericToAlpha2(String(f.id ?? '')) ?? String(f.properties.name ?? ''),
    },
  }))
  registerMap(WORLD_MAP_NAME, { type: 'FeatureCollection', features } as unknown as Parameters<
    typeof registerMap
  >[1])
}

export function ensureWorldMap(): Promise<void> {
  if (getMap(WORLD_MAP_NAME)) return Promise.resolve()
  loading ??= buildAndRegister().catch((err: unknown) => {
    loading = null
    throw err
  })
  return loading
}
