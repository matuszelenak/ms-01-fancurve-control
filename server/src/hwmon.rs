//! Discovery and access of hwmon sysfs devices on the MS-01.
//!
//! Devices are located by chip name (`/sys/class/hwmon/hwmon*/name`) rather
//! than by index, since hwmon numbering can change between boots.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

const HWMON_ROOT: &str = "/sys/class/hwmon";

/// Find all hwmon device directories whose `name` matches a predicate.
fn find_chips(pred: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(HWMON_ROOT) else {
        return out;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if let Ok(name) = fs::read_to_string(dir.join("name")) {
            if pred(name.trim()) {
                out.push(dir);
            }
        }
    }
    out.sort();
    out
}

fn read_trimmed(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?
        .trim()
        .to_string())
}

fn read_u32(path: &Path) -> Result<u32> {
    read_trimmed(path)?
        .parse()
        .with_context(|| format!("parsing {}", path.display()))
}

/// Read a temperature file (millidegrees C) as degrees C.
fn read_temp(path: &Path) -> Result<f64> {
    let milli: i64 = read_trimmed(path)?
        .parse()
        .with_context(|| format!("parsing {}", path.display()))?;
    Ok(milli as f64 / 1000.0)
}

/// One point of the chip's built-in Smart Fan IV curve.
struct AutoPointFiles {
    temp: PathBuf,
    pwm: PathBuf,
}

/// Smart Fan IV mode for `pwm{N}_enable`.
const ENABLE_SMART_FAN: u32 = 5;

/// One PWM-controllable fan on the Super I/O chip (nct6798).
pub struct PwmFan {
    /// 1-based index as used in sysfs file names (pwm1, fan1_input, ...).
    pub index: u32,
    pwm: PathBuf,
    enable: PathBuf,
    fan_input: PathBuf,
    /// `pwm{N}_enable` value observed at startup; restored for "auto" mode.
    pub original_enable: u32,
    /// The chip's Smart Fan IV curve registers (`pwm{N}_auto_point*`).
    auto_points: Vec<AutoPointFiles>,
    /// Auto point values observed at startup (millidegrees, raw pwm);
    /// restored together with `original_enable` for "auto" mode.
    original_auto_points: Vec<(i64, u32)>,
}

impl PwmFan {
    pub fn rpm(&self) -> Result<u32> {
        read_u32(&self.fan_input)
    }

    pub fn pwm_raw(&self) -> Result<u32> {
        read_u32(&self.pwm)
    }

    pub fn enable_value(&self) -> Result<u32> {
        read_u32(&self.enable)
    }

    /// Switch the fan to manual PWM control.
    pub fn set_manual(&self) -> Result<()> {
        fs::write(&self.enable, "1").with_context(|| format!("writing {}", self.enable.display()))
    }

    /// Return the fan to the automatic mode that was active at startup,
    /// including the firmware-programmed Smart Fan IV curve points (which
    /// hardware mode may have overwritten).
    pub fn set_auto(&self) -> Result<()> {
        for (files, &(temp, pwm)) in self.auto_points.iter().zip(&self.original_auto_points) {
            fs::write(&files.temp, temp.to_string())
                .with_context(|| format!("writing {}", files.temp.display()))?;
            fs::write(&files.pwm, pwm.to_string())
                .with_context(|| format!("writing {}", files.pwm.display()))?;
        }
        fs::write(&self.enable, self.original_enable.to_string())
            .with_context(|| format!("writing {}", self.enable.display()))
    }

    /// Number of Smart Fan IV curve points the chip supports for this fan.
    pub fn auto_point_count(&self) -> usize {
        self.auto_points.len()
    }

    /// Program the chip's Smart Fan IV curve and let the hardware run the
    /// control loop autonomously. `points` are (millidegrees C, raw pwm),
    /// sorted by temperature, one per chip auto point.
    pub fn set_hardware_curve(&self, points: &[(i64, u8)]) -> Result<()> {
        if points.len() != self.auto_points.len() {
            bail!(
                "fan {} expects {} auto points, got {}",
                self.index,
                self.auto_points.len(),
                points.len()
            );
        }
        for (files, &(temp, pwm)) in self.auto_points.iter().zip(points) {
            fs::write(&files.temp, temp.to_string())
                .with_context(|| format!("writing {}", files.temp.display()))?;
            fs::write(&files.pwm, pwm.to_string())
                .with_context(|| format!("writing {}", files.pwm.display()))?;
        }
        fs::write(&self.enable, ENABLE_SMART_FAN.to_string())
            .with_context(|| format!("writing {}", self.enable.display()))
    }

    /// Write a raw PWM duty value (0-255). Only meaningful in manual mode.
    pub fn set_pwm_raw(&self, value: u8) -> Result<()> {
        fs::write(&self.pwm, value.to_string())
            .with_context(|| format!("writing {}", self.pwm.display()))
    }
}

/// A temperature source, either shown for information or used as input to
/// the fan curves (`control == true`).
pub struct TempSensor {
    /// Human readable label, e.g. "Package id 0", "Core 4" or "nvme0".
    pub label: String,
    /// "cpu" (package), "core" or "nvme" — lets the frontend group sensors.
    pub kind: &'static str,
    /// Drive model for NVMe sensors (from smartctl, fallback sysfs).
    pub model: Option<String>,
    /// Whether this sensor feeds the fan curves. Only the CPU package does.
    pub control: bool,
    input: PathBuf,
}

impl TempSensor {
    pub fn read(&self) -> Result<f64> {
        read_temp(&self.input)
    }
}

/// Reconcile the fans' "original" firmware state with the persisted
/// snapshot: within the same boot the file wins (the chip may already hold
/// our values from a previous daemon run); on a new boot the chip wins and
/// the file is rewritten. Failures only cost restore fidelity, so they warn
/// instead of aborting.
fn apply_snapshot(fans: &mut [PwmFan], path: &Path) {
    let boot_id = current_boot_id();

    let saved: Option<ChipSnapshot> = fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .filter(|s: &ChipSnapshot| !boot_id.is_empty() && s.boot_id == boot_id);

    if let Some(snap) = saved {
        for fan in fans.iter_mut() {
            if let Some(f) = snap.fans.iter().find(|f| f.index == fan.index) {
                if f.auto_points.len() == fan.auto_points.len() {
                    fan.original_enable = f.enable;
                    fan.original_auto_points = f.auto_points.clone();
                }
            }
        }
        tracing::info!(
            "firmware fan state loaded from {} (daemon restarted within this boot)",
            path.display()
        );
        return;
    }

    let snap = ChipSnapshot {
        boot_id,
        fans: fans
            .iter()
            .map(|fan| FanSnapshot {
                index: fan.index,
                enable: fan.original_enable,
                auto_points: fan.original_auto_points.clone(),
            })
            .collect(),
    };
    match serde_json::to_string_pretty(&snap)
        .map_err(anyhow::Error::from)
        .and_then(|json| {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::write(path, json).map_err(Into::into)
        }) {
        Ok(()) => tracing::info!("firmware fan state snapshotted to {}", path.display()),
        Err(e) => tracing::warn!("could not persist firmware snapshot: {e:#}"),
    }
}

/// Look up an NVMe drive's model name via `smartctl -i`, falling back to the
/// sysfs `model` attribute. Called once at startup.
fn nvme_model(chip: &Path, dev: &str) -> Option<String> {
    let smartctl = std::process::Command::new("smartctl")
        .arg("-i")
        .arg(format!("/dev/{dev}"))
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout).lines().find_map(|line| {
                line.strip_prefix("Model Number:")
                    .map(|m| m.trim().to_string())
            })
        });
    smartctl.or_else(|| {
        fs::read_to_string(chip.join("device/model"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    })
}

/// Firmware chip state captured before this service first touched it.
///
/// The values are only trustworthy when read from a chip the daemon has not
/// yet written to. If the daemon crashes in hardware mode and restarts, the
/// chip still holds *our* curve — so the snapshot taken at the first start of
/// each boot is persisted and reused for the rest of that boot. After a
/// reboot the BIOS reprograms the chip and a fresh snapshot is taken.
#[derive(Serialize, Deserialize)]
struct ChipSnapshot {
    boot_id: String,
    fans: Vec<FanSnapshot>,
}

#[derive(Serialize, Deserialize)]
struct FanSnapshot {
    index: u32,
    enable: u32,
    auto_points: Vec<(i64, u32)>,
}

fn current_boot_id() -> String {
    fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// All hardware handles the controller needs.
pub struct Hardware {
    pub fans: Vec<PwmFan>,
    pub sensors: Vec<TempSensor>,
}

impl Hardware {
    /// Discover the nct6798 fan controller, the CPU package sensor and all
    /// NVMe composite sensors. `snapshot_path` persists the firmware fan
    /// state across daemon restarts within one boot.
    pub fn discover(snapshot_path: &Path) -> Result<Hardware> {
        let nct = find_chips(|n| n.starts_with("nct67"))
            .into_iter()
            .next()
            .context("no nct67xx hwmon chip found (is the nct6775 driver loaded?)")?;
        tracing::info!("fan controller chip: {}", nct.display());

        // The MS-01 has two chassis fans wired to pwm1/fan1 and pwm2/fan2.
        let mut fans = Vec::new();
        for index in [1u32, 2] {
            let pwm = nct.join(format!("pwm{index}"));
            let enable = nct.join(format!("pwm{index}_enable"));
            let fan_input = nct.join(format!("fan{index}_input"));
            if !pwm.exists() || !enable.exists() {
                bail!("missing {} or {}", pwm.display(), enable.display());
            }
            let original_enable = read_u32(&enable)?;

            // Enumerate the chip's Smart Fan IV curve registers and snapshot
            // the firmware-programmed values so "auto" can restore them.
            let mut auto_points = Vec::new();
            let mut original_auto_points = Vec::new();
            for point in 1.. {
                let temp = nct.join(format!("pwm{index}_auto_point{point}_temp"));
                let pwm = nct.join(format!("pwm{index}_auto_point{point}_pwm"));
                if !temp.exists() || !pwm.exists() {
                    break;
                }
                let temp_val: i64 = read_trimmed(&temp)?
                    .parse()
                    .with_context(|| format!("parsing {}", temp.display()))?;
                original_auto_points.push((temp_val, read_u32(&pwm)?));
                auto_points.push(AutoPointFiles { temp, pwm });
            }
            tracing::info!(
                "fan {index}: original pwm{index}_enable = {original_enable}, \
                 {} hardware curve points",
                auto_points.len()
            );
            fans.push(PwmFan {
                index,
                pwm,
                enable,
                fan_input,
                original_enable,
                auto_points,
                original_auto_points,
            });
        }

        let mut sensors = Vec::new();

        // CPU: every coretemp channel. temp1 is "Package id 0" (the overall
        // package temperature) and is the only fan-curve control input; the
        // per-core channels are informational.
        for chip in find_chips(|n| n == "coretemp") {
            let mut channels: Vec<u32> = fs::read_dir(&chip)
                .map(|entries| {
                    entries
                        .flatten()
                        .filter_map(|e| {
                            let name = e.file_name().to_string_lossy().into_owned();
                            name.strip_prefix("temp")?
                                .strip_suffix("_input")?
                                .parse()
                                .ok()
                        })
                        .collect()
                })
                .unwrap_or_default();
            channels.sort_unstable();
            for ch in channels {
                let label = fs::read_to_string(chip.join(format!("temp{ch}_label")))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|_| format!("temp{ch}"));
                let is_package = label.starts_with("Package");
                sensors.push(TempSensor {
                    label,
                    kind: if is_package { "cpu" } else { "core" },
                    model: None,
                    control: is_package,
                    input: chip.join(format!("temp{ch}_input")),
                });
            }
        }

        // NVMe drives: temp1 is the "Composite" sensor. Display only — SSD
        // composite temps are coarse (integer Kelvin) and react slowly, so
        // they are not used for fan control.
        for chip in find_chips(|n| n == "nvme") {
            let input = chip.join("temp1_input");
            if !input.exists() {
                continue;
            }
            // device symlink points at the nvme device, use its name (nvme0..)
            let dev = chip
                .join("device")
                .canonicalize()
                .ok()
                .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "nvme?".into());
            sensors.push(TempSensor {
                model: nvme_model(&chip, &dev),
                label: dev,
                kind: "nvme",
                control: false,
                input,
            });
        }

        apply_snapshot(&mut fans, snapshot_path);

        if !sensors.iter().any(|s| s.control) {
            bail!("no CPU package temperature sensor found (needed for fan control)");
        }
        for s in &sensors {
            tracing::info!(
                "sensor: {} ({}{}{})",
                s.label,
                s.kind,
                if s.control { ", control" } else { "" },
                s.model.as_deref().map(|m| format!(", {m}")).unwrap_or_default(),
            );
        }

        Ok(Hardware { fans, sensors })
    }

    /// Restore every fan to its original automatic mode. Used on shutdown
    /// and as a failsafe.
    pub fn restore_auto(&self) {
        for fan in &self.fans {
            if let Err(e) = fan.set_auto() {
                tracing::error!("failed to restore fan {} to auto: {e:#}", fan.index);
            } else {
                tracing::info!(
                    "fan {} restored to automatic mode (enable={})",
                    fan.index,
                    fan.original_enable
                );
            }
        }
    }
}
