//! The control loop: periodically read temperatures and drive the fans
//! according to the active configuration.

use std::sync::Arc;

use serde::Serialize;
use tokio::sync::RwLock;

use crate::config::{pct_to_raw, Config, CurvePoint, FanMode};
use crate::hwmon::Hardware;

#[derive(Debug, Clone, Serialize, Default)]
pub struct SensorStatus {
    pub label: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub control: bool,
    pub temp: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct FanStatus {
    pub index: u32,
    pub rpm: Option<u32>,
    pub pwm_raw: Option<u32>,
    pub pwm_pct: Option<f64>,
    pub enable: Option<u32>,
    pub mode: Option<FanMode>,
    /// The duty the controller is targeting (None in auto/hardware modes,
    /// where the chip runs the loop itself).
    pub target_pct: Option<f64>,
    /// How many Smart Fan IV curve points the chip supports for this fan.
    pub hw_points: usize,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Status {
    pub sensors: Vec<SensorStatus>,
    /// max() over the control sensors (CPU package) — the temperature the
    /// curves are evaluated at.
    pub control_temp: Option<f64>,
    pub fans: Vec<FanStatus>,
}

pub struct AppState {
    pub hw: Hardware,
    pub config: RwLock<Config>,
    pub status: RwLock<Status>,
}

pub type SharedState = Arc<AppState>;

/// Per-fan bookkeeping so we only touch sysfs when something changes.
#[derive(Default, Clone)]
struct FanRuntime {
    last_mode: Option<FanMode>,
    last_raw: Option<u8>,
    /// The hardware curve last programmed into the chip.
    last_hw_curve: Option<Vec<CurvePoint>>,
}

pub async fn run(state: SharedState) {
    let mut runtimes = vec![FanRuntime::default(); state.hw.fans.len()];

    loop {
        let interval_ms = {
            let config = state.config.read().await;
            tick(&state, &config, &mut runtimes).await;
            config.poll_interval_ms
        };
        tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;
    }
}

async fn tick(state: &SharedState, config: &Config, runtimes: &mut [FanRuntime]) {
    // --- read temperatures ---
    let sensors: Vec<SensorStatus> = state
        .hw
        .sensors
        .iter()
        .map(|s| SensorStatus {
            label: s.label.clone(),
            kind: s.kind.to_string(),
            model: s.model.clone(),
            control: s.control,
            temp: s.read().map_err(|e| tracing::warn!("{e:#}")).ok(),
        })
        .collect();
    let control_temp = sensors
        .iter()
        .filter(|s| s.control)
        .filter_map(|s| s.temp)
        .fold(None, |acc: Option<f64>, t| Some(acc.map_or(t, |a| a.max(t))));

    // --- drive fans ---
    let mut fan_statuses = Vec::with_capacity(state.hw.fans.len());
    for (i, fan) in state.hw.fans.iter().enumerate() {
        let fan_cfg = config.fans.get(i);
        let mode = fan_cfg.map(|c| c.mode);
        let rt = &mut runtimes[i];

        let target_pct = match (fan_cfg, mode) {
            (Some(_), Some(FanMode::Auto | FanMode::Hardware)) | (None, _) => None,
            (Some(cfg), Some(FanMode::Manual)) => Some(cfg.manual_pwm),
            (Some(cfg), Some(FanMode::Curve)) => match control_temp {
                Some(t) => Some(cfg.curve_pwm_at(t)),
                // Failsafe: no readable temperature -> full speed.
                None => {
                    tracing::error!("no temperature available, fan {} to 100%", fan.index);
                    Some(100.0)
                }
            },
            (Some(_), None) => None,
        };

        // Apply mode transitions only when they change. Hardware mode is
        // handled below since it must also react to curve edits.
        if mode != rt.last_mode {
            let result = match mode {
                Some(FanMode::Auto) | None => fan.set_auto(),
                Some(FanMode::Hardware) => Ok(()),
                Some(_) => fan.set_manual(),
            };
            match result {
                Ok(()) => {
                    tracing::info!("fan {}: mode -> {:?}", fan.index, mode);
                    rt.last_mode = mode;
                    rt.last_raw = None;
                    rt.last_hw_curve = None;
                }
                Err(e) => tracing::error!("fan {}: mode change failed: {e:#}", fan.index),
            }
        }

        // Program the chip's Smart Fan IV engine when entering hardware mode
        // or whenever the hardware curve changes.
        if let (Some(cfg), Some(FanMode::Hardware)) = (fan_cfg, mode) {
            if rt.last_hw_curve.as_ref() != Some(&cfg.hw_curve) {
                let points: Vec<(i64, u8)> = cfg
                    .hw_points(fan.auto_point_count())
                    .iter()
                    .map(|p| ((p.temp * 1000.0).round() as i64, pct_to_raw(p.pwm)))
                    .collect();
                match fan.set_hardware_curve(&points) {
                    Ok(()) => {
                        tracing::info!(
                            "fan {}: hardware curve programmed: {points:?}",
                            fan.index
                        );
                        rt.last_hw_curve = Some(cfg.hw_curve.clone());
                    }
                    Err(e) => {
                        tracing::error!("fan {}: hardware curve failed: {e:#}", fan.index)
                    }
                }
            }
        }

        if let Some(pct) = target_pct {
            let raw = pct_to_raw(pct);
            if rt.last_raw != Some(raw) {
                match fan.set_pwm_raw(raw) {
                    Ok(()) => rt.last_raw = Some(raw),
                    Err(e) => tracing::error!("fan {}: pwm write failed: {e:#}", fan.index),
                }
            }
        }

        let pwm_raw = fan.pwm_raw().ok();
        fan_statuses.push(FanStatus {
            index: fan.index,
            rpm: fan.rpm().ok(),
            pwm_raw,
            pwm_pct: pwm_raw.map(|v| (v as f64 / 255.0 * 1000.0).round() / 10.0),
            enable: fan.enable_value().ok(),
            mode,
            target_pct,
            hw_points: fan.auto_point_count(),
        });
    }

    *state.status.write().await = Status {
        sensors,
        control_temp,
        fans: fan_statuses,
    };
}
