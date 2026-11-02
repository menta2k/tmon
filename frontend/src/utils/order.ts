/** Returns a new array with the item at `from` moved to index `to`. */
export function moveItem<T>(items: readonly T[], from: number, to: number): T[] {
  if (from === to || from < 0 || from >= items.length || to < 0 || to >= items.length) return [...items]
  const without = [...items.slice(0, from), ...items.slice(from + 1)]
  const item = items[from] as T
  return [...without.slice(0, to), item, ...without.slice(to)]
}

/** Sorts rules (or anything with a priority) highest priority first, without mutating. */
export function byPriorityDesc<T extends { priority: number }>(items: readonly T[]): T[] {
  return [...items].sort((a, b) => b.priority - a.priority)
}
