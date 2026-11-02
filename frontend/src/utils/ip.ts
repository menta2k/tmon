/** Small IPv4/IPv6 helpers (parsing, CIDR checks, private-range detection). */

const IPV4_RE = /^(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}$/
const HEXTET_RE = /^[0-9a-f]{1,4}$/i

export function isIPv4(value: string): boolean {
  return IPV4_RE.test(value)
}

function ipv4ToBytes(value: string): number[] {
  return value.split('.').map(Number)
}

function expandEmbeddedIpv4(groups: readonly string[]): string[] | null {
  const last = groups[groups.length - 1]
  if (last === undefined || !last.includes('.')) return [...groups]
  if (!isIPv4(last)) return null
  const [a, b, c, d] = ipv4ToBytes(last) as [number, number, number, number]
  return [...groups.slice(0, -1), ((a << 8) | b).toString(16), ((c << 8) | d).toString(16)]
}

/** Parses an IPv6 address into 8 hextets, or returns null when invalid. */
export function parseIPv6(value: string): number[] | null {
  const addr = value.split('%')[0] ?? ''
  const halves = addr.split('::')
  if (halves.length > 2) return null
  const head = expandEmbeddedIpv4(halves[0] ? halves[0].split(':') : [])
  const tail = expandEmbeddedIpv4(halves[1] ? halves[1].split(':') : [])
  if (!head || !tail) return null
  const missing = 8 - head.length - tail.length
  if (halves.length === 1 ? missing !== 0 : missing < 1) return null
  const groups = [...head, ...Array<string>(halves.length === 2 ? missing : 0).fill('0'), ...tail]
  if (!groups.every((g) => HEXTET_RE.test(g))) return null
  return groups.map((g) => parseInt(g, 16))
}

export function isIPv6(value: string): boolean {
  return value.includes(':') && parseIPv6(value) !== null
}

export function isIP(value: string): boolean {
  return isIPv4(value) || isIPv6(value)
}

/** Validates `addr/prefix` for both families. */
export function isCIDR(value: string): boolean {
  const parts = value.split('/')
  if (parts.length !== 2) return false
  const [addr = '', prefixText = ''] = parts
  if (!/^\d{1,3}$/.test(prefixText)) return false
  const prefix = Number(prefixText)
  if (isIPv4(addr)) return prefix <= 32
  if (isIPv6(addr)) return prefix <= 128
  return false
}

/** RFC 5737 TEST-NET ranges — never routed, never have geo data (matches backend `is_private`). */
function isDocumentationV4(a: number, b: number, c: number): boolean {
  return (
    (a === 192 && b === 0 && c === 2) ||
    (a === 198 && b === 51 && c === 100) ||
    (a === 203 && b === 0 && c === 113)
  )
}

function ipv4IsPrivate(value: string): boolean {
  const [a = 0, b = 0] = ipv4ToBytes(value)
  return (
    a === 10 ||
    a === 127 ||
    a === 0 ||
    (a === 172 && b >= 16 && b <= 31) ||
    (a === 192 && b === 168) ||
    (a === 169 && b === 254) ||
    (a === 100 && b >= 64 && b <= 127) ||
    isDocumentationV4(a, b, ipv4ToBytes(value)[2] ?? 0) ||
    a >= 224
  )
}

function ipv6IsPrivate(groups: readonly number[]): boolean {
  const first = groups[0] ?? 0
  const allZeroHead = groups.slice(0, 7).every((g) => g === 0)
  if (allZeroHead && (groups[7] === 0 || groups[7] === 1)) return true // :: and ::1
  if ((first & 0xfe00) === 0xfc00) return true // fc00::/7 ULA
  if ((first & 0xffc0) === 0xfe80) return true // fe80::/10 link-local
  if ((first & 0xff00) === 0xff00) return true // multicast
  if (first === 0x2001 && groups[1] === 0x0db8) return true // 2001:db8::/32 documentation
  const mapped = groups.slice(0, 5).every((g) => g === 0) && groups[5] === 0xffff
  if (mapped) {
    const hi = groups[6] ?? 0
    const lo = groups[7] ?? 0
    return ipv4IsPrivate(`${hi >> 8}.${hi & 0xff}.${lo >> 8}.${lo & 0xff}`)
  }
  return false
}

/** True for RFC1918, CGNAT, loopback, link-local, multicast, ULA and similar non-public ranges. */
export function isPrivateIp(value: string): boolean {
  if (isIPv4(value)) return ipv4IsPrivate(value)
  const groups = parseIPv6(value)
  return groups ? ipv6IsPrivate(groups) : false
}
