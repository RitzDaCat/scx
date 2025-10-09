// SPDX-License-Identifier: GPL-2.0
//
// Thread Pattern Recognition and Storage
// Copyright (c) 2025 RitzDaCat
//
// Learns and persists per-game thread naming patterns for accurate classification.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Thread classification roles (matches BPF classifications)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ThreadRole {
    Main,
    InputHandler,
    GpuSubmit,
    Render,
    Network,
    SystemAudio,
    GameAudio,
    Compositor,
    Background,
    Unknown,
}

/// Runtime statistics for a single thread
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadInfo {
    pub tid: u32,
    pub comm: String,
    pub role: ThreadRole,
    pub classification: String,  // BPF classification name
    pub avg_wakeup_freq: u64,
    pub avg_exec_ns: u64,
    pub cpu_time_pct: f64,
    pub samples: u32,
}

/// Pattern matching rule learned from thread behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternRule {
    pub rule: String,
    pub threshold: Option<u64>,
    pub confidence: f64,
}

/// Complete thread pattern for a game
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadPattern {
    pub game_name: String,
    pub engine: Option<String>,
    pub detected_at: String,
    pub pid: u32,
    pub threads: Vec<ThreadInfo>,
    pub pattern_rules: Vec<PatternRule>,
}

impl ThreadPattern {
    pub fn new(game_name: String, pid: u32) -> Self {
        Self {
            game_name,
            engine: None,
            detected_at: chrono::Utc::now().to_rfc3339(),
            pid,
            threads: Vec::new(),
            pattern_rules: Vec::new(),
        }
    }

    /// Add or update thread information
    pub fn update_thread(&mut self, thread: ThreadInfo) {
        if let Some(existing) = self.threads.iter_mut().find(|t| t.tid == thread.tid) {
            *existing = thread;
        } else {
            self.threads.push(thread);
        }
    }

    /// Generate pattern rules from collected thread data
    pub fn generate_rules(&mut self) {
        self.pattern_rules.clear();

        // Rule 1: Main thread (PID == TID) is usually input handler
        if let Some(main) = self.threads.iter().find(|t| t.tid == self.pid) {
            if main.role == ThreadRole::InputHandler || main.role == ThreadRole::Main {
                self.pattern_rules.push(PatternRule {
                    rule: "main_thread_is_input".to_string(),
                    threshold: None,
                    confidence: 0.95,
                });
            }
        }

        // Rule 2: High wakeup frequency suggests render/GPU thread
        let high_wakeup_threads: Vec<_> = self.threads.iter()
            .filter(|t| t.avg_wakeup_freq > 400 && matches!(t.role, ThreadRole::GpuSubmit | ThreadRole::Render))
            .collect();

        if !high_wakeup_threads.is_empty() {
            let avg_freq = high_wakeup_threads.iter().map(|t| t.avg_wakeup_freq).sum::<u64>()
                / high_wakeup_threads.len() as u64;
            self.pattern_rules.push(PatternRule {
                rule: "high_wakeup_freq_is_render".to_string(),
                threshold: Some(avg_freq),
                confidence: 0.85,
            });
        }

        // Rule 3: If all threads have same name, use behavior-based classification
        let unique_names: std::collections::HashSet<_> =
            self.threads.iter().map(|t| &t.comm).collect();
        if unique_names.len() == 1 {
            self.pattern_rules.push(PatternRule {
                rule: "identical_names_use_behavior".to_string(),
                threshold: None,
                confidence: 1.0,
            });
        }
    }

    /// Check if this pattern applies to a given thread
    pub fn match_thread(&self, tid: u32, comm: &str) -> Option<ThreadRole> {
        // Exact TID match (if game was restarted with same PIDs - unlikely but possible)
        if let Some(thread) = self.threads.iter().find(|t| t.tid == tid && t.comm == comm) {
            return Some(thread.role.clone());
        }

        // Name-based match
        if let Some(thread) = self.threads.iter().find(|t| t.comm == comm) {
            return Some(thread.role.clone());
        }

        None
    }
}

/// Manages thread patterns for all games
pub struct ThreadPatternManager {
    patterns_dir: PathBuf,
    patterns: HashMap<String, ThreadPattern>,
}

impl ThreadPatternManager {
    pub fn new(patterns_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&patterns_dir)
            .with_context(|| format!("Failed to create thread patterns directory: {}", patterns_dir.display()))?;

        let mut manager = Self {
            patterns_dir,
            patterns: HashMap::new(),
        };

        manager.load_all_patterns()?;
        Ok(manager)
    }

    /// Load all existing patterns from disk
    fn load_all_patterns(&mut self) -> Result<()> {
        if !self.patterns_dir.exists() {
            return Ok(());
        }

        for entry in std::fs::read_dir(&self.patterns_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(pattern) = self.load_pattern(&path) {
                    self.patterns.insert(pattern.game_name.clone(), pattern);
                }
            }
        }

        Ok(())
    }

    /// Load a single pattern from file
    fn load_pattern(&self, path: &PathBuf) -> Result<ThreadPattern> {
        let content = std::fs::read_to_string(path)?;
        let pattern: ThreadPattern = serde_json::from_str(&content)?;
        Ok(pattern)
    }

    /// Save pattern to disk
    pub fn save_pattern(&self, pattern: &ThreadPattern) -> Result<()> {
        let filename = format!("{}.json", pattern.game_name);
        let path = self.patterns_dir.join(filename);

        let json = serde_json::to_string_pretty(pattern)?;
        std::fs::write(&path, json)
            .with_context(|| format!("Failed to write thread pattern: {}", path.display()))?;

        Ok(())
    }

    /// Get pattern for a game (if exists)
    pub fn get_pattern(&self, game_name: &str) -> Option<&ThreadPattern> {
        self.patterns.get(game_name)
    }

    /// Create or update pattern for a game
    pub fn update_pattern(&mut self, game_name: String, pid: u32) -> &mut ThreadPattern {
        self.patterns.entry(game_name.clone())
            .or_insert_with(|| ThreadPattern::new(game_name, pid))
    }

    /// List all known games
    pub fn list_games(&self) -> Vec<String> {
        self.patterns.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thread_pattern_creation() {
        let pattern = ThreadPattern::new("test_game.exe".to_string(), 1234);
        assert_eq!(pattern.game_name, "test_game.exe");
        assert_eq!(pattern.pid, 1234);
        assert_eq!(pattern.threads.len(), 0);
    }

    #[test]
    fn test_update_thread() {
        let mut pattern = ThreadPattern::new("test_game.exe".to_string(), 1234);

        let thread = ThreadInfo {
            tid: 1234,
            comm: "test_game.exe".to_string(),
            role: ThreadRole::Main,
            classification: "main".to_string(),
            avg_wakeup_freq: 100,
            avg_exec_ns: 50000,
            cpu_time_pct: 10.0,
            samples: 10,
        };

        pattern.update_thread(thread.clone());
        assert_eq!(pattern.threads.len(), 1);

        // Update same thread
        pattern.update_thread(thread);
        assert_eq!(pattern.threads.len(), 1);
    }

    #[test]
    fn test_generate_rules() {
        let mut pattern = ThreadPattern::new("test_game.exe".to_string(), 1234);

        pattern.update_thread(ThreadInfo {
            tid: 1234,
            comm: "test_game.exe".to_string(),
            role: ThreadRole::Main,
            classification: "input_handler".to_string(),
            avg_wakeup_freq: 100,
            avg_exec_ns: 50000,
            cpu_time_pct: 10.0,
            samples: 10,
        });

        pattern.generate_rules();
        assert!(!pattern.pattern_rules.is_empty());
        assert!(pattern.pattern_rules.iter().any(|r| r.rule == "main_thread_is_input"));
    }
}
