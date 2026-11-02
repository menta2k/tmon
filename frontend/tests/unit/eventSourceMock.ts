/** Minimal EventSource double: records instances and lets tests emit events. */
export class MockEventSource {
  static instances: MockEventSource[] = []
  readonly url: string
  closed = false
  private listeners = new Map<string, ((ev: Event) => void)[]>()

  constructor(url: string) {
    this.url = url
    MockEventSource.instances = [...MockEventSource.instances, this]
  }

  addEventListener(type: string, fn: (ev: Event) => void): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), fn])
  }

  close(): void {
    this.closed = true
  }

  emit(type: string, data?: unknown): void {
    const ev =
      data === undefined
        ? new Event(type)
        : new MessageEvent(type, { data: typeof data === 'string' ? data : JSON.stringify(data) })
    for (const fn of this.listeners.get(type) ?? []) fn(ev)
  }

  static latest(): MockEventSource {
    const last = MockEventSource.instances.at(-1)
    if (!last) throw new Error('no EventSource created')
    return last
  }

  static reset(): void {
    MockEventSource.instances = []
  }
}
