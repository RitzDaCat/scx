use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result};
use log::{info, warn};
use nix::unistd::{Uid, Gid, setuid, setgid, setgroups, setresuid, setresgid, getresuid, getresgid};
use zbus::blocking::{Connection, ProxyBuilder};
use zbus::zvariant::OwnedValue;

const DBUS_DEST: &str = "org.kde.KWin";
const KWIN_PATH: &str = "/KWin";
const KWIN_IFACE: &str = "org.kde.KWin";

#[derive(Debug, Clone, Default)]
pub struct ActiveWindowInfo {
    pub caption: Option<String>,
    pub desktop_file: Option<String>,
    pub fullscreen: bool,
    pub app_class: Option<String>,
    pub uuid: Option<String>,
}

#[derive(Debug)]
pub struct KWinState {
    state: Arc<RwLock<ActiveWindowInfo>>,
    shutdown: Arc<AtomicBool>,
    _thread: Option<JoinHandle<()>>,
}

impl KWinState {
    pub fn new() -> Result<Self> {
        let state = Arc::new(RwLock::new(ActiveWindowInfo::default()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_state = Arc::clone(&state);
        let thread_shutdown = Arc::clone(&shutdown);

        let original_uid = std::env::var("SUDO_UID")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .map(Uid::from_raw);

        let original_gid = std::env::var("SUDO_GID")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .map(Gid::from_raw);

        let handle = thread::Builder::new()
            .name("kwin-focus".to_string())
            .spawn(move || watcher_loop(thread_state, thread_shutdown, original_uid, original_gid))
            .context("failed to spawn KWin watcher")?;

        Ok(Self {
            state,
            shutdown,
            _thread: Some(handle),
        })
    }

    pub fn snapshot(&self) -> ActiveWindowInfo {
        // RwLock allows multiple concurrent readers without blocking
        self.state.read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| ActiveWindowInfo::default())
    }
}

impl Drop for KWinState {
    fn drop(&mut self) {
        // Signal thread to shutdown
        self.shutdown.store(true, Ordering::Relaxed);
        // Give thread 500ms to exit gracefully, then detach
        if let Some(handle) = self._thread.take() {
            // Non-blocking join with timeout emulation
            for _ in 0..5 {
                if handle.is_finished() {
                    let _ = handle.join();
                    return;
                }
                thread::sleep(Duration::from_millis(100));
            }
            // Thread didn't finish, let it detach (will terminate when main exits)
            warn!("KWin watcher thread didn't shutdown cleanly");
        }
    }
}

fn watcher_loop(shared: Arc<RwLock<ActiveWindowInfo>>, shutdown: Arc<AtomicBool>, original_uid: Option<Uid>, original_gid: Option<Gid>) {
    // Check shutdown flag early to prevent cleanup delays
    if shutdown.load(Ordering::Relaxed) {
        return;
    }

    if let (Some(gid), Some(uid)) = (original_gid, original_uid) {
        // Permanently drop privileges in correct order: supplementary groups, gid, then uid
        // Clear supplementary groups first to prevent privilege escalation
        let empty_groups: Vec<Gid> = vec![];
        if let Err(e) = setgroups(&empty_groups) {
            warn!("kwin watcher: failed to clear supplementary groups: {}", e);
            shutdown.store(true, Ordering::Relaxed);
            return;
        }
        // Use setresgid/setresuid to permanently drop privileges (real, effective, AND saved)
        if let Err(e) = setresgid(gid, gid, gid) {
            warn!("kwin watcher: failed to permanently setgid to {}: {}", gid, e);
            shutdown.store(true, Ordering::Relaxed);
            return;
        }
        if let Err(e) = setresuid(uid, uid, uid) {
            warn!("kwin watcher: failed to permanently setuid to {}: {}", uid, e);
            shutdown.store(true, Ordering::Relaxed);
            return;
        }

        // Verify ALL privilege levels were dropped (real, effective, AND saved)
        // CRITICAL: If verification fails, this is a security issue - abort immediately
        let (ruid, euid, suid) = match getresuid() {
            Ok(uids) => uids,
            Err(e) => {
                eprintln!("CRITICAL SECURITY ERROR: Failed to verify UID drop: {}", e);
                std::process::abort();
            }
        };
        let (rgid, egid, sgid) = match getresgid() {
            Ok(gids) => gids,
            Err(e) => {
                eprintln!("CRITICAL SECURITY ERROR: Failed to verify GID drop: {}", e);
                std::process::abort();
            }
        };

        // Check that ALL three UIDs and GIDs are non-root
        if ruid.is_root() || euid.is_root() || suid.is_root() ||
           rgid.is_root() || egid.is_root() || sgid.is_root() {
            eprintln!("CRITICAL SECURITY ERROR: Failed to fully drop root privileges in KWin watcher");
            eprintln!("  Real UID: {}, Effective UID: {}, Saved UID: {}", ruid, euid, suid);
            eprintln!("  Real GID: {}, Effective GID: {}, Saved GID: {}", rgid, egid, sgid);
            eprintln!("Aborting to prevent privilege escalation vulnerability");
            std::process::abort();
        }

        info!("kwin watcher: running as uid={}, gid={} (privileges permanently dropped)", uid, gid);
    } else {
        info!("kwin watcher: running as current user (not started with sudo)");
    }

    let mut backoff = Duration::from_millis(1000);
    let mut conn: Option<Connection> = None;
    let mut retry_count = 0;
    const MAX_RETRIES: u32 = 10;

    while !shutdown.load(Ordering::Relaxed) {
        if conn.is_none() {
            if retry_count >= MAX_RETRIES {
                warn!("kwin watcher: max retries ({}) exceeded, stopping", MAX_RETRIES);
                break;
            }
            match Connection::session() {
                Ok(c) => {
                    conn = Some(c);
                    backoff = Duration::from_millis(1000);
                    retry_count = 0;
                }
                Err(err) => {
                    warn!("kwin connection error: {err:?}");
                    retry_count += 1;
                    let sleep_ms = backoff.as_millis() as u64;
                    for _ in 0..(sleep_ms / 100) {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        thread::sleep(Duration::from_millis(100));
                    }
                    backoff = (backoff * 2).min(Duration::from_secs(5));
                    continue;
                }
            }
        }

        if let Some(ref connection) = conn {
            if let Err(err) = query_once(connection, &shared) {
                warn!("kwin watcher error: {err:?}");
                conn = None;
                retry_count += 1;
                // Sleep in small increments to respond to shutdown
                let sleep_ms = backoff.as_millis() as u64;
                for _ in 0..(sleep_ms / 100) {
                    if shutdown.load(Ordering::Relaxed) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                backoff = (backoff * 2).min(Duration::from_secs(5));
                continue;
            }
        }

        // Poll at 1Hz - window focus changes are infrequent
        thread::sleep(Duration::from_secs(1));
    }
}

fn query_once(conn: &Connection, state: &Arc<RwLock<ActiveWindowInfo>>) -> Result<()> {
    let proxy: zbus::blocking::Proxy<'_> = ProxyBuilder::new(conn)
        .destination(DBUS_DEST)?
        .path(KWIN_PATH)?
        .interface(KWIN_IFACE)?
        .cache_properties(zbus::CacheProperties::No)
        .build()?;

    let reply = proxy.call_method("queryWindowInfo", &())?;
    let dict: HashMap<String, OwnedValue> = reply.body().deserialize()?;
    let mut info = ActiveWindowInfo::default();

    info.caption = take_string(&dict, "caption");
    info.desktop_file = take_string(&dict, "desktopFile");
    info.app_class = take_string(&dict, "resourceClass");
    info.uuid = take_string(&dict, "uuid");
    info.fullscreen = take_bool(&dict, "fullscreen").unwrap_or(false);

    if let Ok(mut guard) = state.write() {
        *guard = info;
    }

    Ok(())
}

fn take_string(map: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    map.get(key).and_then(|v| {
        if let Ok(s) = <&str>::try_from(v) {
            Some(s.to_string())
        } else {
            None
        }
    })
}

fn take_bool(map: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    map.get(key).and_then(|v| {
        bool::try_from(v).ok()
    })
}


