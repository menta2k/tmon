/** Saves a JSON value as a file download in the browser. */
export function downloadJson(value: unknown, filename: string): void {
  const blob = new Blob([JSON.stringify(value, null, 2)], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  triggerDownload(url, filename)
  URL.revokeObjectURL(url)
}

/** Navigates an invisible anchor to `url`; the same-origin session cookie is sent. */
export function triggerDownload(url: string, filename?: string): void {
  const a = document.createElement('a')
  a.href = url
  if (filename) a.download = filename
  a.rel = 'noopener'
  document.body.appendChild(a)
  a.click()
  a.remove()
}

/** Reads a user-selected file as JSON; throws a friendly Error when it is not valid JSON. */
export async function readJsonFile(file: Blob): Promise<unknown> {
  const text = await readText(file)
  try {
    return JSON.parse(text) as unknown
  } catch {
    throw new Error('The selected file is not valid JSON')
  }
}

function readText(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result ?? ''))
    reader.onerror = () => reject(new Error('The selected file could not be read'))
    reader.readAsText(file)
  })
}
