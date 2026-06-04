export interface CurvePoint {
  temp: number
  pwm: number
}

export type FanMode = 'auto' | 'curve' | 'manual'

export interface FanConfig {
  mode: FanMode
  manual_pwm: number
  curve: CurvePoint[]
}

export interface Config {
  poll_interval_ms: number
  fans: FanConfig[]
}

export interface SensorStatus {
  label: string
  kind: 'cpu' | 'core' | 'nvme' | string
  model?: string
  control: boolean
  temp: number | null
}

export interface FanStatus {
  index: number
  rpm: number | null
  pwm_raw: number | null
  pwm_pct: number | null
  enable: number | null
  mode: FanMode | null
  target_pct: number | null
}

export interface Status {
  sensors: SensorStatus[]
  control_temp: number | null
  fans: FanStatus[]
}
