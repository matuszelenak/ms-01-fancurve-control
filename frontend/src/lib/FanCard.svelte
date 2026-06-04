<script lang="ts">
  import CurveEditor from './CurveEditor.svelte'
  import type { FanConfig, FanStatus, FanMode } from './types'

  interface Props {
    index: number
    config: FanConfig
    status: FanStatus | null
    liveTemp: number | null
    onchange: (config: FanConfig) => void
  }

  let { index, config, status, liveTemp, onchange }: Props = $props()

  const modes: { value: FanMode; label: string; hint: string }[] = [
    { value: 'auto', label: 'Auto', hint: 'firmware curve, as set by the BIOS' },
    { value: 'curve', label: 'SW curve', hint: 'daemon follows the curve below (CPU package temp)' },
    {
      value: 'hardware',
      label: 'HW curve',
      hint: 'curve programmed into the fan chip — keeps running even if the daemon or OS dies',
    },
    { value: 'manual', label: 'Manual', hint: 'fixed speed' },
  ]

  function setMode(mode: FanMode) {
    onchange({ ...config, mode })
  }
</script>

<section class="card">
  <header>
    <h2>Fan {index}</h2>
    <div class="stats">
      <span class="stat">
        <span class="value">{status?.rpm ?? '—'}</span>
        <span class="unit">RPM</span>
      </span>
      <span class="stat">
        <span class="value">{status?.pwm_pct != null ? status.pwm_pct.toFixed(0) : '—'}</span>
        <span class="unit">% PWM</span>
      </span>
      {#if config.mode !== 'auto' && status?.target_pct != null}
        <span class="stat">
          <span class="value">{status.target_pct.toFixed(0)}</span>
          <span class="unit">% target</span>
        </span>
      {/if}
    </div>
  </header>

  <div class="modes" role="radiogroup" aria-label="Fan {index} mode">
    {#each modes as m}
      <button
        class="mode"
        class:active={config.mode === m.value}
        role="radio"
        aria-checked={config.mode === m.value}
        title={m.hint}
        onclick={() => setMode(m.value)}
      >
        {m.label}
      </button>
    {/each}
  </div>

  {#if config.mode === 'manual'}
    <div class="manual">
      <input
        type="range"
        min="0"
        max="100"
        step="1"
        value={config.manual_pwm}
        oninput={(e) => onchange({ ...config, manual_pwm: +e.currentTarget.value })}
      />
      <span class="manual-value">{config.manual_pwm.toFixed(0)}%</span>
    </div>
  {/if}

  {#if config.mode === 'hardware'}
    <p class="hw-note">
      The curve below runs inside the nct6798's Smart Fan IV engine — limited to
      {status?.hw_points ?? 5} points with non-decreasing duty, driven by the
      board's CPU sensor (may read a few °C below the package temperature shown).
    </p>
    <CurveEditor
      points={config.hw_curve}
      {liveTemp}
      livePwm={status?.pwm_pct ?? null}
      fixedPoints
      monotonicPwm
      onchange={(hw_curve) => onchange({ ...config, hw_curve })}
    />
  {:else}
    <CurveEditor
      points={config.curve}
      {liveTemp}
      livePwm={config.mode === 'curve' ? (status?.target_pct ?? null) : null}
      disabled={config.mode !== 'curve'}
      onchange={(curve) => onchange({ ...config, curve })}
    />
  {/if}
</section>

<style>
  .card {
    background: #1a2029;
    border: 1px solid #2a3140;
    border-radius: 12px;
    padding: 18px 20px;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 12px;
  }
  h2 {
    margin: 0;
    font-size: 18px;
  }
  .stats {
    display: flex;
    gap: 18px;
  }
  .stat .value {
    font-size: 20px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    color: #dbe6f5;
  }
  .stat .unit {
    font-size: 12px;
    color: #7d8799;
    margin-left: 3px;
  }
  .modes {
    display: inline-flex;
    background: #11151c;
    border: 1px solid #2a3140;
    border-radius: 8px;
    padding: 3px;
    margin-bottom: 12px;
  }
  .mode {
    border: none;
    background: transparent;
    color: #9aa7ba;
    font: inherit;
    font-size: 14px;
    padding: 6px 18px;
    border-radius: 6px;
    cursor: pointer;
  }
  .mode:hover {
    color: #dbe6f5;
  }
  .mode.active {
    background: #4da3ff;
    color: #0c1117;
    font-weight: 600;
  }
  .manual {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 12px;
  }
  .manual input[type='range'] {
    flex: 1;
    accent-color: #4da3ff;
  }
  .manual-value {
    font-variant-numeric: tabular-nums;
    font-weight: 700;
    min-width: 48px;
    text-align: right;
  }
  .hw-note {
    margin: 0 0 10px;
    font-size: 12.5px;
    line-height: 1.45;
    color: #9aa7ba;
    background: #11151c;
    border: 1px solid #2a3140;
    border-left: 3px solid #4da3ff;
    border-radius: 6px;
    padding: 8px 12px;
  }
</style>
