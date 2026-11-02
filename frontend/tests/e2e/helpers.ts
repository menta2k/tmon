/**
 * Shared E2E helpers. Specs run against an already-running tmon backend:
 *   TMON_E2E_BASE_URL     (default http://localhost:8080)
 *   TMON_E2E_PASSWORD     (default changeme)
 *   TMON_E2E_REPLAY_CMD   e.g. "../backend/target/debug/tmon replay {file} --to {to} --speed max"
 *                         ({to} is the NetFlow or sFlow collector, chosen per fixture)
 *   TMON_E2E_NETFLOW_TO   (default 127.0.0.1:2055)
 *   TMON_E2E_SFLOW_TO     (default 127.0.0.1:6343)
 *   TMON_E2E_FIXTURES_DIR (default ../backend/tests/fixtures, relative to frontend/)
 */
import { execSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import path from 'node:path'
import { expect, test, type APIResponse, type Page } from '@playwright/test'

export const PASSWORD = process.env.TMON_E2E_PASSWORD ?? 'changeme'
const REPLAY_CMD = process.env.TMON_E2E_REPLAY_CMD ?? ''
const FIXTURES_DIR = path.resolve(
  process.cwd(),
  process.env.TMON_E2E_FIXTURES_DIR ?? '../backend/tests/fixtures',
)
const REPLAY_TIMEOUT_MS = 180_000
const NETFLOW_TO = process.env.TMON_E2E_NETFLOW_TO ?? '127.0.0.1:2055'
const SFLOW_TO = process.env.TMON_E2E_SFLOW_TO ?? '127.0.0.1:6343'

export const FIXTURES = {
  ipfix: 'routeros7-ipfix.pcap',
  sflow: 'sflow-crs-1in512.pcap',
  mixed: 'mixed-home.pcap',
  steam: 'steam.pcap',
} as const

/** Skips the current test when no replay command is configured. */
export function requireReplay(): void {
  test.skip(!REPLAY_CMD, 'TMON_E2E_REPLAY_CMD is not set; cannot replay fixtures')
}

function shellQuote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`
}

/** Replays a pcap fixture into the collector using TMON_E2E_REPLAY_CMD ({file} is substituted). */
export function replayFixture(name: string): void {
  requireReplay()
  const file = path.join(FIXTURES_DIR, name)
  if (!existsSync(file)) throw new Error(`Fixture not found: ${file}`)
  const to = name.startsWith('sflow') ? SFLOW_TO : NETFLOW_TO
  const template = REPLAY_CMD.replaceAll('{to}', shellQuote(to))
  const command = template.includes('{file}')
    ? template.replaceAll('{file}', shellQuote(file))
    : `${template} ${shellQuote(file)}`
  try {
    execSync(command, { stdio: 'pipe', timeout: REPLAY_TIMEOUT_MS })
  } catch (err) {
    const stderr = (err as { stderr?: Buffer }).stderr?.toString() ?? ''
    throw new Error(`Replay failed (${command}): ${stderr || String(err)}`)
  }
}

/** Logs in through the UI and waits for the dashboard (or the redirect target). */
export async function login(page: Page): Promise<void> {
  await page.goto('/login')
  await page.getByTestId('login-password').locator('input').fill(PASSWORD)
  await page.getByTestId('login-submit').click()
  await page.waitForURL((url) => !url.pathname.startsWith('/login'))
}

/** Picks a preset from the global time range selector (e.g. "Last 1 h"). */
export async function selectTimeRange(page: Page, label: string): Promise<void> {
  await page.getByTestId('time-range-select').click()
  await page.locator('.n-base-select-option', { hasText: label }).first().click()
}

/** Types a value into a Naive UI tag/filterable select and confirms it with Enter. */
export async function addSelectTag(page: Page, testId: string, value: string): Promise<void> {
  const select = page.getByTestId(testId)
  await select.click()
  await select.locator('input').last().fill(value)
  await page.keyboard.press('Enter')
  await page.keyboard.press('Escape')
}

interface Envelope<T> {
  ok: boolean
  data: T
}

/** Authenticated API call reusing the page's session cookie; asserts the envelope is ok. */
export async function apiData<T>(response: Promise<APIResponse>): Promise<T> {
  const res = await response
  expect(res.ok(), `${res.url()} → ${res.status()}`).toBeTruthy()
  const body = (await res.json()) as Envelope<T>
  expect(body.ok).toBe(true)
  return body.data
}
