import type { Config, Status } from './types'

async function check(res: Response): Promise<Response> {
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText)
    throw new Error(`${res.status}: ${text}`)
  }
  return res
}

export async function fetchStatus(): Promise<Status> {
  return (await check(await fetch('/api/status'))).json()
}

export async function fetchConfig(): Promise<Config> {
  return (await check(await fetch('/api/config'))).json()
}

export async function saveConfig(config: Config): Promise<Config> {
  return (
    await check(
      await fetch('/api/config', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(config),
      }),
    )
  ).json()
}
