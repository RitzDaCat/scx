// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer: CPU Detection for Hardware-Specific Training Data
// Copyright (c) 2025 RitzDaCat
//
// Detects CPU model to organize training data by hardware characteristics.
// Allows sharing training data while keeping it relevant to specific CPU architectures.

use anyhow::Result;
use std::fs;

/// Detected CPU information
#[derive(Debug, Clone)]
pub struct CpuInfo {
    pub model_name: String,
    pub safe_name: String,  // Filesystem-safe version
}

impl CpuInfo {
    /// Detect CPU from /proc/cpuinfo
    pub fn detect() -> Result<Self> {
        let cpuinfo = fs::read_to_string("/proc/cpuinfo")?;

        // Find model name line
        let model_name = cpuinfo
            .lines()
            .find(|line| line.starts_with("model name"))
            .and_then(|line| line.split(':').nth(1))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "Unknown CPU".to_string());

        // Create filesystem-safe version
        let safe_name = Self::sanitize_cpu_name(&model_name);

        Ok(Self {
            model_name,
            safe_name,
        })
    }

    /// Convert CPU name to filesystem-safe string
    fn sanitize_cpu_name(name: &str) -> String {
        // Remove common prefixes/suffixes
        let cleaned = name
            .replace("AMD ", "")
            .replace("Intel(R) ", "")
            .replace("Intel ", "")
            .replace("Core(TM) ", "")
            .replace("(R)", "")
            .replace("(TM)", "")
            .replace("Processor", "")
            .trim()
            .to_string();

        // Replace filesystem-unsafe characters
        cleaned
            .chars()
            .map(|c| match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' => c,
                ' ' | '-' => '_',
                _ => '_',
            })
            .collect::<String>()
            .split('_')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("_")
    }

    /// Get short identifier (e.g., "9800X3D", "14900K")
    pub fn short_id(&self) -> String {
        // Try to extract model number
        let parts: Vec<&str> = self.safe_name.split('_').collect();

        // Look for patterns like "9800X3D", "14900K", "5950X"
        for part in &parts {
            // If part contains numbers and letters, likely the model
            if part.chars().any(|c| c.is_numeric()) &&
               part.chars().any(|c| c.is_alphabetic()) {
                return part.to_string();
            }
        }

        // Fallback: use first 2-3 meaningful parts
        parts.iter()
            .take(3)
            .map(|s| *s)
            .collect::<Vec<_>>()
            .join("_")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize() {
        let amd = "AMD Ryzen 9 9800X3D 16-Core Processor";
        let intel = "Intel(R) Core(TM) i9-14900K Processor";

        let amd_safe = CpuInfo::sanitize_cpu_name(amd);
        let intel_safe = CpuInfo::sanitize_cpu_name(intel);

        assert_eq!(amd_safe, "Ryzen_9_9800X3D_16_Core");
        assert_eq!(intel_safe, "i9_14900K");
    }

    #[test]
    fn test_short_id() {
        let cpu = CpuInfo {
            model_name: "AMD Ryzen 9 9800X3D".to_string(),
            safe_name: "Ryzen_9_9800X3D".to_string(),
        };

        assert_eq!(cpu.short_id(), "9800X3D");
    }
}
