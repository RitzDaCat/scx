// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer: Gaming-optimized scheduler for low-latency input and frame delivery
// Copyright (c) 2025 RitzDaCat
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2.

use std::fs;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use arc_swap::ArcSwap;
use inotify::{Inotify, WatchMask, EventMask};
use log::{info, warn};

#[derive(Debug, Clone)]
pub struct GameInfo {
	pub tgid: u32,
	pub name: String,
	pub is_wine: bool,
	pub is_steam: bool,
}

/* OPTIMIZATION: Fixed-size ring buffer with bitmask for better memory usage
 * Eliminates HashSet growth/shrinking overhead and memory fragmentation
 * Uses bitmask for O(1) PID lookup instead of O(log n) HashSet operations */
const CACHE_SIZE: usize = 4096;
const BITMASK_SIZE: usize = CACHE_SIZE / 64; // 64 bits per u64

struct ProcessCache {
	last_game: Option<GameInfo>,
	pid_ring: [u32; CACHE_SIZE],  // Ring buffer of PIDs
	bitmask: [u64; BITMASK_SIZE], // Bitmask for fast PID lookup
	head: usize,                   // Ring buffer head pointer
	count: usize,                  // Number of valid entries
}

impl ProcessCache {
	fn new() -> Self {
		Self {
			last_game: None,
			pid_ring: [0; CACHE_SIZE],
			bitmask: [0; BITMASK_SIZE],
			head: 0,
			count: 0,
		}
	}
	
	/* OPTIMIZATION: O(1) PID lookup using bitmask instead of O(log n) HashSet */
	fn contains(&self, pid: u32) -> bool {
		let idx = (pid % CACHE_SIZE as u32) as usize;
		let bit_idx = idx / 64;
		let bit_pos = idx % 64;
		if bit_idx < BITMASK_SIZE {
			self.bitmask[bit_idx] & (1u64 << bit_pos) != 0
		} else {
			false
		}
	}
	
	/* OPTIMIZATION: O(1) PID insertion with ring buffer eviction */
	fn insert(&mut self, pid: u32) {
		let idx = (pid % CACHE_SIZE as u32) as usize;
		let bit_idx = idx / 64;
		let bit_pos = idx % 64;
		
		if bit_idx < BITMASK_SIZE {
			// Remove old PID if ring buffer is full
			if self.count >= CACHE_SIZE {
				let old_pid = self.pid_ring[self.head];
				let old_idx = (old_pid % CACHE_SIZE as u32) as usize;
				let old_bit_idx = old_idx / 64;
				let old_bit_pos = old_idx % 64;
				if old_bit_idx < BITMASK_SIZE {
					self.bitmask[old_bit_idx] &= !(1u64 << old_bit_pos);
				}
			} else {
				self.count += 1;
			}
			
			// Insert new PID
			self.pid_ring[self.head] = pid;
			self.bitmask[bit_idx] |= 1u64 << bit_pos;
			self.head = (self.head + 1) % CACHE_SIZE;
		}
	}
	
	/* OPTIMIZATION: O(1) cleanup - just reset counters */
	fn clear(&mut self) {
		self.bitmask = [0; BITMASK_SIZE];
		self.head = 0;
		self.count = 0;
	}
}

pub struct GameDetector {
	current_game: Arc<AtomicU32>,
	current_game_info: Arc<ArcSwap<Option<GameInfo>>>,  // PERF: Lock-free reads via ArcSwap
	shutdown: Arc<AtomicBool>,
	_thread: Option<JoinHandle<()>>,
}

impl GameDetector {
	pub fn new() -> Self {
		let current_game = Arc::new(AtomicU32::new(0));
		let current_game_info = Arc::new(ArcSwap::from_pointee(None));
		let shutdown = Arc::new(AtomicBool::new(false));
		let thread_game = Arc::clone(&current_game);
		let thread_game_info = Arc::clone(&current_game_info);
		let thread_shutdown = Arc::clone(&shutdown);

		let handle = thread::Builder::new()
			.name("game-detect".to_string())
			.spawn(move || detection_loop(thread_game, thread_game_info, thread_shutdown))
			.expect("failed to spawn game detector thread");

		Self {
			current_game,
			current_game_info,
			shutdown,
			_thread: Some(handle),
		}
	}

	#[inline]
	pub fn get_game_tgid(&self) -> u32 {
		self.current_game.load(Ordering::Relaxed)
	}

	/// Get full game info including wine/steam detection (lock-free read)
	#[inline]
	pub fn get_game_info(&self) -> Option<GameInfo> {
		(**self.current_game_info.load()).clone()
	}
}

impl Drop for GameDetector {
	fn drop(&mut self) {
		// Signal thread to shutdown
		self.shutdown.store(true, Ordering::Relaxed);

		if let Some(handle) = self._thread.take() {
			// Wait up to 2 seconds for graceful shutdown
			// With non-blocking inotify, thread should exit within 100ms
			for _ in 0..20 {
				if handle.is_finished() {
					let _ = handle.join();
					info!("game detector: clean shutdown");
					return;
				}
				thread::sleep(Duration::from_millis(100));
			}

			// Thread didn't exit gracefully - this shouldn't happen with non-blocking inotify
			warn!("game detector: thread didn't exit within 2s, forcing detach (potential resource leak)");
			// Note: thread will be forcefully detached, may cause resource leak
			// but prevents hanging the entire process on Ctrl+C
		}
	}
}

fn detection_loop(current_game: Arc<AtomicU32>, current_game_info: Arc<ArcSwap<Option<GameInfo>>>, shutdown: Arc<AtomicBool>) {
	info!("game detector: starting");
	let mut cache = ProcessCache::new();

	// Clone shutdown for passing to detect_game_cached (allows early exit)
	let shutdown_check = Arc::clone(&shutdown);

	// Set up inotify watch on /proc for instant detection of new processes
	// CRITICAL: Must use non-blocking mode to allow clean shutdown on Ctrl+C
	let mut inotify = match Inotify::init() {
		Ok(inotify) => {
			// Set non-blocking mode to prevent shutdown hangs
			let fd = inotify.as_raw_fd();
			unsafe {
				// Get current flags
				let flags = libc::fcntl(fd, libc::F_GETFL);
				if flags == -1 {
					warn!("game detector: failed to get fd flags, falling back to polling");
					None
				} else {
					// Set O_NONBLOCK flag
					if libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) == -1 {
						warn!("game detector: failed to set non-blocking: {}, falling back to polling",
							std::io::Error::last_os_error());
						None
					} else {
						match inotify.watches().add("/proc", WatchMask::CREATE | WatchMask::ONLYDIR) {
							Ok(_) => {
								info!("game detector: using inotify for instant process detection (non-blocking)");
								Some(inotify)
							},
							Err(e) => {
								warn!("game detector: failed to watch /proc: {}, falling back to polling", e);
								None
							}
						}
					}
				}
			}
		},
		Err(e) => {
			warn!("game detector: failed to init inotify: {}, falling back to polling", e);
			None
		}
	};

	// OPTIMIZATION: Do initial scan ONCE at startup to detect already-running games
	// After that, rely on inotify for new processes + lightweight liveness check
	// This eliminates expensive recurring full scans (1-5s on busy systems)
	let initial_scan = panic::catch_unwind(AssertUnwindSafe(|| {
		detect_game_cached(&mut cache, &shutdown_check)
	}));
	handle_detection_result(initial_scan, &current_game, &current_game_info, &mut cache);
	info!("game detector: initial scan complete");

	let mut last_liveness_check = std::time::Instant::now();
	const LIVENESS_CHECK_INTERVAL: Duration = Duration::from_secs(5);

	while !shutdown.load(Ordering::Relaxed) {
		// Lightweight: Only check if cached game still exists (1 stat call vs 1000s)
		// Run every 5 seconds (more frequent than old 30s full scan, but 1000x cheaper)
		if last_liveness_check.elapsed() >= LIVENESS_CHECK_INTERVAL {
			last_liveness_check = std::time::Instant::now();
			if let Some(ref game) = cache.last_game {
				if !process_exists(game.tgid) {
					info!("game detector: cached game '{}' exited", game.name);
					cache.last_game = None;
					current_game.store(0, Ordering::Relaxed);
					current_game_info.store(Arc::new(None));
				}
			}
		}

		// Process inotify events for new processes (primary detection method)
		if let Some(ref mut inotify_instance) = inotify {
			// inotify-based instant detection
			let mut buffer = [0u8; 4096];
			match inotify_instance.read_events(&mut buffer) {
				Ok(events) => {
					// Process new PIDs from inotify events
					for event in events {
						if event.mask.contains(EventMask::CREATE | EventMask::ISDIR) {
							if let Some(name) = event.name {
								if let Some(name_str) = name.to_str() {
									if let Ok(pid) = name_str.parse::<u32>() {
										// New process detected, check immediately
										let detection_result = panic::catch_unwind(AssertUnwindSafe(|| {
											check_new_pid(pid, &mut cache)
										}));
										handle_detection_result(detection_result, &current_game, &current_game_info, &mut cache);
									}
								}
							}
						}
					}
				},
				Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
					// No events, this is normal
				},
				Err(e) => {
					warn!("game detector: inotify error: {}, disabling inotify", e);
					inotify = None; // Fall back to polling
				}
			}
		} else {
			// Fallback: inotify disabled/unavailable, do lightweight scan every 5s
			if last_liveness_check.elapsed() >= Duration::from_secs(5) {
				let detection_result = panic::catch_unwind(AssertUnwindSafe(|| {
					detect_game_cached(&mut cache, &shutdown_check)
				}));
				handle_detection_result(detection_result, &current_game, &current_game_info, &mut cache);
			}
		}

		// Short sleep to check shutdown flag and avoid busy-looping
		thread::sleep(Duration::from_millis(100));
	}

	info!("game detector: stopped");
}

fn handle_detection_result(
	detection_result: Result<Option<GameInfo>, Box<dyn std::any::Any + Send>>,
	current_game: &Arc<AtomicU32>,
	current_game_info: &Arc<ArcSwap<Option<GameInfo>>>,
	cache: &mut ProcessCache,
) {
	match detection_result {
		Ok(Some(game)) => {
			let prev = current_game.swap(game.tgid, Ordering::Relaxed);
			if prev != game.tgid {
				info!("game detector: found game '{}' (tgid={}, wine={}, steam={})",
					game.name, game.tgid, game.is_wine, game.is_steam);
			}
			// PERF: Lock-free update via ArcSwap (writers contend on atomic swap, readers never block)
			current_game_info.store(Arc::new(Some(game)));
		}
		Ok(None) => {
			let prev = current_game.swap(0, Ordering::Relaxed);
			if prev != 0 {
				info!("game detector: game closed");
			}
			// Clear game info
			current_game_info.store(Arc::new(None));
		}
		Err(panic_err) => {
			warn!("game detector: detection panicked, clearing cache and continuing");
			cache.clear();
			cache.last_game = None;
			current_game.store(0, Ordering::Relaxed);
			current_game_info.store(Arc::new(None));

			if let Some(msg) = panic_err.downcast_ref::<&str>() {
				warn!("game detector: panic message: {}", msg);
			} else if let Some(msg) = panic_err.downcast_ref::<String>() {
				warn!("game detector: panic message: {}", msg);
			}
		}
	}
}

fn check_new_pid(pid: u32, cache: &mut ProcessCache) -> Option<GameInfo> {
	// If we already checked this PID or it's our cached game, skip
	if cache.contains(pid) {
		return cache.last_game.clone();
	}

	cache.insert(pid);

	// Check if this new PID is a game
	if let Some(info) = check_process(pid) {
		let score = calculate_score(&info);
		// Only update if this is a good candidate (positive score)
		if score > 0 {
			cache.last_game = Some(info.clone());
			return Some(info);
		}
	}

	// Return current game if it's still running
	if let Some(ref game) = cache.last_game {
		if process_exists(game.tgid) {
			return Some(game.clone());
		}
		cache.last_game = None;
	}

	None
}

fn detect_game_cached(cache: &mut ProcessCache, shutdown: &Arc<AtomicBool>) -> Option<GameInfo> {
	if let Some(ref game) = cache.last_game {
		if process_exists(game.tgid) {
			return Some(game.clone());
		}
		cache.last_game = None;
	}

	let proc_entries = fs::read_dir("/proc").ok()?;
	let mut best_game: Option<GameInfo> = None;
	let mut best_score = 0i32;
	let mut entries_checked = 0u32;

	for entry in proc_entries.flatten() {
		// BUG FIX: Check shutdown flag periodically during long /proc scan
		// After 1+ hours, /proc can have thousands of entries (processes + threads)
		// This scan can take 1-5 seconds on busy systems, blocking Ctrl+C
		// Check every 100 entries (~every 10-50ms) for responsive shutdown
		entries_checked += 1;
		if entries_checked % 100 == 0 && shutdown.load(Ordering::Relaxed) {
			info!("game detector: shutdown requested during /proc scan, aborting");
			return cache.last_game.clone();
		}

		let file_name = entry.file_name();
		let pid_str = file_name.to_str()?;

		if let Ok(pid) = pid_str.parse::<u32>() {
			if !cache.contains(pid) {
				if let Some(info) = check_process(pid) {
					let score = calculate_score(&info);
					if score > best_score {
						best_score = score;
						best_game = Some(info);
					}
				}
				cache.insert(pid);
			}
		}
	}

	// OPTIMIZATION: Ring buffer automatically handles eviction
	// No need for manual cleanup or size management

	if let Some(game) = best_game {
		cache.last_game = Some(game.clone());
		Some(game)
	} else {
		cache.last_game.clone()
	}
}

// PERF: Avoid string allocation by using stack buffer for PID formatting
fn process_exists(pid: u32) -> bool {
	use std::io::Write;
	// Stack-allocated buffer for "/proc/NNNNNN" (max 6 digits for PID)
	let mut buf = [0u8; 16];
	let mut cursor = std::io::Cursor::new(&mut buf[..]);
	// Write directly to stack buffer (no heap allocation)
	let _ = write!(cursor, "/proc/{}", pid);
	let len = cursor.position() as usize;
	let path_str = unsafe { std::str::from_utf8_unchecked(&buf[..len]) };
	std::path::Path::new(path_str).exists()
}

/// Process resource usage stats for game detection
struct ProcessStats {
	threads: usize,
	vmrss_kb: u64,  // Resident memory in KB
}

/// Read process stats from /proc/{pid}/status
fn get_process_stats(pid: u32) -> Option<ProcessStats> {
	const MAX_STATUS_SIZE: usize = 8192;
	let status_path = format!("/proc/{}/status", pid);
	let status_bytes = read_file_limited(&status_path, MAX_STATUS_SIZE)?;
	let status = String::from_utf8_lossy(&status_bytes);

	let mut threads = 0;
	let mut vmrss_kb = 0;

	for line in status.lines() {
		if line.starts_with("Threads:") {
			threads = line.split_whitespace()
				.nth(1)?
				.parse::<usize>()
				.ok()?;
		} else if line.starts_with("VmRSS:") {
			vmrss_kb = line.split_whitespace()
				.nth(1)?
				.parse::<u64>()
				.ok()?;
		}
	}

	Some(ProcessStats { threads, vmrss_kb })
}

fn calculate_score(game: &GameInfo) -> i32 {
	let mut score = 0;

	// **STRONGEST SIGNAL**: MangohHUD presence = definitely a game!
	// Check if this process has MangohHUD shared memory active
	if has_mangohud_shm(game.tgid) {
		score += 1000;  // Massive boost - MangohHUD = game
		info!("game detector: MangohHUD detected for PID {} ({}), strong game signal",
		      game.tgid, game.name);
	}

	// **RESOURCE USAGE HEURISTICS**: Distinguish real games from launchers
	// Real games: 50+ threads, 500MB+ memory
	// Launchers: <10 threads, <100MB memory
	if let Some(stats) = get_process_stats(game.tgid) {
		// Thread count is a VERY strong signal
		if stats.threads >= 50 {
			score += 300;  // Many threads = definitely the actual game
		} else if stats.threads >= 20 {
			score += 150;  // Moderate threads = likely game, not launcher
		} else if stats.threads < 5 {
			score -= 200;  // Few threads = likely launcher/wrapper
		}

		// Memory usage (in MB)
		let mem_mb = stats.vmrss_kb / 1024;
		if mem_mb >= 500 {
			score += 200;  // High memory = actual game
		} else if mem_mb >= 100 {
			score += 50;   // Moderate memory
		} else if mem_mb < 50 {
			score -= 100;  // Low memory = likely launcher
		}
	}

	if game.is_wine { score += 100; }
	if game.is_steam { score += 50; }

	let name_lower = game.name.to_lowercase();

	// Strongly deprioritize wrapper processes and system utilities
	// These are typically launchers, not the actual game process
	if matches!(name_lower.as_str(),
		// Proton/Wine wrapper scripts
		"python" | "python3" | "python2" | "bash" | "sh" | "zsh" | "fish" |
		// Steam container processes
		"reaper" | "pressure-vessel" |
		// Generic launchers and Steam UI
		"steam.exe" | "launcher" | "gameoverlayui" | "steamwebhelper" |
		// Wine system processes (not actual games)
		"services.exe" | "winedevice.exe" | "plugplay.exe" | "explorer.exe" |
		"svchost.exe" | "rpcss.exe" | "wineboot.exe" |
		// Battle.net/launcher processes
		"battle.net.exe" | "agent.exe" | "blizzard.exe" |
		// Scheduler processes
		"scx_gamer" | "scx_rusty" | "scx_lavd" | "scx_bpfland"
	) {
		score -= 500;  // Very strong penalty to ensure wrappers never win
	}

	// Prioritize .exe processes (actual Windows games, not wrappers)
	if name_lower.ends_with(".exe") {
		score += 200;
	}

	// Additional boost for processes with "game" or "client" in the name
	if name_lower.contains("game") || name_lower.contains("client") {
		score += 50;
	}

	score
}

/// Fast check if a PID has MangohHUD active (indicates it's a game)
fn has_mangohud_shm(pid: u32) -> bool {
	// Check for MangohHUD shared memory files in /dev/shm
	let shm_paths = [
		format!("/dev/shm/mangoapp.{}", pid),
		format!("/dev/shm/MangoHud.{}", pid),
	];

	shm_paths.iter().any(|p| PathBuf::from(p).exists())
}

/// Safely read a file with a hard size limit to prevent memory exhaustion.
/// Returns None if file doesn't exist or read fails.
/// Returns partial data if file exceeds max_size (useful for analyzing prefixes).
fn read_file_limited(path: &str, max_size: usize) -> Option<Vec<u8>> {
	let file = fs::File::open(path).ok()?;
	let mut buffer = Vec::with_capacity(max_size.min(256)); // Start small, grow if needed

	// Read up to max_size bytes using take() to enforce hard limit
	let mut limited = file.take(max_size as u64);
	limited.read_to_end(&mut buffer).ok()?;

	// Always return the prefix data, even if truncated.
	// This allows game detection from cmdline prefixes (e.g., "/usr/bin/wine ...").
	// Empty reads (0 bytes) are valid for some proc files.
	Some(buffer)
}

fn check_process(pid: u32) -> Option<GameInfo> {
	const MAX_COMM_SIZE: usize = 256;
	const MAX_CMDLINE_SIZE: usize = 4096;

	let comm_path = format!("/proc/{}/comm", pid);
	let cmdline_path = format!("/proc/{}/cmdline", pid);

	// Read comm with size limit enforced during read (prevents memory exhaustion)
	let comm_bytes = read_file_limited(&comm_path, MAX_COMM_SIZE)?;
	let comm = String::from_utf8_lossy(&comm_bytes);
	let comm = comm.trim();
	let comm_lower = comm.to_lowercase();

	// Read cmdline with size limit enforced during read
	let cmdline_bytes = read_file_limited(&cmdline_path, MAX_CMDLINE_SIZE)?;
	let cmdline = String::from_utf8_lossy(&cmdline_bytes);
	let cmdline_lower = cmdline.to_lowercase();

	// Detect Wine/Proton processes:
	// 1. Process name contains wine/proton keywords
	// 2. Command line contains wine/proton keywords
	// 3. Steam game with .exe (cmdline has both "steam" and ".exe")
	// 4. Command line looks like a Windows path (e.g., "C:\Program Files\...")
	let is_wine = comm_lower.contains("wine") ||
		comm_lower.contains("proton") ||
		cmdline_lower.contains("wine") ||
		cmdline_lower.contains("proton") ||
		(cmdline_lower.contains("steam") && cmdline_lower.contains(".exe")) ||
		(cmdline_lower.contains(".exe") && (
			cmdline_lower.contains("c:\\") ||
			cmdline_lower.contains("z:\\") ||
			cmdline.contains(":\\")  // Any Windows drive letter pattern
		));

	let is_steam = cmdline_lower.contains("steam") ||
		cmdline_lower.contains("reaper") ||
		check_steam_cgroup(pid);

	// Filter out Wine/Proton system processes and infrastructure
	// These are system tools, not actual games
	if cmdline_lower.contains(":\\windows\\") ||                // Any Windows system directory (includes system32)
	   cmdline_lower.contains("\\compatibilitytools.d\\") ||    // Proton tools directory
	   cmdline_lower.contains("/compatibilitytools.d/") ||
	   cmdline_lower.contains("\\xalia\\") ||                    // Proton accessibility tool
	   cmdline_lower.contains(":\\programdata\\battle.net\\") {  // Battle.net launcher (any drive)
		return None;
	}

	// Only consider processes that are Wine/Proton games OR explicitly Steam games
	// This filters out system tools that happen to be in Steam's cgroup
	if is_wine || (is_steam && (cmdline_lower.contains("steam") || cmdline_lower.contains("reaper"))) {
		Some(GameInfo {
			tgid: pid,
			name: comm.to_string(),
			is_wine,
			is_steam,
		})
	} else {
		None
	}
}

fn check_steam_cgroup(pid: u32) -> bool {
	const MAX_CGROUP_SIZE: usize = 8192;

	let cgroup_path = format!("/proc/{}/cgroup", pid);
	if let Some(cgroup_bytes) = read_file_limited(&cgroup_path, MAX_CGROUP_SIZE) {
		let content = String::from_utf8_lossy(&cgroup_bytes);
		content.contains("steam") || content.contains("app.slice")
	} else {
		false
	}
}