<script lang="ts">
  import { onMount } from 'svelte'
  import FanCard from './lib/FanCard.svelte'
  import { fetchConfig, fetchStatus, saveConfig } from './lib/api'
  import type { Config, FanConfig, Status } from './lib/types'

  let config: Config | null = $state(null)
  let savedConfig = $state('') // JSON snapshot of the last saved config
  let status: Status | null = $state(null)
  let error: string | null = $state(null)
  let saving = $state(false)

  let dirty = $derived(config !== null && JSON.stringify(config) !== savedConfig)

  onMount(() => {
    fetchConfig()
      .then((c) => {
        config = c
        savedConfig = JSON.stringify(c)
      })
      .catch((e) => (error = String(e)))

    const poll = async () => {
      try {
        status = await fetchStatus()
        error = null
      } catch (e) {
        error = String(e)
      }
    }
    poll()
    const timer = setInterval(poll, 2000)
    return () => clearInterval(timer)
  })

  function updateFan(i: number, fan: FanConfig) {
    if (!config) return
    const fans = config.fans.map((f, j) => (j === i ? fan : f))
    config = { ...config, fans }
  }

  async function save() {
    if (!config) return
    saving = true
    try {
      const applied = await saveConfig(config)
      config = applied
      savedConfig = JSON.stringify(applied)
      error = null
    } catch (e) {
      error = String(e)
    } finally {
      saving = false
    }
  }

  function revert() {
    if (savedConfig) config = JSON.parse(savedConfig)
  }

  function tempClass(t: number | null): string {
    if (t == null) return ''
    if (t >= 80) return 'hot'
    if (t >= 65) return 'warm'
    return 'cool'
  }

  let cpuPackage = $derived(status?.sensors.find((s) => s.kind === 'cpu') ?? null)
  let cores = $derived(status?.sensors.filter((s) => s.kind === 'core') ?? [])
  let nvmes = $derived(status?.sensors.filter((s) => s.kind === 'nvme') ?? [])
</script>

<main>
  <header class="top">
    <h1>MS-01 Fan Control</h1>
    <div class="actions">
      {#if dirty}
        <span class="dirty-badge">unsaved changes</span>
        <button class="secondary" onclick={revert}>Revert</button>
      {/if}
      <button class="primary" disabled={!dirty || saving} onclick={save}>
        {saving ? 'Saving…' : 'Save & Apply'}
      </button>
    </div>
  </header>

  {#if error}
    <div class="error">{error}</div>
  {/if}

  <section class="sensor-group">
    <h3>CPU <span class="group-note">controls the fans</span></h3>
    <div class="temps">
      {#if cpuPackage}
        <div class="temp control {tempClass(cpuPackage.temp)}">
          <span class="label">{cpuPackage.label}</span>
          <span class="value">{cpuPackage.temp != null ? cpuPackage.temp.toFixed(1) + '°C' : '—'}</span>
        </div>
      {/if}
      {#each cores as s}
        <div class="temp small {tempClass(s.temp)}">
          <span class="label">{s.label}</span>
          <span class="value">{s.temp != null ? s.temp.toFixed(0) + '°' : '—'}</span>
        </div>
      {/each}
    </div>
  </section>

  {#if nvmes.length > 0}
    <section class="sensor-group">
      <h3>NVMe drives <span class="group-note">display only</span></h3>
      <div class="temps">
        {#each nvmes as s}
          <div class="temp {tempClass(s.temp)}">
            <span class="label" title={s.model ?? s.label}>{s.model ?? s.label}</span>
            <span class="sublabel">{s.label}</span>
            <span class="value">{s.temp != null ? s.temp.toFixed(1) + '°C' : '—'}</span>
          </div>
        {/each}
      </div>
    </section>
  {/if}

  {#if config}
    <div class="fans">
      {#each config.fans as fan, i}
        <FanCard
          index={i + 1}
          config={fan}
          status={status?.fans[i] ?? null}
          liveTemp={status?.control_temp ?? null}
          onchange={(f) => updateFan(i, f)}
        />
      {/each}
    </div>
  {:else if !error}
    <p class="loading">Loading configuration…</p>
  {/if}
</main>

<style>
  main {
    max-width: 880px;
    margin: 0 auto;
    padding: 20px 16px 48px;
  }
  .top {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
    margin-bottom: 16px;
  }
  h1 {
    margin: 0;
    font-size: 22px;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .dirty-badge {
    font-size: 12px;
    color: #ffb454;
  }
  button {
    font: inherit;
    border-radius: 8px;
    padding: 8px 18px;
    cursor: pointer;
    border: 1px solid #2a3140;
  }
  button.primary {
    background: #4da3ff;
    border-color: #4da3ff;
    color: #0c1117;
    font-weight: 600;
  }
  button.primary:disabled {
    background: #2a3140;
    border-color: #2a3140;
    color: #7d8799;
    cursor: default;
  }
  button.secondary {
    background: transparent;
    color: #9aa7ba;
  }
  button.secondary:hover {
    color: #dbe6f5;
  }
  .error {
    background: #3a1d1d;
    border: 1px solid #7c3030;
    color: #ff9c9c;
    border-radius: 8px;
    padding: 10px 14px;
    margin-bottom: 14px;
    font-size: 14px;
  }
  .sensor-group {
    margin-bottom: 18px;
  }
  .sensor-group h3 {
    margin: 0 0 8px;
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: #9aa7ba;
  }
  .group-note {
    text-transform: none;
    letter-spacing: 0;
    font-weight: 400;
    font-size: 12px;
    color: #7d8799;
    margin-left: 6px;
  }
  .temps {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    align-items: stretch;
  }
  .temp {
    background: #1a2029;
    border: 1px solid #2a3140;
    border-radius: 10px;
    padding: 8px 14px;
    display: flex;
    flex-direction: column;
    min-width: 110px;
  }
  .temp.small {
    min-width: 56px;
    padding: 5px 10px;
  }
  .temp.small .value {
    font-size: 14px;
  }
  .temp.control {
    border-color: #ff9c41;
  }
  .temp .label {
    font-size: 11px;
    color: #7d8799;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 200px;
  }
  .temp .sublabel {
    font-size: 10px;
    color: #5b6577;
    white-space: nowrap;
  }
  .temp .value {
    font-size: 18px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
  .temp.cool .value {
    color: #6fd388;
  }
  .temp.warm .value {
    color: #ffb454;
  }
  .temp.hot .value {
    color: #ff6b6b;
  }
  .fans {
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  .loading {
    color: #7d8799;
  }
</style>
