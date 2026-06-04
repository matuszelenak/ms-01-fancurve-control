//! Discovery and access of hwmon sysfs devices on the MS-01.
//!
//! Devices are located by chip name (`/sys/class/hwmon/hwmon*/name`) rather
//! than by index, since hwmon numbering can change between boots.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

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

/// One PWM-controllable fan on the Super I/O chip (nct6798).
pub struct PwmFan {
    /// 1-based index as used in sysfs file names (pwm1, fan1_input, ...).
    pub index: u32,
    pwm: PathBuf,
    enable: PathBuf,
    fan_input: PathBuf,
    /// `pwm{N}_enable` value observed at startup; restored for "auto" mode.
    pub original_enable: u32,
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

    /// Return the fan to the automatic mode that was active at startup.
    pub fn set_auto(&self) -> Result<()> {
        fs::write(&self.enable, self.original_enable.to_string())
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

/// All hardware handles the controller needs.
pub struct Hardware {
    pub fans: Vec<PwmFan>,
    pub sensors: Vec<TempSensor>,
}

impl Hardware {
    /// Discover the nct6798 fan controller, the CPU package sensor and all
    /// NVMe composite sensors.
    pub fn discover() -> Result<Hardware> {
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
            tracing::info!("fan {index}: original pwm{index}_enable = {original_enable}");
            fans.push(PwmFan {
                index,
                pwm,
                enable,
                fan_input,
                original_enable,
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
