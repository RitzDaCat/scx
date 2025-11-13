// SPDX-License-Identifier: GPL-2.0
//
// Power and thermal monitoring for automatic scheduler hints.
// Detects CPU package temperature and socket power via hwmon sensors (zenpower/k10temp).
// Applies hysteresis to avoid oscillation and emits coarse power-hint levels for BPF.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use log::{info, warn};

const TEMP_LEVEL1: f64 = 78.0; // °C
const TEMP_LEVEL2: f64 = 83.0; // °C
const TEMP_RELEASE1: f64 = 74.0;
const TEMP_RELEASE2: f64 = 80.0;

const POWER_LEVEL1: f64 = 120.0; // Watts
const POWER_LEVEL2: f64 = 135.0; // Watts
const POWER_RELEASE1: f64 = 110.0;
const POWER_RELEASE2: f64 = 125.0;

const SAMPLE_INTERVAL: Duration = Duration::from_millis(500);

pub struct PowerHint {
    pub level: u32,
    pub duration_ms: u32,
}

pub struct PowerMonitor {
    temp_sensor: Option<Sensor>,
    power_sensor: Option<Sensor>,
    last_level: u32,
    last_sample: Instant,
    last_change: Instant,
}

#[derive(Clone)]
struct Sensor {
    path: PathBuf,
    label: Option<String>,
    scale: f64,
    kind: SensorKind,
}

#[derive(Clone, Copy)]
enum SensorKind {
    Temperature,
    Power,
}

impl SensorKind {
    #[inline(always)]
    fn name(self) -> &'static str {
        match self {
            SensorKind::Temperature => "temperature",
            SensorKind::Power => "power",
        }
    }

    #[inline(always)]
    fn unit(self) -> &'static str {
        match self {
            SensorKind::Temperature => "°C",
            SensorKind::Power => "W",
        }
    }
}

impl PowerMonitor {
    pub fn new() -> Option<Self> {
        let temp_sensor = discover_temperature_sensor();
        let power_sensor = discover_power_sensor();

        if temp_sensor.is_none() && power_sensor.is_none() {
            warn!("Power monitor: no temperature/power sensors detected (zenpower/k10temp)");
            return None;
        }

        if let Some(ref sensor) = temp_sensor {
            info!(
                "Power monitor: {} sensor -> {} ({})",
                sensor.kind.name(),
                sensor.path.display(),
                sensor
                    .label
                    .as_deref()
                    .unwrap_or("unlabeled")
            );
        } else {
            warn!("Power monitor: temperature sensor not found; thermal hints disabled");
        }

        if let Some(ref sensor) = power_sensor {
            info!(
                "Power monitor: {} sensor -> {} ({})",
                sensor.kind.name(),
                sensor.path.display(),
                sensor
                    .label
                    .as_deref()
                    .unwrap_or("unlabeled")
            );
        } else {
            warn!("Power monitor: power sensor not found; power-limit hints disabled");
        }

        Some(Self {
            temp_sensor,
            power_sensor,
            last_level: 0,
            last_sample: Instant::now() - SAMPLE_INTERVAL,
            last_change: Instant::now(),
        })
    }

    pub fn poll(&mut self) -> Option<PowerHint> {
        let now = Instant::now();
        if now.duration_since(self.last_sample) < SAMPLE_INTERVAL {
            return None;
        }
        self.last_sample = now;

        let temp = self
            .temp_sensor
            .as_ref()
            .and_then(|sensor| sensor.read_value().ok());
        let power = self
            .power_sensor
            .as_ref()
            .and_then(|sensor| sensor.read_value().ok());

        let mut desired = self.determine_level(temp, power);

        if desired < self.last_level {
            let should_hold = match self.last_level {
                2 => {
                    let temp_hold = temp.map_or(false, |t| t >= TEMP_RELEASE2);
                    let power_hold = power.map_or(false, |p| p >= POWER_RELEASE2);
                    temp_hold || power_hold
                }
                1 => {
                    let temp_hold = temp.map_or(false, |t| t >= TEMP_RELEASE1);
                    let power_hold = power.map_or(false, |p| p >= POWER_RELEASE1);
                    temp_hold || power_hold
                }
                _ => false,
            };
            if should_hold {
                desired = self.last_level;
            }
        }

        if desired != self.last_level {
            self.last_level = desired;
            self.last_change = now;

            let temp_unit = self
                .temp_sensor
                .as_ref()
                .map(|s| s.kind.unit())
                .unwrap_or("°C");
            let power_unit = self
                .power_sensor
                .as_ref()
                .map(|s| s.kind.unit())
                .unwrap_or("W");

            let duration_ms = match desired {
                0 => 0,
                1 => 3_000,
                2 => 6_000,
                _ => 0,
            };

            info!(
                "Power monitor: hint level {} (temp: {}, power: {})",
                desired,
                format_sensor(temp, temp_unit),
                format_sensor(power, power_unit)
            );

            return Some(PowerHint {
                level: desired,
                duration_ms,
            });
        }

        None
    }

    fn determine_level(&self, temp: Option<f64>, power: Option<f64>) -> u32 {
        let mut level = 0;
        if let Some(temp) = temp {
            if temp >= TEMP_LEVEL2 {
                level = level.max(2);
            } else if temp >= TEMP_LEVEL1 {
                level = level.max(1);
            }
        }

        if let Some(power) = power {
            if power >= POWER_LEVEL2 {
                level = level.max(2);
            } else if power >= POWER_LEVEL1 {
                level = level.max(1);
            }
        }

        level
    }
}

impl Sensor {
    fn read_value(&self) -> io::Result<f64> {
        let raw = fs::read_to_string(&self.path)?;
        let value: f64 = raw.trim().parse::<f64>().map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to parse {}: {}", self.path.display(), e),
            )
        })?;
        Ok(value * self.scale)
    }
}

fn format_sensor(value: Option<f64>, suffix: &str) -> String {
    match value {
        Some(v) => format!("{:.1}{}", v, suffix),
        None => "n/a".to_string(),
    }
}

fn discover_temperature_sensor() -> Option<Sensor> {
    let preferred_names = ["zenpower", "k10temp", "coretemp"];
    discover_sensor("temp", 0.001, SensorKind::Temperature, &preferred_names)
}

fn discover_power_sensor() -> Option<Sensor> {
    let preferred_names = ["zenpower"];
    discover_sensor("power", 1f64 / 1_000_000f64, SensorKind::Power, &preferred_names)
}

fn discover_sensor(
    prefix: &str,
    scale: f64,
    kind: SensorKind,
    preferred_hwmons: &[&str],
) -> Option<Sensor> {
    let hwmon_root = Path::new("/sys/class/hwmon");
    let mut preferred = Vec::new();
    let mut fallback = Vec::new();

    let entries = match fs::read_dir(hwmon_root) {
        Ok(entries) => entries,
        Err(e) => {
            warn!("Power monitor: failed to read hwmon directory: {}", e);
            return None;
        }
    };

    for entry in entries.flatten() {
        let base = entry.path();
        if !base.is_dir() {
            continue;
        }
        let name_path = base.join("name");
        let hwmon_name = fs::read_to_string(&name_path)
            .unwrap_or_default()
            .trim()
            .to_lowercase();

        let mut candidates = gather_hwmon_sensors(&base, prefix, scale, kind);
        if candidates.is_empty() {
            continue;
        }

        if preferred_hwmons.iter().any(|p| hwmon_name.contains(p)) {
            preferred.append(&mut candidates);
        } else {
            fallback.append(&mut candidates);
        }
    }

    if let Some(sensor) = choose_best_sensor(preferred, prefix) {
        return Some(sensor);
    }
    choose_best_sensor(fallback, prefix)
}

fn gather_hwmon_sensors(
    base: &Path,
    prefix: &str,
    scale: f64,
    kind: SensorKind,
) -> Vec<Sensor> {
    let mut sensors = Vec::new();
    let entries = match fs::read_dir(base) {
        Ok(entries) => entries,
        Err(_) => return sensors,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(prefix) || !name.ends_with("_input") {
            continue;
        }
        let suffix = &name[prefix.len()..name.len() - "_input".len()];
        let label_path = base.join(format!("{prefix}{suffix}_label"));
        let label = fs::read_to_string(&label_path).ok().map(|s| s.trim().to_string());
        sensors.push(Sensor {
            path,
            label,
            scale,
            kind,
        });
    }

    sensors
}

fn choose_best_sensor(mut sensors: Vec<Sensor>, prefix: &str) -> Option<Sensor> {
    if sensors.is_empty() {
        return None;
    }

    sensors.sort_by_key(|sensor| {
        let label = sensor.label.as_deref().unwrap_or_default().to_lowercase();
        match prefix {
            "temp" => {
                if label.contains("tdie") || label.contains("cpu") || label.contains("socket") {
                    0
                } else if label.contains("tctl") {
                    1
                } else {
                    2
                }
            }
            "power" => {
                if label.contains("socket") || label.contains("cpu") {
                    0
                } else {
                    1
                }
            }
            _ => 2,
        }
    });

    sensors.into_iter().next()
}

