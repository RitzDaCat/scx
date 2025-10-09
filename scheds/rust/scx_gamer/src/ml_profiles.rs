// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer: Per-Game Configuration Profiles
// Copyright (c) 2025 RitzDaCat
//
// Automatically saves and loads optimal scheduler configurations per game.
// When a game is detected, its best known config is applied automatically.

use anyhow::Result;
use log::info;
use serde::{Deserialize, Serialize};
use ahash::AHashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::ml_collect::SchedulerConfig;

/// Per-game profile with performance data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameProfile {
    pub game_name: String,
    pub best_config: SchedulerConfig,
    pub best_score: f64,
    pub sample_count: usize,
    pub last_updated: u64,  // Unix timestamp
    pub avg_fps: f64,
    pub avg_jitter_ms: f64,
    pub avg_latency_ns: u64,
}

/// Profile manager for per-game configs
pub struct ProfileManager {
    profiles_dir: PathBuf,
    profiles: AHashMap<String, GameProfile>,
}

impl ProfileManager {
    /// Create new profile manager
    pub fn new(profiles_dir: impl AsRef<Path>) -> Result<Self> {
        let profiles_dir = profiles_dir.as_ref().to_path_buf();
        fs::create_dir_all(&profiles_dir)?;

        let mut manager = Self {
            profiles_dir,
            profiles: AHashMap::new(),
        };

        // Load existing profiles
        manager.load_all()?;

        Ok(manager)
    }

    /// Load all profiles from disk
    fn load_all(&mut self) -> Result<()> {
        for entry in fs::read_dir(&self.profiles_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(profile) = self.load_profile_file(&path) {
                    self.profiles.insert(profile.game_name.clone(), profile);
                }
            }
        }

        info!("Profile: Loaded {} game profiles", self.profiles.len());
        Ok(())
    }

    /// Load a single profile file
    fn load_profile_file(&self, path: &Path) -> Result<GameProfile> {
        let content = fs::read_to_string(path)?;
        let profile: GameProfile = serde_json::from_str(&content)?;
        Ok(profile)
    }

    /// Get profile for a game
    pub fn get_profile(&self, game_name: &str) -> Option<&GameProfile> {
        self.profiles.get(game_name)
    }

    // update_if_better() removed - autotuner handles profile updates directly

    /// Save or update profile for a game (test-only helper)
    #[cfg(test)]
    fn save_profile(&mut self, profile: GameProfile) -> Result<()> {
        let profile_path = self.get_profile_path(&profile.game_name);

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&profile)?;
        fs::write(&profile_path, json)?;

        info!(
            "Profile: Saved '{}' - Score: {:.2}, FPS: {:.1}, Jitter: {:.2}ms",
            profile.game_name,
            profile.best_score,
            profile.avg_fps,
            profile.avg_jitter_ms
        );

        // Update in-memory cache
        self.profiles.insert(profile.game_name.clone(), profile);

        Ok(())
    }

    /// Get profile file path for a game (test-only helper)
    #[cfg(test)]
    fn get_profile_path(&self, game_name: &str) -> PathBuf {
        // Sanitize game name for filesystem
        let safe_name = game_name.replace(['/', '\\', ' ', ':', '.'], "_");
        self.profiles_dir.join(format!("{}.json", safe_name))
    }

    /// List all known games
    pub fn list_games(&self) -> Vec<String> {
        self.profiles.keys().cloned().collect()
    }

    // delete_profile() removed - use filesystem commands (rm) to delete profiles if needed

    /// Get summary statistics across all profiles
    pub fn get_summary(&self) -> ProfileSummary {
        if self.profiles.is_empty() {
            return ProfileSummary::default();
        }

        let total_games = self.profiles.len();
        let avg_score = self.profiles.values()
            .map(|p| p.best_score)
            .sum::<f64>() / total_games as f64;

        let avg_fps = self.profiles.values()
            .map(|p| p.avg_fps)
            .sum::<f64>() / total_games as f64;

        ProfileSummary {
            total_games,
            avg_score,
            avg_fps,
        }
    }
}

/// Summary statistics across all profiles
#[derive(Debug, Default)]
pub struct ProfileSummary {
    pub total_games: usize,
    pub avg_score: f64,
    pub avg_fps: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_profile_save_load() {
        let temp_dir = tempdir().unwrap();
        let mut manager = ProfileManager::new(temp_dir.path()).unwrap();

        let config = SchedulerConfig {
            slice_us: 10,
            slice_lag_us: 20000,
            input_window_us: 2000,
            mig_window_ms: 50,
            mig_max: 3,
            mm_affinity: true,
            avoid_smt: false,
            preferred_idle_scan: true,
            enable_numa: false,
            wakeup_timer_us: 500,
        };

        let profile = GameProfile {
            game_name: "TestGame.exe".to_string(),
            best_config: config,
            best_score: 85.5,
            sample_count: 100,
            last_updated: 1234567890,
            avg_fps: 62.3,
            avg_jitter_ms: 0.9,
            avg_latency_ns: 850,
        };

        manager.save_profile(profile).unwrap();

        // Reload from disk
        let mut manager2 = ProfileManager::new(temp_dir.path()).unwrap();
        let loaded = manager2.get_profile("TestGame.exe").unwrap();

        assert_eq!(loaded.game_name, "TestGame.exe");
        assert_eq!(loaded.best_score, 85.5);
        assert_eq!(loaded.best_config.slice_us, 10);
    }
}
