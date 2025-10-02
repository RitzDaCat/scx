use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use inotify::{Inotify, WatchMask, EventMask};
use log::{info, warn};

#[derive(Debug, Clone)]
pub struct GameInfo {
	pub tgid: u32,
	pub name: String,
	pub is_wine: bool,
	pub is_steam: bool,
}

struct ProcessCache {
	seen_pids: HashSet<u32>,
	last_game: Option<GameInfo>,
}

pub struct GameDetector {
	current_game: Arc<AtomicU32>,
	shutdown: Arc<AtomicBool>,
	_thread: Option<JoinHandle<()>>,
}

impl GameDetector {
	pub fn new() -> Self {
		let current_game = Arc::new(AtomicU32::new(0));
		let shutdown = Arc::new(AtomicBool::new(false));
		let thread_game = Arc::clone(&current_game);
		let thread_shutdown = Arc::clone(&shutdown);

		let handle = thread::Builder::new()
			.name("game-detect".to_string())
			.spawn(move || detection_loop(thread_game, thread_shutdown))
			.expect("failed to spawn game detector thread");

		Self {
			current_game,
			shutdown,
			_thread: Some(handle),
		}
	}

	pub fn get_game_tgid(&self) -> u32 {
		self.current_game.load(Ordering::Relaxed)
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

fn detection_loop(current_game: Arc<AtomicU32>, shutdown: Arc<AtomicBool>) {
	info!("game detector: starting");
	let mut cache = ProcessCache {
		seen_pids: HashSet::new(),
		last_game: None,
	};

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

	let mut last_full_scan = std::time::Instant::now();
	const FULL_SCAN_INTERVAL: Duration = Duration::from_secs(30);

	while !shutdown.load(Ordering::Relaxed) {
		// Check if we should do a full scan (every 30s as safety net)
		let should_full_scan = last_full_scan.elapsed() >= FULL_SCAN_INTERVAL;

		if should_full_scan {
			// Full scan fallback
			last_full_scan = std::time::Instant::now();
			let detection_result = panic::catch_unwind(AssertUnwindSafe(|| {
				detect_game_cached(&mut cache)
			}));
			handle_detection_result(detection_result, &current_game, &mut cache);
		} else if let Some(ref mut inotify_instance) = inotify {
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
										handle_detection_result(detection_result, &current_game, &mut cache);
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
		}

		// Short sleep to check shutdown flag and avoid busy-looping
		thread::sleep(Duration::from_millis(100));
	}

	info!("game detector: stopped");
}

fn handle_detection_result(
	detection_result: Result<Option<GameInfo>, Box<dyn std::any::Any + Send>>,
	current_game: &Arc<AtomicU32>,
	cache: &mut ProcessCache,
) {
	match detection_result {
		Ok(Some(game)) => {
			let prev = current_game.swap(game.tgid, Ordering::Relaxed);
			if prev != game.tgid {
				info!("game detector: found game '{}' (tgid={}, wine={}, steam={})",
					game.name, game.tgid, game.is_wine, game.is_steam);
			}
		}
		Ok(None) => {
			let prev = current_game.swap(0, Ordering::Relaxed);
			if prev != 0 {
				info!("game detector: game closed");
			}
		}
		Err(panic_err) => {
			warn!("game detector: detection panicked, clearing cache and continuing");
			cache.seen_pids.clear();
			cache.last_game = None;
			current_game.store(0, Ordering::Relaxed);

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
	if cache.seen_pids.contains(&pid) {
		return cache.last_game.clone();
	}

	cache.seen_pids.insert(pid);

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

fn detect_game_cached(cache: &mut ProcessCache) -> Option<GameInfo> {
	if let Some(ref game) = cache.last_game {
		if process_exists(game.tgid) {
			return Some(game.clone());
		}
		cache.last_game = None;
	}

	let proc_entries = fs::read_dir("/proc").ok()?;
	let mut current_pids = HashSet::new();
	let mut best_game: Option<GameInfo> = None;
	let mut best_score = 0i32;

	for entry in proc_entries.flatten() {
		let file_name = entry.file_name();
		let pid_str = file_name.to_str()?;

		if let Ok(pid) = pid_str.parse::<u32>() {
			current_pids.insert(pid);

			if !cache.seen_pids.contains(&pid) {
				if let Some(info) = check_process(pid) {
					let score = calculate_score(&info);
					if score > best_score {
						best_score = score;
						best_game = Some(info);
					}
				}
			}
		}
	}

	cache.seen_pids.retain(|pid| current_pids.contains(pid));
	cache.seen_pids.extend(current_pids);

	// Periodically shrink the HashSet to prevent unbounded memory growth
	// Threshold chosen to balance memory efficiency vs reallocation overhead
	if cache.seen_pids.len() > 10000 {
		cache.seen_pids.shrink_to_fit();
	}

	if let Some(game) = best_game {
		cache.last_game = Some(game.clone());
		Some(game)
	} else {
		cache.last_game.clone()
	}
}

fn process_exists(pid: u32) -> bool {
	std::path::Path::new(&format!("/proc/{}", pid)).exists()
}

fn calculate_score(game: &GameInfo) -> i32 {
	let mut score = 0;
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
		// Generic launchers
		"steam.exe" | "launcher" | "gameoverlayui" |
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