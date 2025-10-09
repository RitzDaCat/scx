// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer: BPF LSM Game Detection (Userspace Consumer)
// Copyright (c) 2025 RitzDaCat
//
// Ring buffer consumer for kernel-level game detection events.
// Eliminates expensive /proc scanning by processing only BPF-filtered candidates.

use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use arc_swap::ArcSwap;
use libbpf_rs::RingBufferBuilder;
use log::{info, warn};

/// Process event from BPF ring buffer
/// Must match struct process_event in game_detect.bpf.h
#[repr(C)]
#[derive(Debug)]
struct ProcessEvent {
	type_: u32,           // event type
	pid: u32,             // Process PID
	parent_pid: u32,      // Parent PID
	flags: u32,           // game_flags bitmask
	timestamp: u64,       // Event timestamp
	comm: [u8; 16],       // Process name
	parent_comm: [u8; 16], // Parent name
}

const GAME_EVENT_EXEC: u32 = 1;
const GAME_EVENT_EXIT: u32 = 2;

#[allow(dead_code)]
const FLAG_WINE: u32 = 1 << 0;
#[allow(dead_code)]
const FLAG_STEAM: u32 = 1 << 1;
#[allow(dead_code)]
const FLAG_EXE: u32 = 1 << 2;

#[derive(Debug, Clone)]
pub struct GameInfo {
	pub tgid: u32,
	pub name: String,
	pub is_wine: bool,
	pub is_steam: bool,
}

/// BPF LSM-based game detector
/// Uses kernel-level hooks to track process lifecycle with minimal overhead
pub struct BpfGameDetector {
	current_game: Arc<AtomicU32>,
	current_game_info: Arc<ArcSwap<Option<GameInfo>>>,
	shutdown: Arc<AtomicBool>,
	_thread: Option<JoinHandle<()>>,
}

impl BpfGameDetector {
	/// Create new BPF LSM game detector
	///
	/// Spawns consumer thread that processes ring buffer events from kernel.
	/// Thread polls with 200ms timeout for responsive shutdown (low overhead).
	pub fn new(skel: &mut crate::BpfSkel) -> anyhow::Result<Self> {
		let current_game = Arc::new(AtomicU32::new(0));
		let current_game_info = Arc::new(ArcSwap::from_pointee(None));
		let shutdown = Arc::new(AtomicBool::new(false));

		// Clone for thread
		let thread_game = Arc::clone(&current_game);
		let thread_game_info = Arc::clone(&current_game_info);
		let thread_shutdown = Arc::clone(&shutdown);

		// Build ring buffer consumer with callback
		let mut builder = RingBufferBuilder::new();
		builder.add(&skel.maps.process_events, move |data: &[u8]| -> i32 {
			handle_process_event(data, &thread_game, &thread_game_info)
		})?;

		let ringbuf = builder.build()?;

		// HYBRID APPROACH: Do optimized initial scan for already-running games
		// This solves the "game launched before scheduler" problem
		// BPF LSM only catches NEW exec() calls, not existing processes
		//
		// Performance: 200-800ms one-time cost at startup
		// Benefit: Detects games already running (e.g., World of Warcraft)
		if let Some(game) = detect_initial_game() {
			current_game.store(game.tgid, Ordering::Relaxed);
			current_game_info.store(Arc::new(Some(game)));
		}

		// Spawn consumer thread for ongoing BPF LSM events
		let handle = thread::Builder::new()
			.name("bpf-game-detect".to_string())
			.spawn(move || {
				info!("BPF LSM game detector: starting event consumer");

				while !thread_shutdown.load(Ordering::Relaxed) {
					// PERF: 100ms poll (faster shutdown response)
					// Reduced from 200ms to improve Ctrl+C responsiveness
					match ringbuf.poll(Duration::from_millis(100)) {
						Ok(_) => {
							// Events processed, continue
						}
						Err(e) => {
							// Poll error (not timeout - timeout returns Ok)
							warn!("BPF LSM: ring buffer poll error: {}", e);
							thread::sleep(Duration::from_millis(100));
						}
					}
					// Check shutdown flag frequently
				}

				info!("BPF LSM game detector: stopped");
			})?;

		info!("BPF LSM game detector: initialized (kernel-level tracking active)");

		Ok(Self {
			current_game,
			current_game_info,
			shutdown,
			_thread: Some(handle),
		})
	}

	#[inline]
	pub fn get_game_tgid(&self) -> u32 {
		self.current_game.load(Ordering::Relaxed)
	}

	#[inline]
	pub fn get_game_info(&self) -> Option<GameInfo> {
		(**self.current_game_info.load()).clone()
	}
}

impl Drop for BpfGameDetector {
	fn drop(&mut self) {
		// Signal thread to shutdown
		self.shutdown.store(true, Ordering::Relaxed);

		if let Some(handle) = self._thread.take() {
			// Wait up to 500ms for graceful shutdown (reduced from 1s)
			// Ring buffer poll has 100ms timeout, should exit within 200ms max
			for i in 0..5 {
				if handle.is_finished() {
					let _ = handle.join();
					info!("BPF LSM game detector: clean shutdown ({}ms)", i * 100);
					return;
				}
				thread::sleep(Duration::from_millis(100));
			}

			// Thread didn't exit - this is OK, it will be killed when process exits
			// Ring buffer polling sometimes blocks on kernel cleanup
			warn!("BPF LSM game detector: thread didn't exit within 500ms (hung on BPF cleanup)");
			warn!("This is normal - kernel will clean up on process exit");
		}
	}
}

/// Ring buffer callback - processes events from kernel
///
/// This runs in the context of ringbuf.poll() and must be fast.
/// Heavy work (full classification) is done inline since events are rare (1-5/min).
fn handle_process_event(
	data: &[u8],
	current_game: &Arc<AtomicU32>,
	current_game_info: &Arc<ArcSwap<Option<GameInfo>>>
) -> i32 {
	// Parse event from BPF
	if data.len() < std::mem::size_of::<ProcessEvent>() {
		warn!("BPF LSM: invalid event size: {}", data.len());
		return -1;
	}

	let evt: &ProcessEvent = unsafe { &*(data.as_ptr() as *const ProcessEvent) };

	match evt.type_ {
		GAME_EVENT_EXEC => {
			// BPF already filtered: this is a HIGH PROBABILITY game candidate
			// Do deep classification with /proc (only 1-5 per minute vs 1000s/sec before!)

			let pid = evt.pid;
			let flags = evt.flags;

			// Convert comm from fixed array to String
			let comm = String::from_utf8_lossy(&evt.comm)
				.trim_end_matches('\0')
				.to_string();

			info!("BPF LSM: Candidate process: {} (pid={}, flags={:#x})", comm, pid, flags);

			// Deep classification: read /proc for detailed analysis
			if let Some(game) = validate_game_candidate(pid, flags) {
				let prev = current_game.swap(game.tgid, Ordering::Relaxed);
				if prev != game.tgid {
					info!("BPF LSM: Game detected: '{}' (pid={}, wine={}, steam={})",
						  game.name, game.tgid, game.is_wine, game.is_steam);
				}
				current_game_info.store(Arc::new(Some(game)));
			}
		}
		GAME_EVENT_EXIT => {
			let pid = evt.pid;
			let prev = current_game.swap(0, Ordering::Relaxed);
			if prev == pid {
				info!("BPF LSM: Game exited (pid={})", pid);
				current_game_info.store(Arc::new(None));
			}
		}
		_ => {
			warn!("BPF LSM: Unknown event type: {}", evt.type_);
		}
	}

	0  // Success
}

/// Deep classification using /proc (only called for BPF-filtered candidates)
///
/// Frequency: ~1-5 calls per MINUTE (vs 1000s/sec with old full /proc scanning)
/// Cost: 200-500μs per call (reads cmdline, status, cgroup)
/// Total overhead: ~10-50μs/sec (negligible)
fn validate_game_candidate(pid: u32, bpf_flags: u32) -> Option<GameInfo> {
	// Read cmdline for Wine path detection AND executable name
	let cmdline = read_file_limited(&format!("/proc/{}/cmdline", pid), 4096)?;
	let cmdline_str = String::from_utf8_lossy(&cmdline);
	let cmdline_lower = cmdline_str.to_lowercase();

	// Extract executable name from cmdline (more accurate than comm)
	// cmdline is null-separated, first arg is the executable path
	let exe_name = cmdline_str
		.split('\0')
		.next()
		.and_then(|path| {
			// Get basename (everything after last /)
			path.rsplit('/').next()
		})
		.unwrap_or("")
		.trim()
		.to_string();

	// Read comm as fallback if cmdline parsing fails
	let comm = read_file_limited(&format!("/proc/{}/comm", pid), 256)?;
	let comm_str = String::from_utf8_lossy(&comm).trim().to_string();

	// Use exe_name if available, fallback to comm
	let game_name = if !exe_name.is_empty() {
		exe_name
	} else {
		comm_str.clone()
	};

	// Enhanced Wine/Proton detection (using cmdline)
	let is_wine = (bpf_flags & 0x1) != 0 ||  // BPF detected Wine
		cmdline_lower.contains("wine") ||
		cmdline_lower.contains("proton") ||
		(cmdline_lower.contains("steam") && cmdline_lower.contains(".exe")) ||
		cmdline_lower.contains("c:\\") ||
		cmdline_lower.contains("z:\\") ||
		cmdline_str.contains(":\\");  // Windows drive letter

	// Enhanced Steam detection
	let is_steam = (bpf_flags & 0x2) != 0 ||  // BPF detected Steam
		cmdline_lower.contains("steam") ||
		cmdline_lower.contains("reaper") ||
		check_steam_cgroup(pid);

	// Filter out Wine/Proton system processes
	if cmdline_lower.contains(":\\windows\\") ||
	   cmdline_lower.contains("\\compatibilitytools.d\\") ||
	   cmdline_lower.contains("/compatibilitytools.d/") ||
	   cmdline_lower.contains("\\xalia\\") ||
	   cmdline_lower.contains(":\\programdata\\battle.net\\") {
		return None;
	}

	// Score the candidate (same logic as old detector)
	let mut score = 0i32;

	// BPF already gave us a head start with flags
	if (bpf_flags & 0x1) != 0 { score += 100; }  // Wine
	if (bpf_flags & 0x8) != 0 { score += 50; }   // Parent Wine

	// MangohHUD detection (strong signal)
	if has_mangohud_shm(pid) {
		score += 1000;
		info!("BPF LSM: MangohHUD detected for pid {} ({}), strong game signal", pid, game_name);
	}

	// Resource usage heuristics
	if let Some(stats) = get_process_stats(pid) {
		if stats.threads >= 50 {
			score += 300;
		} else if stats.threads >= 20 {
			score += 150;
		} else if stats.threads < 5 {
			score -= 200;
		}

		let mem_mb = stats.vmrss_kb / 1024;
		if mem_mb >= 500 {
			score += 200;
		} else if mem_mb >= 100 {
			score += 50;
		} else if mem_mb < 50 {
			score -= 100;
		}
	}

	if is_wine { score += 100; }
	if is_steam { score += 50; }

	// Deprioritize wrappers and Steam infrastructure
	let name_lower = game_name.to_lowercase();
	if matches!(name_lower.as_str(),
		"python" | "python3" | "bash" | "sh" |
		"reaper" | "pressure-vessel" |
		"steam.exe" | "launcher" | "steamwebhelper" |
		"services.exe" | "winedevice.exe" | "explorer.exe" |
		"scx_gamer" | "scx_rusty"
	) {
		score -= 500;
	}

	if name_lower.ends_with(".exe") {
		score += 200;
	}

	// Only accept if score is positive
	if score > 0 {
		Some(GameInfo {
			tgid: pid,
			name: game_name,  // Use extracted exe name instead of comm
			is_wine,
			is_steam,
		})
	} else {
		None
	}
}

// Helper functions (reused from old game_detect.rs)

fn read_file_limited(path: &str, max_size: usize) -> Option<Vec<u8>> {
	let file = fs::File::open(path).ok()?;
	let mut buffer = Vec::with_capacity(max_size.min(256));
	let mut limited = file.take(max_size as u64);
	limited.read_to_end(&mut buffer).ok()?;
	Some(buffer)
}

struct ProcessStats {
	threads: usize,
	vmrss_kb: u64,
}

fn get_process_stats(pid: u32) -> Option<ProcessStats> {
	let status_bytes = read_file_limited(&format!("/proc/{}/status", pid), 8192)?;
	let status = String::from_utf8_lossy(&status_bytes);

	let mut threads = 0;
	let mut vmrss_kb = 0;

	for line in status.lines() {
		if line.starts_with("Threads:") {
			threads = line.split_whitespace().nth(1)?.parse().ok()?;
		} else if line.starts_with("VmRSS:") {
			vmrss_kb = line.split_whitespace().nth(1)?.parse().ok()?;
		}
	}

	Some(ProcessStats { threads, vmrss_kb })
}

fn has_mangohud_shm(pid: u32) -> bool {
	let shm_paths = [
		format!("/dev/shm/mangoapp.{}", pid),
		format!("/dev/shm/MangoHud.{}", pid),
	];
	shm_paths.iter().any(|p| PathBuf::from(p).exists())
}

fn check_steam_cgroup(pid: u32) -> bool {
	if let Some(cgroup_bytes) = read_file_limited(&format!("/proc/{}/cgroup", pid), 8192) {
		let content = String::from_utf8_lossy(&cgroup_bytes);
		content.contains("steam") || content.contains("app.slice")
	} else {
		false
	}
}

/// Fast filter: Check if process name contains game-related keywords
///
/// Rejects 90-95% of system processes in <10μs per process.
/// Only reads comm (16 bytes) to avoid expensive cmdline/status reads.
///
/// Returns: true if potential game, false if definitely not a game
fn has_game_keywords(comm: &str) -> bool {
	let comm_lower = comm.to_lowercase();

	// FAST REJECT: Kernel threads (eliminate 70-80% immediately)
	// Kernel threads contain '/' or start with 'k' and contain specific patterns
	if comm.contains('/') ||
	   comm_lower.starts_with("kworker") ||
	   comm_lower.starts_with("migration") ||
	   comm_lower.starts_with("ksoftirqd") ||
	   comm_lower.starts_with("nvidia") ||
	   comm_lower.starts_with("irq/") ||
	   comm_lower.starts_with("scsi_") ||
	   comm_lower.starts_with("btrfs") ||
	   comm_lower.starts_with("khugepaged") ||
	   comm_lower.starts_with("kcompactd") ||
	   comm_lower.starts_with("ksmd") ||
	   comm_lower.starts_with("kswapd") ||
	   comm_lower.starts_with("kdevtmpfs") ||
	   comm_lower.starts_with("kauditd") ||
	   comm_lower.starts_with("kthreadd") ||
	   comm_lower.starts_with("oom_reaper") ||
	   comm_lower.starts_with("watchdogd") ||
	   comm_lower.starts_with("rcu") ||
	   comm_lower.starts_with("rcub") ||
	   comm_lower.starts_with("idle_inject") ||
	   comm_lower.starts_with("cpuhp") ||
	   comm_lower.starts_with("uvm") ||
	   comm_lower.starts_with("pool_workqueue") {
		return false;
	}

	// POSITIVE SIGNALS: Strong game indicators (return early)
	if comm_lower.contains("wine") ||
	   comm_lower.contains("proton") ||
	   comm_lower.contains(".exe") ||
	   comm_lower.ends_with(".ex") ||  // Truncated .exe (kernel comm limit is 16 chars)
	   comm_lower.contains("game") ||
	   comm_lower.contains("warframe") {  // Warframe detection (common game)
		return true;
	}

	// Steam processes - only some are games
	// "steam" alone is too broad (steamwebhelper, steam.exe launcher, etc.)
	// Let these through for deeper validation
	if comm_lower.contains("steam") || comm_lower.contains("reaper") {
		return true;
	}

	// NEGATIVE SIGNALS: System binaries, shells, utils, desktop apps
	if matches!(comm_lower.as_str(),
		"systemd" | "systemd-journal" | "systemd-logind" | "systemd-resolve" |
		"systemd-timesyn" | "systemd-udevd" | "systemd-userdbd" | "systemd-userwor" |
		"bash" | "sh" | "zsh" | "fish" |
		"python" | "python3" | "perl" | "node" |
		"git" | "cargo" | "gcc" | "clang" |
		"cat" | "ls" | "grep" | "sed" | "awk" |
		"vim" | "nvim" | "emacs" |
		"ssh" | "sshd" | "sudo" | "su" |
		"dbus-broker" | "dbus-broker-lau" | "polkit" | "polkitd" |
		"pipewire" | "wireplumber" | "pipewire-pulse" |
		"kwin_wayland" | "kwin_wayland_wr" | "plasmashell" |
		"xwayland" | "gvfsd" | "gvfsd-fuse" | "gvfsd-metadata" |
		"discord" | "chromium" | "firefox" |
		"konsole" | "alacritty" | "kitty" |
		"scx_gamer" | "scx_rusty" | "scx_lavd" | "scx_loader" | "scx-manager" |
		"postgres" | "ollama" | "dotnet" | "idea" |
		"avahi-daemon" | "rtkit-daemon" | "upowerd" | "udisksd" |
		"at-spi-bus-laun" | "at-spi2-registr" | "dconf-service" |
		"xdg-desktop-por" | "xdg-permission-" | "xdg-document-po" | "xdg-open" |
		"fusermount3" | "srt-logger" | "srt-bwrap" | "pv-adverb" |
		"ksmserver" | "kded6" | "kactivitymanage" | "kaccess" | "kdeconnectd" |
		"polkit-kde-auth" | "org_kde_powerde" | "gmenudbusmenupr" | "xembedsniproxy" |
		"baloorunner" | "kwalletd6" | "pika-backup-mon" | "xsettingsd" |
		"chrome_crashpad" | "appimagelaunche" | "ksecretd" | "startplasma-way" |
		"plasma-browser-" | "goxlr-daemon" | "arch-update" | "boltd" |
		"psimon" | "fsnotifier" | "binfmt-bypass" | "mainthread" |
		"inotifywait" | "bpftune" | "sddm" | "sddm-helper" |
		"memfd:runtime" | "(sd-pam)" | "vidmem lazy free"
	) {
		return false;
	}

	// Default: Reject (changed from permissive 'true')
	// Only allow processes that explicitly matched positive signals above
	// This prevents kernel threads and obscure processes from passing through
	false
}

/// Optimized initial /proc scan to detect already-running games
///
/// Performance optimization: Two-phase filtering
/// Phase 1: Fast reject 90-95% by checking comm only (16 bytes)
/// Phase 2: Deep validate remaining 5-10% (full classification)
///
/// Expected performance:
/// - Processes scanned: 500-2000
/// - Phase 1 reads: 500-2000 × 16 bytes = 8-32KB
/// - Phase 2 reads: 25-100 × ~8KB = 200-800KB
/// - Total time: 200-800ms (vs 1-5s for full scan)
/// - Improvement: 2-6× faster
fn detect_initial_game() -> Option<GameInfo> {
	info!("BPF LSM: Starting optimized initial scan for already-running games...");
	let start = std::time::Instant::now();

	let proc_entries = match fs::read_dir("/proc") {
		Ok(entries) => entries,
		Err(e) => {
			warn!("BPF LSM: Failed to read /proc: {}", e);
			return None;
		}
	};

	let mut best_game: Option<GameInfo> = None;
	let mut best_score = 0i32;
	let mut total_checked = 0u32;
	let mut candidates_validated = 0u32;

	for entry in proc_entries.flatten() {
		// Parse PID from directory name
		let file_name = entry.file_name();
		let pid_str = match file_name.to_str() {
			Some(s) => s,
			None => continue,
		};
		let pid: u32 = match pid_str.parse() {
			Ok(p) => p,
			Err(_) => continue,  // Not a PID (e.g., "self", "thread-self")
		};

		total_checked += 1;

		// PHASE 1: Fast filter by comm (16 bytes, <10μs per process)
		// Read ONLY comm file first (smallest, fastest)
		let comm = match fs::read_to_string(format!("/proc/{}/comm", pid)) {
			Ok(c) => c.trim().to_string(),
			Err(_) => continue,  // Process exited, skip
		};

		// Quick reject if no game keywords
		if !has_game_keywords(&comm) {
			continue;  // Skip 90-95% of processes here!
		}

		// PHASE 2: Deep validation for candidates (5-10% of processes)
		candidates_validated += 1;
		info!("BPF LSM: Checking candidate: {} (pid={})", comm, pid);

		if let Some(game) = validate_game_candidate(pid, 0) {
			let score = calculate_game_score(&game);
			info!("BPF LSM: Candidate '{}' scored {} (wine={}, steam={}, threads={:?})",
			      game.name, score, game.is_wine, game.is_steam,
			      get_process_stats(pid).map(|s| s.threads));
			if score > best_score {
				best_score = score;
				best_game = Some(game);
			}
		}
	}

	let elapsed = start.elapsed();
	info!("BPF LSM: Initial scan complete in {:.2}ms (checked {} processes, validated {} candidates)",
	      elapsed.as_secs_f64() * 1000.0, total_checked, candidates_validated);

	if let Some(ref game) = best_game {
		info!("BPF LSM: Found running game: '{}' (pid={}, score={})",
		      game.name, game.tgid, best_score);
	} else {
		info!("BPF LSM: No running game detected");
	}

	best_game
}

/// Calculate game detection score
/// Higher score = more confident it's a game
fn calculate_game_score(game: &GameInfo) -> i32 {
	let mut score = 0i32;

	// Wine/Proton/Steam indicators
	if game.is_wine { score += 100; }
	if game.is_steam { score += 50; }

	// Check MangohHUD (very strong signal)
	if has_mangohud_shm(game.tgid) {
		score += 1000;
	}

	// Resource usage (if available)
	if let Some(stats) = get_process_stats(game.tgid) {
		// Thread count heuristic
		if stats.threads >= 50 {
			score += 300;  // Many threads = likely game
		} else if stats.threads >= 20 {
			score += 150;
		} else if stats.threads < 5 {
			score -= 200;  // Few threads = likely launcher/wrapper
		}

		// Memory usage (MB)
		let mem_mb = stats.vmrss_kb / 1024;
		if mem_mb >= 500 {
			score += 200;  // High memory = likely game
		} else if mem_mb >= 100 {
			score += 50;
		} else if mem_mb < 50 {
			score -= 100;  // Low memory = launcher
		}
	}

	// Process name heuristics
	let name_lower = game.name.to_lowercase();

	// Penalize known wrappers heavily
	if matches!(name_lower.as_str(),
		"python" | "python3" | "bash" | "sh" |
		"reaper" | "pressure-vessel" |
		"steam.exe" | "launcher" |
		"services.exe" | "winedevice.exe" | "explorer.exe" |
		"scx_gamer" | "scx_rusty"
	) {
		score -= 500;
	}

	// Boost for .exe processes
	if name_lower.ends_with(".exe") {
		score += 200;
	}

	// Boost for "game" in name
	if name_lower.contains("game") || name_lower.contains("client") {
		score += 50;
	}

	score
}
