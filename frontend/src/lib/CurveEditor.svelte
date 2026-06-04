<script lang="ts">
  import type { CurvePoint } from './types'

  interface Props {
    points: CurvePoint[]
    /** Current control temperature, drawn as a live marker. */
    liveTemp?: number | null
    /** Current fan output percent, drawn on the live marker. */
    livePwm?: number | null
    disabled?: boolean
    /** Fixed number of points: adding and removing is not allowed. */
    fixedPoints?: boolean
    /** Duty may never decrease with temperature (Smart Fan IV constraint). */
    monotonicPwm?: boolean
    onchange: (points: CurvePoint[]) => void
  }

  let {
    points,
    liveTemp = null,
    livePwm = null,
    disabled = false,
    fixedPoints = false,
    monotonicPwm = false,
    onchange,
  }: Props = $props()

  // Chart geometry
  const W = 720
  const H = 320
  const PAD = { left: 46, right: 16, top: 14, bottom: 34 }
  const T_MIN = 20
  const T_MAX = 100
  // Fans start spinning around raw 40/255
  const SPIN_PCT = (40 / 255) * 100

  const x = (t: number) => PAD.left + ((t - T_MIN) / (T_MAX - T_MIN)) * (W - PAD.left - PAD.right)
  const y = (p: number) => H - PAD.bottom - (p / 100) * (H - PAD.top - PAD.bottom)
  const invX = (px: number) => T_MIN + ((px - PAD.left) / (W - PAD.left - PAD.right)) * (T_MAX - T_MIN)
  const invY = (py: number) => ((H - PAD.bottom - py) / (H - PAD.top - PAD.bottom)) * 100

  let svgEl: SVGSVGElement
  let dragIndex: number | null = $state(null)
  let hoverIndex: number | null = $state(null)

  const tTicks = [20, 30, 40, 50, 60, 70, 80, 90, 100]
  const pTicks = [0, 25, 50, 75, 100]

  // Curve path including clamped extension to the chart edges
  let path = $derived.by(() => {
    if (points.length === 0) return ''
    const first = points[0]
    const last = points[points.length - 1]
    let d = `M ${x(T_MIN)} ${y(first.pwm)}`
    for (const p of points) d += ` L ${x(p.temp)} ${y(p.pwm)}`
    d += ` L ${x(T_MAX)} ${y(last.pwm)}`
    return d
  })

  let fillPath = $derived(
    path ? `${path} L ${x(T_MAX)} ${y(0)} L ${x(T_MIN)} ${y(0)} Z` : '',
  )

  /** Linear interpolation of the curve at a temperature (mirrors the server). */
  function curveAt(t: number): number {
    if (points.length === 0) return 0
    if (t <= points[0].temp) return points[0].pwm
    for (let i = 1; i < points.length; i++) {
      if (t <= points[i].temp) {
        const a = points[i - 1]
        const b = points[i]
        if (b.temp === a.temp) return b.pwm
        return a.pwm + ((b.pwm - a.pwm) * (t - a.temp)) / (b.temp - a.temp)
      }
    }
    return points[points.length - 1].pwm
  }

  function svgCoords(e: PointerEvent | MouseEvent): { px: number; py: number } {
    const rect = svgEl.getBoundingClientRect()
    return {
      px: ((e.clientX - rect.left) / rect.width) * W,
      py: ((e.clientY - rect.top) / rect.height) * H,
    }
  }

  function clamp(v: number, lo: number, hi: number) {
    return Math.min(hi, Math.max(lo, v))
  }

  function startDrag(e: PointerEvent, i: number) {
    if (disabled) return
    e.preventDefault()
    dragIndex = i
    ;(e.currentTarget as Element).setPointerCapture(e.pointerId)
  }

  function onPointerMove(e: PointerEvent) {
    if (dragIndex === null || disabled) return
    const { px, py } = svgCoords(e)
    const i = dragIndex
    // Keep points ordered: clamp temp between neighbours (with 1°C gap)
    const lo = i > 0 ? points[i - 1].temp + 1 : T_MIN
    const hi = i < points.length - 1 ? points[i + 1].temp - 1 : T_MAX
    // Smart Fan IV: duty must never decrease along the curve
    const pwmLo = monotonicPwm && i > 0 ? points[i - 1].pwm : 0
    const pwmHi = monotonicPwm && i < points.length - 1 ? points[i + 1].pwm : 100
    const next = points.map((p, j) =>
      j === i
        ? {
            temp: Math.round(clamp(invX(px), lo, hi)),
            pwm: Math.round(clamp(invY(py), pwmLo, pwmHi)),
          }
        : p,
    )
    onchange(next)
  }

  function endDrag(e: PointerEvent) {
    if (dragIndex !== null) {
      ;(e.currentTarget as Element).releasePointerCapture?.(e.pointerId)
      dragIndex = null
    }
  }

  function addPoint(e: MouseEvent) {
    if (disabled || fixedPoints) return
    const { px, py } = svgCoords(e)
    const temp = Math.round(clamp(invX(px), T_MIN, T_MAX))
    const pwm = Math.round(clamp(invY(py), 0, 100))
    if (points.some((p) => Math.abs(p.temp - temp) < 1)) return
    const next = [...points, { temp, pwm }].sort((a, b) => a.temp - b.temp)
    onchange(next)
  }

  function removePoint(e: MouseEvent, i: number) {
    e.stopPropagation()
    if (disabled || fixedPoints || points.length <= 2) return
    onchange(points.filter((_, j) => j !== i))
  }
</script>

<div class="editor" class:disabled>
  <svg
    bind:this={svgEl}
    viewBox="0 0 {W} {H}"
    role="application"
    aria-label="Fan curve editor"
    ondblclick={addPoint}
  >
    <!-- grid -->
    {#each tTicks as t}
      <line class="grid" x1={x(t)} y1={y(0)} x2={x(t)} y2={y(100)} />
      <text class="tick" x={x(t)} y={H - PAD.bottom + 18} text-anchor="middle">{t}°</text>
    {/each}
    {#each pTicks as p}
      <line class="grid" x1={x(T_MIN)} y1={y(p)} x2={x(T_MAX)} y2={y(p)} />
      <text class="tick" x={PAD.left - 8} y={y(p) + 4} text-anchor="end">{p}%</text>
    {/each}

    <!-- minimum spin-up duty hint -->
    <line class="spin-hint" x1={x(T_MIN)} y1={y(SPIN_PCT)} x2={x(T_MAX)} y2={y(SPIN_PCT)} />
    <text class="spin-label" x={x(T_MAX) - 4} y={y(SPIN_PCT) - 5} text-anchor="end">
      spin-up ≈{SPIN_PCT.toFixed(0)}%
    </text>

    <!-- curve -->
    <path class="fill" d={fillPath} />
    <path class="curve" d={path} />

    <!-- live operating point -->
    {#if liveTemp !== null && liveTemp >= T_MIN && liveTemp <= T_MAX}
      <line class="live-line" x1={x(liveTemp)} y1={y(0)} x2={x(liveTemp)} y2={y(100)} />
      <text class="live-label" x={x(liveTemp)} y={PAD.top + 2} text-anchor="middle">
        {liveTemp.toFixed(1)}°C
      </text>
      <circle class="live-dot" cx={x(liveTemp)} cy={y(livePwm ?? curveAt(liveTemp))} r="5" />
    {/if}

    <!-- draggable points -->
    {#each points as p, i}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <g
        class="point"
        class:dragging={dragIndex === i}
        onpointerdown={(e) => startDrag(e, i)}
        onpointermove={onPointerMove}
        onpointerup={endDrag}
        onpointercancel={endDrag}
        ondblclick={(e) => removePoint(e, i)}
        onmouseenter={() => (hoverIndex = i)}
        onmouseleave={() => (hoverIndex = null)}
      >
        <circle class="hit" cx={x(p.temp)} cy={y(p.pwm)} r="14" />
        <circle class="dot" cx={x(p.temp)} cy={y(p.pwm)} r="6" />
        {#if hoverIndex === i || dragIndex === i}
          <text
            class="point-label"
            x={x(p.temp)}
            y={y(p.pwm) - 12}
            text-anchor="middle"
          >{p.temp}°C → {p.pwm}%</text>
        {/if}
      </g>
    {/each}
  </svg>
  {#if fixedPoints}
    <p class="hint">Drag the {points.length} points to shape the curve · the chip supports exactly {points.length} points and the duty can only rise with temperature</p>
  {:else}
    <p class="hint">Drag points to shape the curve · double-click empty space to add · double-click a point to remove</p>
  {/if}
</div>

<style>
  .editor {
    user-select: none;
  }
  .editor.disabled {
    opacity: 0.45;
    pointer-events: none;
  }
  svg {
    width: 100%;
    height: auto;
    display: block;
    touch-action: none;
    background: var(--chart-bg, #11151c);
    border-radius: 8px;
  }
  .grid {
    stroke: #2a3140;
    stroke-width: 1;
  }
  .tick {
    fill: #7d8799;
    font-size: 11px;
  }
  .spin-hint {
    stroke: #b3823a;
    stroke-width: 1;
    stroke-dasharray: 5 4;
    opacity: 0.7;
  }
  .spin-label {
    fill: #b3823a;
    font-size: 10px;
    opacity: 0.9;
  }
  .curve {
    fill: none;
    stroke: #4da3ff;
    stroke-width: 2.5;
    stroke-linejoin: round;
  }
  .fill {
    fill: #4da3ff18;
    stroke: none;
  }
  .live-line {
    stroke: #ff9c41;
    stroke-width: 1.5;
    stroke-dasharray: 3 3;
  }
  .live-label {
    fill: #ff9c41;
    font-size: 11px;
    dominant-baseline: hanging;
  }
  .live-dot {
    fill: #ff9c41;
    stroke: #11151c;
    stroke-width: 2;
  }
  .point {
    cursor: grab;
  }
  .point.dragging {
    cursor: grabbing;
  }
  .point .hit {
    fill: transparent;
  }
  .point .dot {
    fill: #dbe6f5;
    stroke: #4da3ff;
    stroke-width: 2.5;
    transition: r 0.1s;
  }
  .point:hover .dot,
  .point.dragging .dot {
    fill: #4da3ff;
    stroke: #dbe6f5;
  }
  .point-label {
    fill: #dbe6f5;
    font-size: 12px;
    font-weight: 600;
    paint-order: stroke;
    stroke: #11151c;
    stroke-width: 4px;
  }
  .hint {
    margin: 6px 2px 0;
    font-size: 12px;
    color: #7d8799;
  }
</style>
