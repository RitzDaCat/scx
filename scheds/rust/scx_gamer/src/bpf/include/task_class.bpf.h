/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Thread Classification
 * Copyright (c) 2025 RitzDaCat
 *
 * Automatic detection of GPU, compositor, network, audio, and input threads.
 * This file is AI-friendly: ~250 lines, single responsibility.
 */
#ifndef __GAMER_TASK_CLASS_BPF_H
#define __GAMER_TASK_CLASS_BPF_H

#include "config.bpf.h"

/*
 * Thread Name Pattern Matching
 * All functions check comm[] for specific thread naming patterns
 */

/* Apply baseline boost derived from role presets. Keeps maximum value so multiple
 * classifications can cooperate without losing the highest requested priority. */
static __always_inline void apply_class_boost(struct task_ctx *tctx, u8 boost)
{
	if (boost > tctx->class_boost)
		tctx->class_boost = boost;
}

/**
 * is_gpu_submit_name - Check if thread name matches GPU submission pattern
 * @comm: Thread name (comm field from task_struct)
 *
 * GPU submission threads - critical for frame presentation.
 * Examples: vkd3d-swapchain, dxvk-submit, RenderThread 0, RHIThread
 *
 * CRITICAL FOR SPLITGATE: Unreal Engine 4 uses RenderThread for GPU submission.
 * This thread must get physical cores (no SMT) to avoid frame pacing issues.
 *
 * TIER 0: Optimized character-by-character comparison
 * - Character comparisons: Tier 0 (~0.5-1ns each)
 * - Early exit on first match: Tier 0 (saves ~5-20ns for matches)
 * - Total: ~5-30ns (depending on pattern position)
 *
 * Frequency: Called during thread classification (thousands/sec during startup,
 *            then cached in task_ctx for subsequent checks)
 * Net overhead: Minimal (results cached in task_ctx->is_gpu_submit)
 */
static __always_inline bool is_gpu_submit_name(const char *comm)
{
	/* TIER 0: DXVK threads (DX9/10/11 to Vulkan translation - VERY common with Proton)
	 * Ordered by frequency: dxvk-* is most common */
	if (likely(comm[0] == 'd' && comm[1] == 'x' && comm[2] == 'v' && comm[3] == 'k' && comm[4] == '-'))
		return true;  /* dxvk-submit, dxvk-queue, dxvk-frame, dxvk-cs, dxvk-shader-* */

	/* TIER 0: Unreal Engine RHI (Render Hardware Interface) threads */
	if (comm[0] == 'R' && comm[1] == 'H' && comm[2] == 'I')
		return true;  /* RHIThread, RHISubmissionTh, RHIInterruptThr */

	/* TIER 0: Unreal Engine RenderThread (Splitgate, Fortnite, Kovaaks, etc.) - CRITICAL PATH */
	if (comm[0] == 'R' && comm[1] == 'e' && comm[2] == 'n' && comm[3] == 'd' &&
	    comm[4] == 'e' && comm[5] == 'r' && comm[6] == 'T') {
		/* Handle RenderThread, RenderThread 0, RenderThread 1, etc. */
		if (likely(comm[7] == '\0' || comm[7] == ' '))
			return true;  /* RenderThread, RenderThread 0, RenderThread 1 */
	}

	/* TIER 0: vkd3d threads (Vulkan/D3D12 translation layer for Proton) */
	if (comm[0] == 'v' && comm[1] == 'k' && comm[2] == 'd' && comm[3] == '3')
		return true;  /* vkd3d_queue, vkd3d_fence, vkd3d-swapchain */

	/* TIER 0: Bracketed Vulkan threads (WoW, etc.) */
	if (comm[0] == '[' && comm[1] == 'v' && comm[2] == 'k')
		return true;  /* [vkrt] Analysis, [vkps] Update, [vkcf] Analysis */

	/* TIER 0: Unity render threads */
	if (comm[0] == 'U' && comm[1] == 'n' && comm[2] == 'i' && comm[3] == 't' &&
	    comm[4] == 'y' && comm[5] == 'G' && comm[6] == 'f' && comm[7] == 'x')
		return true;  /* UnityGfxDevice */

	/* TIER 0: Generic "render" or "gpu" thread names */
	if (comm[0] == 'r' && comm[1] == 'e' && comm[2] == 'n' && comm[3] == 'd' &&
	    comm[4] == 'e' && comm[5] == 'r')
		return true;

	if (comm[0] == 'g' && comm[1] == 'p' && comm[2] == 'u')
		return true;

	return false;
}

/**
 * is_compositor_name - Check if thread name matches compositor pattern
 * @comm: Thread name (comm field from task_struct)
 *
 * Compositor/window manager threads.
 * Examples: kwin_wayland, mutter, weston
 *
 * TIER 0: Optimized character-by-character comparison (~5-30ns)
 */
static __always_inline bool is_compositor_name(const char *comm)
{
	/* KDE Plasma Wayland */
	if (comm[0] == 'k' && comm[1] == 'w' && comm[2] == 'i' && comm[3] == 'n')
		return true;

	/* GNOME Mutter */
	if (comm[0] == 'm' && comm[1] == 'u' && comm[2] == 't' && comm[3] == 't')
		return true;

	/* Weston reference compositor */
	if (comm[0] == 'w' && comm[1] == 'e' && comm[2] == 's' && comm[3] == 't')
		return true;

	/* Sway (i3-like) */
	if (comm[0] == 's' && comm[1] == 'w' && comm[2] == 'a' && comm[3] == 'y')
		return true;

	/* Hyprland */
	if (comm[0] == 'H' && comm[1] == 'y' && comm[2] == 'p' && comm[3] == 'r')
		return true;

	/* labwc (Openbox-like) */
	if (comm[0] == 'l' && comm[1] == 'a' && comm[2] == 'b' && comm[3] == 'w')
		return true;

	/* Xwayland server */
	if (comm[0] == 'X' && comm[1] == 'w' && comm[2] == 'a' && comm[3] == 'y')
		return true;

	/* Unreal Engine compositor threads (Kovaaks, etc.) */
	if (comm[0] == 'C' && comm[1] == 'o' && comm[2] == 'm' && comm[3] == 'p' &&
	    comm[4] == 'o' && comm[5] == 's' && comm[6] == 'i' && comm[7] == 't')
		return true;  /* CompositorTileW, CompositorThread, etc. */

	return false;
}

/*
 * Network/netcode threads - critical for online games
 * Network threads are critical path: player input -> network -> server.
 * Examples: WebSocketClient, UdpSocket, NetThread, RtcWorkerThread
 */
static __always_inline bool is_network_name(const char *comm)
{
	/* Unreal Engine network threads */
	if (comm[0] == 'W' && comm[1] == 'e' && comm[2] == 'b' && comm[3] == 'S' &&
	    comm[4] == 'o' && comm[5] == 'c' && comm[6] == 'k')
		return true;  /* WebSocketClient */

	/* LibWebSockets (voice chat WebSocket library - Vivox, etc.) */
	if (comm[0] == 'L' && comm[1] == 'i' && comm[2] == 'b' && comm[3] == 'w' &&
	    comm[4] == 'e' && comm[5] == 'b')
		return true;  /* LibwebsocketsTh */

	if (comm[0] == 'U' && comm[1] == 'd' && comm[2] == 'p' && comm[3] == 'S')
		return true;  /* UdpSocket */

	if (comm[0] == 'R' && comm[1] == 't' && comm[2] == 'c')
		return true;  /* RtcWorkerThread, RtcSignalingThr, RtcNetworkThrea */

	if (comm[0] == 'H' && comm[1] == 't' && comm[2] == 't' && comm[3] == 'p' &&
	    comm[4] == 'M' && comm[5] == 'a' && comm[6] == 'n')
		return true;  /* HttpManagerThre */

	if (comm[0] == 'I' && comm[1] == 'o' && comm[2] == 'S')
		return true;  /* IoService */

	if (comm[0] == 'I' && comm[1] == 'o' && comm[2] == 'D')
		return true;  /* IoDispatcher */

	if (comm[0] == 'I' && comm[1] == 'O' && comm[2] == 'T' && comm[3] == 'h')
		return true;  /* IOThreadPool */

	if (comm[0] == 'N' && comm[1] == 'A' && comm[2] == 'T' && comm[3] == 'S')
		return true;  /* NATSClientThrea */

	if (comm[0] == 'O' && comm[1] == 'n' && comm[2] == 'l' && comm[3] == 'i' &&
	    comm[4] == 'n' && comm[5] == 'e' && comm[6] == 'A')
		return true;  /* OnlineAsyncTask */

	/* Generic patterns: "network", "netcode", "net_", "recv", "send", "socket" */
	if (comm[0] == 'n' && comm[1] == 'e' && comm[2] == 't')
		return true;

	/* WoW uppercase network threads */
	if (comm[0] == 'N' && comm[1] == 'e' && comm[2] == 't')
		return true;  /* NetThread, Net Queue, Network */

	/* Warframe network threads - Wine networking */
	if (comm[0] == 'w' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'e' &&
	    comm[4] == '_' && comm[5] == 'r' && comm[6] == 'p' && comm[7] == 'c')
		return true;  /* wine_rpcrt4_ser */

	/* Generic Wine networking patterns */
	if (comm[0] == 'w' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'e' &&
	    comm[4] == '_' && comm[5] == 'n' && comm[6] == 'e' && comm[7] == 't')
		return true;  /* wine_net* threads */

	/* Warframe main game threads - generic game executable patterns */
	if (comm[0] == 'W' && comm[1] == 'a' && comm[2] == 'r' && comm[3] == 'f' &&
	    comm[4] == 'r' && comm[5] == 'a' && comm[6] == 'm' && comm[7] == 'e' &&
	    comm[8] == '.' && comm[9] == 'x' && comm[10] == '6' && comm[11] == '4')
		return true;  /* Warframe.x64.ex */

	/* Wine game threads - common Wine thread patterns */
	if (comm[0] == 'w' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'e' &&
	    comm[4] == '_' && comm[5] == 't' && comm[6] == 'h' && comm[7] == 'r')
		return true;  /* wine_threadpool */

	if (comm[0] == 'w' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'e' &&
	    comm[4] == '_' && comm[5] == 'x' && comm[6] == 'i' && comm[7] == 'n')
		return true;  /* wine_xinput_hid */

	if (comm[0] == 'w' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'e' &&
	    comm[4] == '_' && comm[5] == 's' && comm[6] == 'e' && comm[7] == 'c')
		return true;  /* wine_sechost_de */

	if (comm[0] == 'w' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'e' &&
	    comm[4] == '_' && comm[5] == 'm' && comm[6] == 'm' && comm[7] == 'd')
		return true;  /* wine_mmdevapi_n */

	if (comm[0] == 'w' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'e' &&
	    comm[4] == '_' && comm[5] == 'd' && comm[6] == 'i' && comm[7] == 'n')
		return true;  /* wine_dinput_wor */


	if (comm[0] == 'r' && comm[1] == 'e' && comm[2] == 'c' && comm[3] == 'v')
		return true;

	if (comm[0] == 's' && comm[1] == 'e' && comm[2] == 'n' && comm[3] == 'd')
		return true;

	if (comm[0] == 's' && comm[1] == 'o' && comm[2] == 'c' && comm[3] == 'k')
		return true;

	if (comm[0] == 'i' && comm[1] == 'o' && comm[2] == '_')
		return true;

	if (comm[0] == 'p' && comm[1] == 'a' && comm[2] == 'c' && comm[3] == 'k')
		return true;

	return false;
}

/*
 * Gaming-specific network thread detection
 * Gaming network threads have specific patterns and require ultra-low latency
 * Examples: Client, Server, Netcode, Multiplayer, GameClient, GameServer
 */
static __always_inline bool is_gaming_network_thread(const char *comm)
{
	/* Game client/server threads */
	if (comm[0] == 'C' && comm[1] == 'l' && comm[2] == 'i' && comm[3] == 'e')
		return true;  /* Client */

	if (comm[0] == 'S' && comm[1] == 'e' && comm[2] == 'r' && comm[3] == 'v')
		return true;  /* Server */

	if (comm[0] == 'G' && comm[1] == 'a' && comm[2] == 'm' && comm[3] == 'e') {
		if (comm[4] == 'C' && comm[5] == 'l' && comm[6] == 'i' && comm[7] == 'e')
			return true;  /* GameClient */
		if (comm[4] == 'S' && comm[5] == 'e' && comm[6] == 'r' && comm[7] == 'v')
			return true;  /* GameServer */
	}

	/* Multiplayer/netcode threads */
	if (comm[0] == 'M' && comm[1] == 'u' && comm[2] == 'l' && comm[3] == 't')
		return true;  /* Multiplayer */

	if (comm[0] == 'N' && comm[1] == 'e' && comm[2] == 't' && comm[3] == 'c')
		return true;  /* Netcode */

	/* Real-time communication (voice chat, etc.) */
	if (comm[0] == 'V' && comm[1] == 'o' && comm[2] == 'i' && comm[3] == 'c')
		return true;  /* Voice */

	if (comm[0] == 'C' && comm[1] == 'h' && comm[2] == 'a' && comm[3] == 't')
		return true;  /* Chat */

	return false;
}

/*
 * System audio threads (PipeWire/PulseAudio/ALSA/JACK)
 * System audio has strict latency requirements but shouldn't block game input.
 * Examples: pipewire, pw-*, pulseaudio, jackdbus
 */
static __always_inline bool is_system_audio_name(const char *comm)
{
	/* PipeWire audio server (modern Linux standard)
	 * Thread names: pipewire, pipewire-pulse, module-rt, data-loop.0, etc.
	 * Check for "pipewire" prefix (first 8 chars = "pipewire") */
	if (comm[0] == 'p' && comm[1] == 'i' && comm[2] == 'p' && comm[3] == 'e' &&
	    comm[4] == 'w' && comm[5] == 'i' && comm[6] == 'r' && comm[7] == 'e')
		return true;  /* pipewire */

	/* PipeWire module threads (module-rt, module-protocol-pulse, etc.) */
	if (comm[0] == 'm' && comm[1] == 'o' && comm[2] == 'd' && comm[3] == 'u' &&
	    comm[4] == 'l' && comm[5] == 'e' && comm[6] == '-')
		return true;  /* module-* */

	/* PipeWire data loop threads (data-loop.0, data-loop.1, etc.) */
	if (comm[0] == 'd' && comm[1] == 'a' && comm[2] == 't' && comm[3] == 'a' &&
	    comm[4] == '-' && comm[5] == 'l' && comm[6] == 'o' && comm[7] == 'o')
		return true;  /* data-loop.* */

	/* Check for "pw-" prefix (pipewire worker threads) */
	if (comm[0] == 'p' && comm[1] == 'w' && comm[2] == '-')
		return true;  /* pw-* threads */

	/* ALSA (Advanced Linux Sound Architecture) */
	if (comm[0] == 'a' && comm[1] == 'l' && comm[2] == 's' && comm[3] == 'a')
		return true;

	/* JACK audio connection kit (pro audio) */
	if (comm[0] == 'j' && comm[1] == 'a' && comm[2] == 'c' && comm[3] == 'k')
		return true;

	/* PulseAudio (legacy, but still common) */
	if (comm[0] == 'p' && comm[1] == 'u' && comm[2] == 'l' && comm[3] == 's')
		return true;

	return false;
}

/*
 * USB audio interface threads (GoXLR, Focusrite, etc.)
 * USB audio interfaces have strict latency requirements for real-time audio.
 * Examples: snd-usb-audio, snd-usb-caiaq, snd-usb-hiface
 */
static __always_inline bool is_usb_audio_interface(const char *comm)
{
	/* USB audio interface patterns */
	if (comm[0] == 's' && comm[1] == 'n' && comm[2] == 'd' && comm[3] == '_') {
		/* snd-usb-audio, snd-usb-caiaq, snd-usb-hiface, etc. */
		return true;
	}

	/* GoXLR specific patterns */
	if (comm[0] == 'g' && comm[1] == 'o' && comm[2] == 'x' && comm[3] == 'l')
		return true;  /* goxlr */

	/* Focusrite USB audio */
	if (comm[0] == 'f' && comm[1] == 'o' && comm[2] == 'c' && comm[3] == 'u')
		return true;  /* focusrite */

	return false;
}

/*
 * GoXLR mixer-specific thread detection
 * GoXLR mixer threads have specific naming patterns and require ultra-low latency
 * Examples: GoXLR Mixer, GoXLR Audio, GoXLR Control, goxlr-mixer
 */
static __always_inline bool is_goxlr_mixer_thread(const char *comm)
{
	/* GoXLR mixer thread patterns */
	if (comm[0] == 'G' && comm[1] == 'o' && comm[2] == 'X' && comm[3] == 'L' && comm[4] == 'R')
		return true;  /* GoXLR Mixer, GoXLR Audio, etc. */

	/* GoXLR daemon processes */
	if (comm[0] == 'g' && comm[1] == 'o' && comm[2] == 'x' && comm[3] == 'l' && comm[4] == 'r' && comm[5] == '-')
		return true;  /* goxlr-mixer, goxlr-daemon, etc. */

	return false;
}

/*
 * Detect audio buffer size from thread wakeup patterns
 * Audio threads wake up at sample_rate / buffer_size frequency
 * Examples: 48kHz/64 samples = 750Hz, 48kHz/128 samples = 375Hz
 */
static __always_inline u32 detect_audio_buffer_size(u64 wakeup_freq, u32 sample_rate)
{
	if (sample_rate == 0 || wakeup_freq == 0)
		return 0;
	
	u32 calculated_buffer = sample_rate / wakeup_freq;
	
	/* Round to common audio buffer sizes */
	if (calculated_buffer <= 32) return 32;
	if (calculated_buffer <= 64) return 64;
	if (calculated_buffer <= 128) return 128;
	if (calculated_buffer <= 256) return 256;
	if (calculated_buffer <= 512) return 512;
	if (calculated_buffer <= 1024) return 1024;
	if (calculated_buffer <= 2048) return 2048;
	
	return calculated_buffer;  /* Return calculated value if not standard size */
}

/*
 * Detect audio sample rate from thread patterns
 * Audio threads wake up at sample_rate / buffer_size frequency
 */
static __always_inline u32 detect_audio_sample_rate(u64 wakeup_freq, u32 buffer_size)
{
	if (buffer_size == 0 || wakeup_freq == 0)
		return 44100;  /* Default to 44.1kHz */
	
	u32 calculated_rate = wakeup_freq * buffer_size;
	
	/* Round to common audio sample rates */
	if (calculated_rate >= 44000 && calculated_rate <= 45000) return 44100;
	if (calculated_rate >= 47000 && calculated_rate <= 49000) return 48000;
	if (calculated_rate >= 95000 && calculated_rate <= 97000) return 96000;
	if (calculated_rate >= 175000 && calculated_rate <= 185000) return 176400;
	if (calculated_rate >= 190000 && calculated_rate <= 200000) return 192000;
	
	return calculated_rate;  /* Return calculated value if not standard rate */
}

/*
 * Calculate dynamic audio boost based on buffer size and sample rate
 * Smaller buffers and higher sample rates get higher boost
 */
static __always_inline u8 calculate_audio_boost(u8 base_boost, u32 buffer_size, u32 sample_rate)
{
	u8 boost = base_boost;

	u64 buffer_ns = 0;
	if (sample_rate > 0 && buffer_size > 0)
		buffer_ns = ((u64)buffer_size * 1000000000ULL) / sample_rate;
	
	if (buffer_ns > 0) {
		/* Smaller playback buffers require aggressive boosting. */
		if (buffer_ns <= 2000000ULL) boost += 3;       /* ≤2ms */
		else if (buffer_ns <= 4000000ULL) boost += 2;  /* ≤4ms */
		else if (buffer_ns <= 8000000ULL) boost += 1;  /* ≤8ms */
	} else {
		/* Fallback for unknown sample rate: use raw buffer size heuristics. */
		if (buffer_size <= 32) boost += 3;
		else if (buffer_size <= 64) boost += 2;
		else if (buffer_size <= 128) boost += 1;
	}
	
	/* Higher boost for higher sample rates */
	if (sample_rate >= 192000) boost += 2;  /* High-res audio */
	else if (sample_rate >= 96000) boost += 1; /* High-res audio */
	
	return MIN(boost, 10);  /* Cap at 10x boost */
}

/*
 * Calculate GoXLR-specific boost based on mixer complexity and audio settings
 * GoXLR mixers require ultra-low latency for real-time audio processing
 */
static __always_inline u8 calculate_goxlr_boost(u32 mixer_channels, u32 sample_rate, u32 buffer_size)
{
	u8 boost = 6; /* Base USB audio boost for GoXLR */
	
	/* Higher boost for more mixer channels (more CPU intensive) */
	if (mixer_channels >= 8) boost += 2;      /* Complex mixer (8+ channels) */
	else if (mixer_channels >= 4) boost += 1; /* Standard mixer (4-7 channels) */
	
	/* Ultra-low latency mode for gaming (48kHz-96kHz) */
	if (sample_rate >= 48000 && sample_rate <= 96000) boost += 1;
	
	/* Maximum boost for smallest buffers (ultra-low latency) */
	if (buffer_size <= 32) boost += 2;      /* Ultra-low latency */
	else if (buffer_size <= 64) boost += 1; /* Low latency */
	
	return MIN(boost, 10);  /* Cap at 10x boost */
}

/*
 * Detect NVMe-specific I/O patterns
 * NVMe threads have high page fault rates and specific I/O wait patterns
 */
static __always_inline bool is_nvme_io_thread(const struct task_struct *p, struct task_ctx *tctx)
{
	/* High page fault rate indicates asset loading */
	if (tctx->pgfault_rate <= 100)
		return false;
	
	/* Check for I/O wait patterns (voluntary context switches) */
	u64 voluntary_switches = BPF_CORE_READ(p, nvcsw);
	u64 involuntary_switches = BPF_CORE_READ(p, nivcsw);
	
	if (voluntary_switches == 0)
		return false;
	
	/* Calculate I/O wait ratio */
	u64 total_switches = voluntary_switches + involuntary_switches;
	u64 io_wait_ratio = (voluntary_switches * 100) / total_switches;
	
	/* NVMe I/O threads typically have >30% voluntary switches (I/O wait) */
	return io_wait_ratio > 30;
}

/*
 * Detect NVMe hot path threads for sequential asset streaming
 * Hot path threads have higher page fault rates and sequential I/O patterns
 * These benefit from maximum boost and longer slices for optimal throughput
 */
static __always_inline bool is_nvme_hot_path_thread(const struct task_struct *p, struct task_ctx *tctx)
{
	/* Higher page fault threshold for hot path detection */
	if (tctx->pgfault_rate <= 200)
		return false;
	
	/* Check for sequential I/O patterns (asset streaming) */
	u64 read_bytes = BPF_CORE_READ(p, ioac.read_bytes);
	u64 read_chars = BPF_CORE_READ(p, ioac.rchar);
	
	/* Sequential I/O: large read_bytes vs small read_chars ratio */
	if (read_bytes > 0 && read_chars > 0) {
		u64 sequential_ratio = read_bytes / read_chars;
		/* Sequential I/O typically has ratio > 100 (large contiguous reads) */
		if (sequential_ratio > 100) {
			return true;
		}
	}
	
	/* High I/O wait ratio indicates storage-intensive operations */
	u64 voluntary_switches = BPF_CORE_READ(p, nvcsw);
	u64 involuntary_switches = BPF_CORE_READ(p, nivcsw);
	
	if (voluntary_switches > 0) {
		u64 total_switches = voluntary_switches + involuntary_switches;
		u64 io_wait_ratio = (voluntary_switches * 100) / total_switches;
		
		/* Hot path threads have >50% I/O wait (higher than regular NVMe) */
		if (io_wait_ratio > 50) {
			return true;
		}
	}
	
	return false;
}

/*
 * Game audio threads - ELEVATED PRIORITY for competitive gaming
 * Game audio (footsteps, gunshots, voice chat) is critical for gameplay awareness.
 * Latency here directly affects reaction time to audio cues.
 * 
 * Supported engines: FMOD, Wwise, Vivox, OpenAL, FAudio, Miles, Criware
 * Examples: AudioThread, FMODThread, AkAudioThread, VivoxVoice
 */
static __always_inline bool is_game_audio_name(const char *comm)
{
	/* FMOD Studio (Arc Raiders, Fortnite, many indie games)
	 * Thread patterns: FMOD*, fmod*, FMODStream, FMODMixer */
	if (comm[0] == 'F' && comm[1] == 'M' && comm[2] == 'O' && comm[3] == 'D')
		return true;  /* FMOD* (uppercase) */
	if (comm[0] == 'f' && comm[1] == 'm' && comm[2] == 'o' && comm[3] == 'd')
		return true;  /* fmod* (lowercase) */

	/* Wwise (Destiny 2, Valorant, many AAA games)
	 * Thread patterns: AkAudio*, Wwise*, AK::* */
	if (comm[0] == 'A' && comm[1] == 'k' && comm[2] == 'A' && comm[3] == 'u')
		return true;  /* AkAudio* (Wwise thread prefix) */
	if (comm[0] == 'W' && comm[1] == 'w' && comm[2] == 'i' && comm[3] == 's' && comm[4] == 'e')
		return true;  /* Wwise* */
	if (comm[0] == 'w' && comm[1] == 'w' && comm[2] == 'i' && comm[3] == 's' && comm[4] == 'e')
		return true;  /* wwise* (lowercase) */

	/* Vivox (voice middleware - Fortnite, PUBG, many multiplayer games)
	 * Thread patterns: vivox*, Vivox*, vx_* */
	if (comm[0] == 'V' && comm[1] == 'i' && comm[2] == 'v' && comm[3] == 'o' && comm[4] == 'x')
		return true;  /* Vivox* */
	if (comm[0] == 'v' && comm[1] == 'i' && comm[2] == 'v' && comm[3] == 'o' && comm[4] == 'x')
		return true;  /* vivox* */
	if (comm[0] == 'v' && comm[1] == 'x' && comm[2] == '_')
		return true;  /* vx_* (Vivox internal threads) */

	/* Unreal Engine audio threads */
	if (comm[0] == 'A' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'i' && comm[4] == 'o') {
		/* Handle AudioThread, AudioThread0, AudioMixerRende, AudioDevice, etc. */
		if (comm[5] == '\0' || comm[5] == 'T' || comm[5] == 'M' || comm[5] == 'D')
			return true;  /* AudioThread, AudioMixerRende, AudioDevice */
	}

	/* FAudio (Wine/Proton XAudio2 implementation) - THE key audio layer for Proton */
	if (comm[0] == 'F' && comm[1] == 'A' && comm[2] == 'u' && comm[3] == 'd')
		return true;  /* FAudio* */

	/* Wine/Proton audio threads - CRITICAL for Linux gaming
	 * Wine translates Windows audio APIs, these are the actual threads doing the work.
	 * Even if FMOD/Wwise runs inside Wine, audio flows through these threads. */
	if (comm[0] == 'w' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'e') {
		/* winepulse - Wine PulseAudio/PipeWire bridge */
		if (comm[4] == 'p' && comm[5] == 'u' && comm[6] == 'l' && comm[7] == 's')
			return true;  /* winepulse */
		/* wineaudio - Generic Wine audio thread */
		if (comm[4] == 'a' && comm[5] == 'u' && comm[6] == 'd' && comm[7] == 'i')
			return true;  /* wineaudio */
		/* wine-audio - Alternate naming */
		if (comm[4] == '-' && comm[5] == 'a' && comm[6] == 'u' && comm[7] == 'd')
			return true;  /* wine-audio */
	}

	/* mmdevapi - Windows Multimedia Device API (used by FMOD/Wwise in Proton) */
	if (comm[0] == 'm' && comm[1] == 'm' && comm[2] == 'd' && comm[3] == 'e' && comm[4] == 'v')
		return true;  /* mmdevapi */

	/* xaudio2 - DirectX audio threads in Proton */
	if (comm[0] == 'x' && comm[1] == 'a' && comm[2] == 'u' && comm[3] == 'd' && comm[4] == 'i')
		return true;  /* xaudio2_* */

	/* dsound - DirectSound threads in Proton (older games) */
	if (comm[0] == 'd' && comm[1] == 's' && comm[2] == 'o' && comm[3] == 'u' && comm[4] == 'n')
		return true;  /* dsound* */

	/* Miles Sound System (older games, Source engine)
	 * Thread patterns: Miles*, MSS* */
	if (comm[0] == 'M' && comm[1] == 'i' && comm[2] == 'l' && comm[3] == 'e' && comm[4] == 's')
		return true;  /* Miles* */
	if (comm[0] == 'M' && comm[1] == 'S' && comm[2] == 'S')
		return true;  /* MSS* */

	/* Criware ADX (Japanese games, many JRPGs)
	 * Thread patterns: CriAtom*, criatomex*, Adx* */
	if (comm[0] == 'C' && comm[1] == 'r' && comm[2] == 'i' && comm[3] == 'A')
		return true;  /* CriAtom* */
	if (comm[0] == 'c' && comm[1] == 'r' && comm[2] == 'i' && comm[3] == 'a')
		return true;  /* criatomex* */
	if (comm[0] == 'A' && comm[1] == 'd' && comm[2] == 'x')
		return true;  /* Adx* */

	/* Bink audio (common video codec in games) */
	if (comm[0] == 'B' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'k')
		return true;  /* Bink* (video/audio codec) */

	/* OpenAL (common game audio library) */
	if (comm[0] == 'O' && comm[1] == 'p' && comm[2] == 'e' && comm[3] == 'n' && comm[4] == 'A' && comm[5] == 'L')
		return true;  /* OpenAL* */
	if (comm[0] == 'o' && comm[1] == 'p' && comm[2] == 'e' && comm[3] == 'n' && comm[4] == 'a' && comm[5] == 'l')
		return true;  /* openal* */

	/* XAudio2 (Windows games via Wine/Proton)
	 * Thread patterns: XAudio*, xaudio* */
	if (comm[0] == 'X' && comm[1] == 'A' && comm[2] == 'u' && comm[3] == 'd')
		return true;  /* XAudio* */
	if (comm[0] == 'x' && comm[1] == 'a' && comm[2] == 'u' && comm[3] == 'd')
		return true;  /* xaudio* */

	/* Generic game audio threads: "audio", "sound", "snd_" */
	if (comm[0] == 'a' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'i' && comm[4] == 'o')
		return true;  /* audio* */
	if (comm[0] == 's' && comm[1] == 'o' && comm[2] == 'u' && comm[3] == 'n' && comm[4] == 'd')
		return true;  /* sound* */
	if (comm[0] == 's' && comm[1] == 'n' && comm[2] == 'd' && comm[3] == '_')
		return true;  /* snd_* */

	/* Voice chat threads (in-game voice) */
	if (comm[0] == 'V' && comm[1] == 'o' && comm[2] == 'i' && comm[3] == 'c' && comm[4] == 'e')
		return true;  /* Voice* (VoiceChat, VoiceThread) */
	if (comm[0] == 'v' && comm[1] == 'o' && comm[2] == 'i' && comm[3] == 'c' && comm[4] == 'e')
		return true;  /* voice* */
	if (comm[0] == 'V' && comm[1] == 'O' && comm[2] == 'I' && comm[3] == 'P')
		return true;  /* VOIP* */

	return false;
}

/**
 * is_input_handler_name - Check if thread name matches input handler pattern
 * @comm: Thread name (comm field from task_struct)
 *
 * Input handler threads - HIGHEST priority for gaming.
 * Mouse/keyboard lag is THE WORST experience for gamers.
 * Examples: GameThread (Unreal), InputThread, SDL, EventHandler
 *
 * CRITICAL FOR SPLITGATE: UE4 processes input on GameThread, not a separate thread!
 * At 480Hz (2083µs/frame), input must reach GameThread in <500µs for responsive aim.
 *
 * TIER 0: Optimized character-by-character comparison (~5-30ns)
 * Ordered by frequency: GameThread is most common
 */
static __always_inline bool is_input_handler_name(const char *comm)
{
	/* LAYER 1: Engine-specific patterns (highest confidence) */
	
	/* TIER 0: Unreal Engine GameThread (handles input + game logic) - HIGHEST PRIORITY
	 * Ordered first for optimal branch prediction (most common pattern) */
	if (likely(comm[0] == 'G' && comm[1] == 'a' && comm[2] == 'm' && comm[3] == 'e' &&
	    comm[4] == 'T' && comm[5] == 'h' && comm[6] == 'r'))
		return true;  /* GameThread - gets 10× boost during input window */

	/* Unity Main Thread (common pattern) */
	if (comm[0] == 'M' && comm[1] == 'a' && comm[2] == 'i' && comm[3] == 'n' &&
	    comm[4] == 'T' && comm[5] == 'h' && comm[6] == 'r')
		return true;  /* MainThread */

	/* Generic "Main" thread (many engines use this) */
	if (comm[0] == 'M' && comm[1] == 'a' && comm[2] == 'i' && comm[3] == 'n')
		return true;  /* Main, MainThread, MainLoop, etc. */

	/* LAYER 2: Input library patterns (high confidence) */
	
	/* SDL input threads (very common in games) */
	if (comm[0] == 'S' && comm[1] == 'D' && comm[2] == 'L')
		return true;

	/* GLFW input (common game library) */
	if (comm[0] == 'g' && comm[1] == 'l' && comm[2] == 'f' && comm[3] == 'w')
		return true;

	/* Input/event processing threads */
	if (comm[0] == 'i' && comm[1] == 'n' && comm[2] == 'p' && comm[3] == 'u' && comm[4] == 't')
		return true;

	if (comm[0] == 'e' && comm[1] == 'v' && comm[2] == 'e' && comm[3] == 'n' && comm[4] == 't')
		return true;

	/* Qt/GTK input threads (less common in games but possible) */
	if (comm[0] == 'Q' && comm[1] == 't' && comm[2] == 'I' && comm[3] == 'n')
		return true;

	/* LAYER 3: Wine/Proton input handling (critical for Linux gaming) */
	
	/* Wine XInput controller handling (critical for gamepad input latency) */
	if (comm[0] == 'w' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'e' && comm[4] == '_' &&
	    comm[5] == 'x' && comm[6] == 'i' && comm[7] == 'n')
		return true;  /* wine_xinput_hid */

	/* Wine Windows Gaming Input (WGI) worker threads - critical for Sea of Thieves input */
	if (comm[0] == 'w' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'e' && comm[4] == '_' &&
	    comm[5] == 'w' && comm[6] == 'g' && comm[7] == 'i')
		return true;  /* wine_wginput_worker, wine_wginput_wo, etc. */

	/* Wine Raw Input dispatcher */
	if (comm[0] == 'w' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'e' && comm[4] == '_' &&
	    comm[5] == 'd' && comm[6] == 'i' && comm[7] == 'n')
		return true;  /* wine_dinput_worker */

	if (comm[0] == 'w' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'e' && comm[4] == '_' &&
	    comm[5] == 'r' && comm[6] == 'a' && comm[7] == 'w')
		return true;  /* wine_rawinput_* */

#if CONFIG_GAMER_ENABLE_LEGACY_CLASSIFY
	/* LAYER 4: Generic game logic patterns (medium confidence - common but less specific)
	 * Build-time gated so we can strip broad name matching when precise telemetry is available. */

	/* "Game" prefix (many engines use GameThread, GameLoop, GameLogic, etc.) */
	if (comm[0] == 'G' && comm[1] == 'a' && comm[2] == 'm' && comm[3] == 'e')
		return true;  /* GameThread, GameLoop, GameLogic, GameUpdate, etc. */

	/* "Logic" thread (game logic often handles input) */
	if (comm[0] == 'L' && comm[1] == 'o' && comm[2] == 'g' && comm[3] == 'i' && comm[4] == 'c')
		return true;  /* LogicThread, Logic */

	/* "Update" thread (update loop often processes input) */
	if (comm[0] == 'U' && comm[1] == 'p' && comm[2] == 'd' && comm[3] == 'a' && comm[4] == 't' && comm[5] == 'e')
		return true;  /* UpdateThread, Update */

	/* "Tick" thread (tick loop processes input) */
	if (comm[0] == 'T' && comm[1] == 'i' && comm[2] == 'c' && comm[3] == 'k')
		return true;  /* TickThread, Tick */
#endif

	return false;
}

#if CONFIG_GAMER_ENABLE_LEGACY_CLASSIFY
/**
 * is_taskgraph_thread - Check if thread name matches TaskGraph worker pattern
 * @comm: Thread name (comm field from task_struct)
 *
 * Unreal Engine 5.6 DirectX12 TaskGraph workers - parallel command list generation.
 * Examples: TaskGraphThreadHP (High Priority), TaskGraphThread (Normal), TaskGraphThreadBP (Background)
 *
 * These threads should be corralled to dedicated cores (E-cores or separate CCD) to prevent
 * cache pollution on P-cores used by GameThread/RenderThread/RHIThread.
 *
 * TIER 0: Optimized character-by-character comparison (~5-30ns)
 * Ordered by frequency: TaskGraphThread is most common (no suffix)
 */
static __always_inline bool is_taskgraph_thread(const char *comm)
{
	/* TIER 0: Unreal Engine TaskGraph workers - all variants share "TaskGraphThread" prefix
	 * TaskGraphThreadHP (High Priority), TaskGraphThread (Normal), TaskGraphThreadBP (Background) */
	if (comm[0] == 'T' && comm[1] == 'a' && comm[2] == 's' && comm[3] == 'k' &&
	    comm[4] == 'G' && comm[5] == 'r' && comm[6] == 'a' && comm[7] == 'p' &&
	    comm[8] == 'h' && comm[9] == 'T' && comm[10] == 'h' && comm[11] == 'r') {
		/* Match TaskGraphThread, TaskGraphThreadHP, TaskGraphThreadBP, etc. */
		/* Check if we've reached end of string or space/number suffix */
		if (likely(comm[12] == '\0' || comm[12] == ' ' ||
		           (comm[12] >= '0' && comm[12] <= '9') ||
		           (comm[12] == 'H' && comm[13] == 'P') ||
		           (comm[12] == 'B' && comm[13] == 'P')))
			return true;  /* TaskGraphThread, TaskGraphThreadHP, TaskGraphThreadBP, TaskGraphThread0, etc. */
	}

	return false;
}
#else
static __always_inline bool is_taskgraph_thread(const char *comm)
{
	/* LEGACY DISABLED: Build stripped of comm-based TaskGraph detection. */
	(void)comm;
	return false;
}
#endif

static __always_inline bool comm_contains(const char *comm, const char *needle, int needle_len)
{
	for (int i = 0; i <= TASK_COMM_LEN - needle_len; i++) {
		int j = 0;
		for (; j < needle_len; j++) {
			if (comm[i + j] != needle[j])
				break;
		}
		if (j == needle_len)
			return true;
	}
	return false;
}

/**
 * is_wine_input_thread - Check if thread is a Wine/Proton input handler
 * @comm: Thread name (comm field from task_struct)
 *
 * Wine input threads handle XInput, DirectInput, RawInput, and WGI (Windows Gaming Input).
 * These are critical for gamepad and keyboard/mouse input in Proton games.
 *
 * TIER 0: Optimized character-by-character comparison (~5-20ns)
 */
static __always_inline bool is_wine_input_thread(const char *comm)
{
	/* Check for "wine_" prefix first */
	if (comm[0] != 'w' || comm[1] != 'i' || comm[2] != 'n' || comm[3] != 'e' || comm[4] != '_')
		return false;
	
	/* wine_xinput_hid - XInput controller handling */
	if (comm[5] == 'x' && comm[6] == 'i' && comm[7] == 'n')
		return true;
	
	/* wine_wginput_worker - Windows Gaming Input (WGI) */
	if (comm[5] == 'w' && comm[6] == 'g' && comm[7] == 'i')
		return true;
	
	/* wine_dinput_worker - DirectInput */
	if (comm[5] == 'd' && comm[6] == 'i' && comm[7] == 'n')
		return true;
	
	/* wine_rawinput_* - Raw Input */
	if (comm[5] == 'r' && comm[6] == 'a' && comm[7] == 'w')
		return true;
	
	return false;
}

/**
 * is_sdl_event_thread - Check if thread is an SDL event loop thread
 * @comm: Thread name (comm field from task_struct)
 *
 * SDL event threads handle input events in SDL-based games.
 * Examples: SDLTimer, SDLVideoResize, SDL Main Thread
 *
 * TIER 0: Optimized character-by-character comparison (~5-15ns)
 */
static __always_inline bool is_sdl_event_thread(const char *comm)
{
	/* SDL* prefix */
	if (comm[0] == 'S' && comm[1] == 'D' && comm[2] == 'L')
		return true;
	
	return false;
}

/**
 * classify_input_handler - Classify thread as input handler
 * @p: Task struct pointer
 * @tctx: Task context to update
 *
 * TIER 0/1: Optimized for thread classification hot path
 * - Name check: Tier 0 (~5-30ns, is_input_handler_name)
 * - Struct field writes: Tier 0 (~1-2ns each)
 * - String contains check: Tier 0/1 (~5-20ns, comm_contains)
 * - Total: ~11-52ns (depending on pattern match)
 */
static __always_inline void classify_input_handler(struct task_struct *p, struct task_ctx *tctx)
{
	if (likely(is_input_handler_name(p->comm))) {
		tctx->is_input_handler = 1;
		apply_class_boost(tctx, 7);
		
		/* EXTENDED WAKE CHAINS: Detect Wine and SDL input threads for signal propagation
		 * This enables wake chain signals to flow through Wine/SDL input handlers
		 * to game threads, reducing latency for Proton games and SDL-based games. */
		if (is_wine_input_thread(p->comm))
			tctx->is_wine_input = 1;
		else if (is_sdl_event_thread(p->comm))
			tctx->is_sdl_event = 1;
		
		if (likely(tctx->input_lane == INPUT_LANE_OTHER)) {
			if (comm_contains(p->comm, "mouse", 5))
				tctx->input_lane = INPUT_LANE_MOUSE;
			else if (comm_contains(p->comm, "kbd", 3) ||
				 comm_contains(p->comm, "keyboard", 8))
				tctx->input_lane = INPUT_LANE_KEYBOARD;
		}
	}
}

/**
 * classify_gpu_submit - Classify thread as GPU submission thread
 * @p: Task struct pointer
 * @tctx: Task context to update
 *
 * TIER 0/1: Optimized for thread classification hot path (~6-32ns)
 */
static __always_inline void classify_gpu_submit(struct task_struct *p, struct task_ctx *tctx)
{
	if (likely(!tctx->is_gpu_submit && is_gpu_submit_name(p->comm))) {
		tctx->is_gpu_submit = 1;
		apply_class_boost(tctx, 6);
	}
}

/* Forward declaration - full definition in Discord detection section */
static __always_inline bool is_voice_chat_audio_thread(const char *comm);

static __always_inline void classify_audio(struct task_struct *p, struct task_ctx *tctx)
{
	/* LAYER 1 (HIGHEST PRIORITY): TGID-based system audio detection
	 * Check if this thread belongs to a known audio server process (PipeWire, PulseAudio, etc.).
	 * This catches ALL threads in audio server processes, regardless of thread name.
	 * Fast O(1) hash lookup (~20-40ns) vs name pattern matching.
	 */
	if (!tctx->is_system_audio) {
		u32 tgid = (u32)p->tgid;
		struct system_audio_entry *entry = bpf_map_lookup_elem(&system_audio_tgids_map, &tgid);
		if (entry && entry->refcount > 0) {
			tctx->is_system_audio = 1;
			apply_class_boost(tctx, 2);  /* AUDIO IMPROVEMENT: Elevated to match USB audio */
		}
	}
	
	/* LAYER 2: Name-based detection (fallback for threads not in audio server processes) */
	if (!tctx->is_system_audio && is_system_audio_name(p->comm)) {
		tctx->is_system_audio = 1;
		apply_class_boost(tctx, 2);  /* AUDIO IMPROVEMENT: Elevated to match USB audio */
	}
	
	/* LAYER 3: Game audio detection (FMOD, Wwise, Vivox, etc.)
	 * Game audio is critical for gameplay (footsteps, gunshots, voice chat) */
	if (!tctx->is_game_audio && is_game_audio_name(p->comm)) {
		tctx->is_game_audio = 1;
		apply_class_boost(tctx, 2);  /* AUDIO IMPROVEMENT: Same priority as USB audio */
	}
	
	/* LAYER 4: Voice chat audio detection (Discord, TeamSpeak, etc.)
	 * Voice threads need low latency for clear teammate communication */
	if (!tctx->is_system_audio && is_voice_chat_audio_thread(p->comm)) {
		tctx->is_system_audio = 1;
		apply_class_boost(tctx, 2);  /* Same priority as USB audio for clear comms */
	}
}

static __always_inline void classify_network(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_network && is_network_name(p->comm)) {
		tctx->is_network = 1;
		apply_class_boost(tctx, 3);
	}
}

/**
 * is_compiler_name - Detect compiler/build tool processes
 * @comm: Process name
 *
 * Compilers and build tools are EXTREMELY CPU-intensive and should
 * be heavily penalized when gaming. These processes can easily
 * consume 100% of multiple cores and pollute caches.
 *
 * Returns: true if this is a compiler/build tool
 */
static __always_inline bool is_compiler_name(const char *comm)
{
	/* Rust toolchain - cargo, rustc, rustdoc, rust-analyzer */
	if (comm[0] == 'c' && comm[1] == 'a' && comm[2] == 'r' && comm[3] == 'g' &&
	    comm[4] == 'o')
		return true;  /* cargo */

	if (comm[0] == 'r' && comm[1] == 'u' && comm[2] == 's' && comm[3] == 't') {
		if (comm[4] == 'c')
			return true;  /* rustc */
		if (comm[4] == 'd' && comm[5] == 'o' && comm[6] == 'c')
			return true;  /* rustdoc */
		if (comm[4] == '-' && comm[5] == 'a' && comm[6] == 'n')
			return true;  /* rust-analyzer */
	}

	/* C/C++ compilers - gcc, g++, clang, clang++, cc, c++ */
	if (comm[0] == 'g' && comm[1] == 'c' && comm[2] == 'c')
		return true;  /* gcc */

	if (comm[0] == 'g' && comm[1] == '+' && comm[2] == '+')
		return true;  /* g++ */

	if (comm[0] == 'c' && comm[1] == 'l' && comm[2] == 'a' && comm[3] == 'n' &&
	    comm[4] == 'g')
		return true;  /* clang, clang++ */

	if (comm[0] == 'c' && comm[1] == 'c' && (comm[2] == '\0' || comm[2] == '1'))
		return true;  /* cc, cc1 */

	if (comm[0] == 'c' && comm[1] == '+' && comm[2] == '+')
		return true;  /* c++ */

	/* Linkers - ld, lld, gold, mold */
	if (comm[0] == 'l' && comm[1] == 'd' && (comm[2] == '\0' || comm[2] == '.'))
		return true;  /* ld, ld.bfd, ld.gold */

	if (comm[0] == 'l' && comm[1] == 'l' && comm[2] == 'd')
		return true;  /* lld */

	if (comm[0] == 'g' && comm[1] == 'o' && comm[2] == 'l' && comm[3] == 'd')
		return true;  /* gold */

	if (comm[0] == 'm' && comm[1] == 'o' && comm[2] == 'l' && comm[3] == 'd')
		return true;  /* mold */

	/* Build systems - make, ninja, cmake */
	if (comm[0] == 'm' && comm[1] == 'a' && comm[2] == 'k' && comm[3] == 'e')
		return true;  /* make */

	if (comm[0] == 'n' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'j' &&
	    comm[4] == 'a')
		return true;  /* ninja */

	if (comm[0] == 'c' && comm[1] == 'm' && comm[2] == 'a' && comm[3] == 'k' &&
	    comm[4] == 'e')
		return true;  /* cmake */

	/* LLVM tools */
	if (comm[0] == 'l' && comm[1] == 'l' && comm[2] == 'v' && comm[3] == 'm')
		return true;  /* llvm-* tools */

	/* Node.js build tools (npm, yarn, node when building) */
	if (comm[0] == 'n' && comm[1] == 'p' && comm[2] == 'm')
		return true;  /* npm */

	if (comm[0] == 'y' && comm[1] == 'a' && comm[2] == 'r' && comm[3] == 'n')
		return true;  /* yarn */

	/* Other compilers/interpreters doing heavy work */
	if (comm[0] == 'j' && comm[1] == 'a' && comm[2] == 'v' && comm[3] == 'a' &&
	    comm[4] == 'c')
		return true;  /* javac */

	if (comm[0] == 'g' && comm[1] == 'o' && comm[2] == ' ' && comm[3] == 'b')
		return true;  /* go build */

	/* Assembler */
	if (comm[0] == 'a' && comm[1] == 's' && (comm[2] == '\0' || comm[2] == ' '))
		return true;  /* as (GNU assembler) */

	return false;
}

static __always_inline bool is_background_name(const char *comm)
{
	/* COMPILERS: Highest priority for background detection
	 * These are extremely CPU-intensive and should be heavily penalized */
	if (is_compiler_name(comm))
		return true;

	/* GPU render threads often treated as background when they go idle */
	if (comm[0] == 'R' && comm[1] == 'e' && comm[2] == 'n' && comm[3] == 'd' &&
	    comm[4] == 'e' && comm[5] == 'r' && comm[6] == 'T')
		return true;

	if (comm[0] == 'v' && comm[1] == 'k' && comm[2] == 'd' && comm[3] == '3')
		return true;

	if (comm[0] == '[' && comm[1] == 'v' && comm[2] == 'k')
		return true;

	if (comm[0] == 'U' && comm[1] == 'n' && comm[2] == 'i' && comm[3] == 't' &&
	    comm[4] == 'y' && comm[5] == 'G' && comm[6] == 'f' && comm[7] == 'x')
		return true;

	if (comm[0] == 'r' && comm[1] == 'e' && comm[2] == 'n' && comm[3] == 'd')
		return true;

	if (comm[0] == 'g' && comm[1] == 'p' && comm[2] == 'u')
		return true;

	/* Steam WebHelper - CPU-intensive browser component that should be throttled */
	if (comm[0] == 's' && comm[1] == 't' && comm[2] == 'e' && comm[3] == 'a' &&
	    comm[4] == 'm' && comm[5] == 'w' && comm[6] == 'e' && comm[7] == 'b')
		return true;  /* steamwebhelper */

	/* Cursor/VS Code - Electron-based editor that can consume significant CPU */
	if (comm[0] == 'c' && comm[1] == 'u' && comm[2] == 'r' && comm[3] == 's' &&
	    comm[4] == 'o' && comm[5] == 'r')
		return true;  /* cursor */

	/* Plasma System Monitor - System monitoring tool that should be background */
	if (comm[0] == 'p' && comm[1] == 'l' && comm[2] == 'a' && comm[3] == 's' &&
	    comm[4] == 'm' && comm[5] == 'a' && comm[6] == '-' && comm[7] == 's')
		return true;  /* plasma-systemmonitor */

	/* Discord - Electron-based communication app that can consume significant CPU */
	if (comm[0] == 'd' && comm[1] == 'i' && comm[2] == 's' && comm[3] == 'c' &&
	    comm[4] == 'o' && comm[5] == 'r' && comm[6] == 'd')
		return true;  /* discord */

	/* Chromium - Web browser that can consume significant CPU for rendering */
	if (comm[0] == 'c' && comm[1] == 'h' && comm[2] == 'r' && comm[3] == 'o' &&
	    comm[4] == 'm' && comm[5] == 'i' && comm[6] == 'u' && comm[7] == 'm')
		return true;  /* chromium */

	/* World of Warcraft background threads - non-critical game threads */
	if (comm[0] == 'T' && comm[1] == 'A' && comm[2] == 'C' && comm[3] == 'T' &&
	    comm[4] == ' ' && comm[5] == 'T' && comm[6] == 'a' && comm[7] == 's')
		return true;  /* TACT Task Threa */

	if (comm[0] == 'W' && comm[1] == 'o' && comm[2] == 'w' && comm[3] == 'D' &&
	    comm[4] == 'o' && comm[5] == 'w' && comm[6] == 'n' && comm[7] == 'l')
		return true;  /* WowDownloadDisp */

	if (comm[0] == 'T' && comm[1] == 'A' && comm[2] == 'C' && comm[3] == 'T' &&
	    comm[4] == ' ' && comm[5] == 'C' && comm[6] == 'u' && comm[7] == 'r')
		return true;  /* TACT Curl Downl */

	if (comm[0] == 'W' && comm[1] == 'a' && comm[2] == 't' && comm[3] == 'c' &&
	    comm[4] == 'h' && comm[5] == 'd' && comm[6] == 'o' && comm[7] == 'g')
		return true;  /* WatchdogThread */

	if (comm[0] == 'C' && comm[1] == 'o' && comm[2] == 'm' && comm[3] == 'b' &&
	    comm[4] == 'a' && comm[5] == 't' && comm[6] == ' ' && comm[7] == 'L')
		return true;  /* Combat Log Thre */

	if (comm[0] == 'A' && comm[1] == 's' && comm[2] == 'y' && comm[3] == 'n' &&
	    comm[4] == 'c' && comm[5] == ' ' && comm[6] == 'P' && comm[7] == 'e')
		return true;  /* Async Pending */

	if (comm[0] == 'A' && comm[1] == 'I' && comm[2] == 'O' && comm[3] == ' ' &&
	    comm[4] == 'T' && comm[5] == 'h' && comm[6] == 'r' && comm[7] == 'e')
		return true;  /* AIO Thread */

	if (comm[0] == 'D' && comm[1] == 'i' && comm[2] == 's' && comm[3] == 'k' &&
	    comm[4] == ' ' && comm[5] == 'Q' && comm[6] == 'u' && comm[7] == 'e')
		return true;  /* Disk Queue 0 */

	if (comm[0] == 'c' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'a' &&
	    comm[4] == '-' && comm[5] == 'E' && comm[6] == 'v' && comm[7] == 't')
		return true;  /* cuda-EvtHandlr */

	if (comm[0] == 'c' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'a' &&
	    comm[4] == '0' && comm[5] == '0' && comm[6] == '0' && comm[7] == '8')
		return true;  /* cuda00089400226 */

	if (comm[0] == 'C' && comm[1] == 'O' && comm[2] == 'M' && comm[3] == 'M' &&
	    comm[4] == 'A' && comm[5] == 'N' && comm[6] == 'D')
		return true;  /* COMMAND */

	return false;
}

/*
 * Gaming peripheral device detection
 * Gaming peripherals (Razer, Logitech, Corsair) often have specialized drivers
 * that require low-latency processing for optimal gaming performance
 */
static __always_inline bool is_gaming_peripheral_thread(const char *comm)
{
	/* Razer peripheral drivers */
	if (comm[0] == 'r' && comm[1] == 'a' && comm[2] == 'z' && comm[3] == 'e') {
		if (comm[4] == 'r' && comm[5] == '_') return true;  /* razer_* */
		if (comm[4] == 'c' && comm[5] == 'o') return true;  /* razercore */
		if (comm[4] == 's' && comm[5] == 'y') return true;  /* razersynapse */
	}

	/* Logitech gaming peripherals */
	if (comm[0] == 'l' && comm[1] == 'o' && comm[2] == 'g' && comm[3] == 'i') {
		if (comm[4] == 't' && comm[5] == 'e' && comm[6] == 'c' && comm[7] == 'h')
			return true;  /* logitech */
		if (comm[4] == 'g' && comm[5] == 'h' && comm[6] == 'u' && comm[7] == 'b')
			return true;  /* logitech_hub */
	}

	/* Corsair gaming peripherals */
	if (comm[0] == 'c' && comm[1] == 'o' && comm[2] == 'r' && comm[3] == 's') {
		if (comm[4] == 'a' && comm[5] == 'i' && comm[6] == 'r')
			return true;  /* corsair */
		if (comm[4] == 'i' && comm[5] == 'c' && comm[6] == 'u' && comm[7] == 'e')
			return true;  /* corsair_icue */
	}

	/* SteelSeries gaming peripherals */
	if (comm[0] == 's' && comm[1] == 't' && comm[2] == 'e' && comm[3] == 'e') {
		if (comm[4] == 'l' && comm[5] == 's' && comm[6] == 'e' && comm[7] == 'r')
			return true;  /* steelseries */
		if (comm[4] == 'e' && comm[5] == 'n' && comm[6] == 'g' && comm[7] == 'i')
			return true;  /* steelengine */
	}

	/* ASUS ROG gaming peripherals */
	if (comm[0] == 'a' && comm[1] == 's' && comm[2] == 'u' && comm[3] == 's') {
		if (comm[4] == '_' && comm[5] == 'r' && comm[6] == 'o' && comm[7] == 'g')
			return true;  /* asus_rog */
		if (comm[4] == '_' && comm[5] == 'a' && comm[6] == 'r' && comm[7] == 'm')
			return true;  /* asus_armoury */
	}

	/* MSI gaming peripherals */
	if (comm[0] == 'm' && comm[1] == 's' && comm[2] == 'i') {
		if (comm[3] == '_' && comm[4] == 'd' && comm[5] == 'r' && comm[6] == 'a')
			return true;  /* msi_dragon */
		if (comm[3] == '_' && comm[4] == 'm' && comm[5] == 'y' && comm[6] == 's')
			return true;  /* msi_mystic */
	}

	return false;
}

/*
 * Gaming traffic pattern detection
 * Gaming traffic typically has high frequency, small packet sizes
 * This helps identify real-time gaming communication vs bulk data transfer
 */
static __always_inline bool is_gaming_traffic_pattern(const struct task_struct *p, struct task_ctx *tctx)
{
	/* High wakeup frequency indicates real-time communication */
	if (tctx->wakeup_freq < 100)  /* Less than 100Hz */
		return false;
	
	/* Check for small packet patterns in network I/O */
	u64 read_bytes = BPF_CORE_READ(p, ioac.read_bytes);
	u64 write_bytes = BPF_CORE_READ(p, ioac.write_bytes);
	u64 read_chars = BPF_CORE_READ(p, ioac.rchar);
	u64 write_chars = BPF_CORE_READ(p, ioac.wchar);
	
	/* Gaming traffic: many small packets (high char count vs low byte count) */
	if (read_chars > 0 && read_bytes > 0) {
		u64 packet_ratio = read_chars / read_bytes;
		/* Gaming traffic typically has ratio > 10 (many small packets) */
		if (packet_ratio > 10) {
			return true;
		}
	}
	
	if (write_chars > 0 && write_bytes > 0) {
		u64 packet_ratio = write_chars / write_bytes;
		/* Gaming traffic typically has ratio > 10 (many small packets) */
		if (packet_ratio > 10) {
			return true;
		}
	}
	
	/* High frequency wakeups with low CPU usage indicate network I/O wait */
	if (tctx->wakeup_freq > 200 && tctx->exec_avg < 1000) {  /* >200Hz, <1ms exec */
		return true;
	}
	
	return false;
}

/*
 * Audio pipeline thread detection
 * Audio pipeline threads handle real-time audio processing chains
 * These require ultra-low latency for seamless audio experience
 */
static __always_inline bool is_audio_pipeline_thread(const char *comm)
{
	/* Audio pipeline processing threads */
	if (comm[0] == 'A' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'i' && comm[4] == 'o') {
		if (comm[5] == 'P' && comm[6] == 'i' && comm[7] == 'p') return true;  /* AudioPipeline */
		if (comm[5] == 'P' && comm[6] == 'r' && comm[7] == 'o') return true;  /* AudioProcessor */
		if (comm[5] == 'C' && comm[6] == 'h' && comm[7] == 'a') return true;  /* AudioChannel */
		if (comm[5] == 'M' && comm[6] == 'i' && comm[7] == 'x') return true;  /* AudioMixer */
	}

	/* Real-time audio processing */
	if (comm[0] == 'R' && comm[1] == 'T' && comm[2] == 'A' && comm[3] == 'u') return true;  /* RTAudio */
	if (comm[0] == 'R' && comm[1] == 'e' && comm[2] == 'a' && comm[3] == 'l' && comm[4] == 'T') return true;  /* RealTime */

	/* Audio effects processing */
	if (comm[0] == 'A' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'i' && comm[4] == 'o' && comm[5] == 'E') return true;  /* AudioEffect */
	if (comm[0] == 'E' && comm[1] == 'f' && comm[2] == 'f' && comm[3] == 'e' && comm[4] == 'c' && comm[5] == 't') return true;  /* Effect */

	/* Audio codec processing */
	if (comm[0] == 'A' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'i' && comm[4] == 'o' && comm[5] == 'C') return true;  /* AudioCodec */
	if (comm[0] == 'C' && comm[1] == 'o' && comm[2] == 'd' && comm[3] == 'e' && comm[4] == 'c') return true;  /* Codec */

	/* Audio streaming */
	if (comm[0] == 'A' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'i' && comm[4] == 'o' && comm[5] == 'S') return true;  /* AudioStream */
	if (comm[0] == 'S' && comm[1] == 't' && comm[2] == 'r' && comm[3] == 'e' && comm[4] == 'a' && comm[5] == 'm') return true;  /* Stream */

	return false;
}

/*
 * Storage hot path detection for I/O intensive operations
 * Storage hot path threads have high I/O wait and specific patterns
 * These benefit from maximum boost and longer slices for optimal throughput
 */
static __always_inline bool is_storage_hot_path_thread(const struct task_struct *p, struct task_ctx *tctx)
{
	/* Very high page fault rate indicates intensive storage operations */
	if (tctx->pgfault_rate <= 300)
		return false;
	
	/* Check for high I/O wait patterns */
	u64 voluntary_switches = BPF_CORE_READ(p, nvcsw);
	u64 involuntary_switches = BPF_CORE_READ(p, nivcsw);
	
	if (voluntary_switches > 0) {
		u64 total_switches = voluntary_switches + involuntary_switches;
		u64 io_wait_ratio = (voluntary_switches * 100) / total_switches;
		
		/* Storage hot path threads have >60% I/O wait */
		if (io_wait_ratio > 60) {
			return true;
		}
	}
	
	/* Check for high I/O throughput patterns */
	u64 read_bytes = BPF_CORE_READ(p, ioac.read_bytes);
	u64 write_bytes = BPF_CORE_READ(p, ioac.write_bytes);
	u64 total_io_bytes = read_bytes + write_bytes;
	
	/* High I/O throughput indicates storage-intensive operations */
	if (total_io_bytes > 1000000) {  /* >1MB I/O */
		return true;
	}
	
	/* High frequency wakeups with high I/O wait indicate storage hot path */
	if (tctx->wakeup_freq > 150 && tctx->exec_avg < 2000) {  /* >150Hz, <2ms exec */
		return true;
	}
	
	return false;
}

/*
 * Ethernet NIC interrupt thread detection
 * Ethernet NIC interrupt threads handle network packet processing
 * These require low-latency processing for optimal gaming network performance
 */
static __always_inline bool is_ethernet_nic_interrupt_thread(const char *comm)
{
	/* Ethernet NIC interrupt thread patterns */
	if (comm[0] == 'i' && comm[1] == 'r' && comm[2] == 'q' && comm[3] == '/') {
		/* irq/eth0, irq/eth1, etc. */
		return true;
	}

	/* Network interface interrupt handlers */
	if (comm[0] == 'n' && comm[1] == 'e' && comm[2] == 't' && comm[3] == 'i') {
		if (comm[4] == 'f' && comm[5] == '_') return true;  /* netif_* */
		if (comm[4] == 'r' && comm[5] == 'x') return true;  /* netirq_* */
	}

	/* Ethernet driver interrupt handlers */
	if (comm[0] == 'e' && comm[1] == 't' && comm[2] == 'h' && comm[3] == '_') return true;  /* eth_* */
	if (comm[0] == 'e' && comm[1] == 't' && comm[2] == 'h' && comm[3] == 'e') return true;  /* ethe* */

	/* Generic network interrupt handlers */
	if (comm[0] == 'n' && comm[1] == 'e' && comm[2] == 't' && comm[3] == '_') return true;  /* net_* */
	if (comm[0] == 'n' && comm[1] == 'e' && comm[2] == 't' && comm[3] == 'r') return true;  /* netr* */

	/* PCIe network device interrupt handlers */
	if (comm[0] == 'p' && comm[1] == 'c' && comm[2] == 'i' && comm[3] == '_') {
		if (comm[4] == 'n' && comm[5] == 'e' && comm[6] == 't') return true;  /* pci_net* */
	}

	return false;
}

static __always_inline void classify_gaming_peripheral(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_gaming_peripheral && is_gaming_peripheral_thread(p->comm)) {
		tctx->is_gaming_peripheral = 1;
		apply_class_boost(tctx, 1);
	}
}

static __always_inline void classify_gaming_traffic(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_gaming_traffic && is_gaming_traffic_pattern(p, tctx)) {
		tctx->is_gaming_traffic = 1;
		apply_class_boost(tctx, 3);
	}
}

static __always_inline void classify_audio_pipeline(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_audio_pipeline && is_audio_pipeline_thread(p->comm)) {
		tctx->is_audio_pipeline = 1;
		apply_class_boost(tctx, 1);
	}
}

static __always_inline void classify_storage_hot_path(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_storage_hot_path && is_storage_hot_path_thread(p, tctx)) {
		tctx->is_storage_hot_path = 1;
		apply_class_boost(tctx, 1);
	}
}

static __always_inline void classify_ethernet_nic_interrupt(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_ethernet_nic_interrupt && is_ethernet_nic_interrupt_thread(p->comm)) {
		tctx->is_ethernet_nic_interrupt = 1;
		apply_class_boost(tctx, 4);
	}
}

/*
 * Steam WebHelper detection - CPU-intensive browser component
 * Steam WebHelper runs Chromium-based browser components for Steam UI
 * These should be heavily throttled to preserve game performance
 */
static __always_inline bool is_steam_webhelper_name(const char *comm)
{
	/* Steam WebHelper process name pattern */
	if (comm[0] == 's' && comm[1] == 't' && comm[2] == 'e' && comm[3] == 'a' &&
	    comm[4] == 'm' && comm[5] == 'w' && comm[6] == 'e' && comm[7] == 'b')
		return true;  /* steamwebhelper */

	return false;
}

static __always_inline void classify_steam_webhelper(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_background && is_steam_webhelper_name(p->comm)) {
		tctx->is_background = 1;
		/* Steam WebHelper gets maximum background penalty (8x slower) */
		/* This ensures it doesn't compete with game threads for CPU time */
	}
}

/*
 * Cursor/VS Code detection - Electron-based editor processes
 * Cursor and VS Code are Electron-based editors that can consume significant CPU
 * These should be throttled when not in foreground to preserve game performance
 */
static __always_inline bool is_cursor_name(const char *comm)
{
	/* Cursor editor process name pattern */
	if (comm[0] == 'c' && comm[1] == 'u' && comm[2] == 'r' && comm[3] == 's' &&
	    comm[4] == 'o' && comm[5] == 'r')
		return true;  /* cursor */

	return false;
}

static __always_inline void classify_cursor(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_background && is_cursor_name(p->comm)) {
		tctx->is_background = 1;
		/* Cursor gets background penalty (8x slower) when not in foreground */
		/* This prevents editor from competing with games for CPU time */
	}
}

/*
 * Plasma System Monitor detection - System monitoring tool
 * Plasma System Monitor can consume CPU for system monitoring
 * Should be throttled to preserve game performance
 */
static __always_inline bool is_plasma_systemmonitor_name(const char *comm)
{
	/* Plasma System Monitor process name pattern */
	if (comm[0] == 'p' && comm[1] == 'l' && comm[2] == 'a' && comm[3] == 's' &&
	    comm[4] == 'm' && comm[5] == 'a' && comm[6] == '-' && comm[7] == 's')
		return true;  /* plasma-systemmonitor */

	return false;
}

static __always_inline void classify_plasma_systemmonitor(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_background && is_plasma_systemmonitor_name(p->comm)) {
		tctx->is_background = 1;
		/* Plasma System Monitor gets background penalty (8x slower) */
		/* System monitoring should not interfere with gaming performance */
	}
}

/*
 * Voice chat audio thread detection
 * Detects audio/voice threads from voice chat applications (Discord, TeamSpeak, Mumble, etc.)
 * These threads should get audio priority for clear comms with teammates.
 * 
 * Thread patterns:
 * - AudioDevice*, AudioInput*, AudioOutput* (generic audio threads)
 * - WebRTC*, webrtc* (real-time communication)
 * - opus*, Opus* (voice codec)
 * - voice*, Voice* (voice processing)
 */
static __always_inline bool is_voice_chat_audio_thread(const char *comm)
{
	/* WebRTC audio/voice processing (Discord, browser-based voice) */
	if (comm[0] == 'W' && comm[1] == 'e' && comm[2] == 'b' && comm[3] == 'R' && comm[4] == 'T' && comm[5] == 'C')
		return true;  /* WebRTC* */
	if (comm[0] == 'w' && comm[1] == 'e' && comm[2] == 'b' && comm[3] == 'r' && comm[4] == 't' && comm[5] == 'c')
		return true;  /* webrtc* */

	/* Opus codec (used by Discord, many VoIP apps) */
	if (comm[0] == 'O' && comm[1] == 'p' && comm[2] == 'u' && comm[3] == 's')
		return true;  /* Opus* */
	if (comm[0] == 'o' && comm[1] == 'p' && comm[2] == 'u' && comm[3] == 's')
		return true;  /* opus* */

	/* Audio device/input/output threads */
	if (comm[0] == 'A' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'i' && comm[4] == 'o') {
		if (comm[5] == 'D' || comm[5] == 'I' || comm[5] == 'O' || comm[5] == 'C')
			return true;  /* AudioDevice, AudioInput, AudioOutput, AudioCapture */
	}

	/* PulseAudio/PipeWire sink threads from apps */
	if (comm[0] == 'p' && comm[1] == 'u' && comm[2] == 'l' && comm[3] == 's' && comm[4] == 'e')
		return true;  /* pulse* (app PulseAudio threads) */

	/* TeamSpeak patterns */
	if (comm[0] == 't' && comm[1] == 's' && comm[2] == '3')
		return true;  /* ts3* (TeamSpeak 3) */
	if (comm[0] == 'T' && comm[1] == 'e' && comm[2] == 'a' && comm[3] == 'm' && comm[4] == 'S')
		return true;  /* TeamSpeak, TeamS* */

	/* Mumble patterns */
	if (comm[0] == 'm' && comm[1] == 'u' && comm[2] == 'm' && comm[3] == 'b' && comm[4] == 'l')
		return true;  /* mumble */
	if (comm[0] == 'M' && comm[1] == 'u' && comm[2] == 'm' && comm[3] == 'b' && comm[4] == 'l')
		return true;  /* Mumble */

	return false;
}

/*
 * Discord detection - Electron-based communication application
 * Discord is an Electron-based app that can consume significant CPU for UI/JS
 * 
 * AUDIO IMPROVEMENT: Discord voice threads should NOT be penalized.
 * Only Discord UI/renderer threads get background penalty.
 * Voice threads are detected separately and get audio priority.
 */
static __always_inline bool is_discord_name(const char *comm)
{
	/* Discord process name pattern */
	if (comm[0] == 'd' && comm[1] == 'i' && comm[2] == 's' && comm[3] == 'c' &&
	    comm[4] == 'o' && comm[5] == 'r' && comm[6] == 'd')
		return true;  /* discord */
	/* Discord Canary/PTB variants */
	if (comm[0] == 'D' && comm[1] == 'i' && comm[2] == 's' && comm[3] == 'c' &&
	    comm[4] == 'o' && comm[5] == 'r' && comm[6] == 'd')
		return true;  /* Discord* */

	return false;
}

static __always_inline void classify_discord(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_background && is_discord_name(p->comm)) {
		/* AUDIO IMPROVEMENT: Check if this is a voice/audio thread first
		 * Voice threads should NOT be classified as background */
		if (is_voice_chat_audio_thread(p->comm)) {
			/* This is a Discord voice thread - give it system audio priority */
			if (!tctx->is_system_audio) {
				tctx->is_system_audio = 1;
				apply_class_boost(tctx, 2);  /* Same boost as USB audio */
			}
			return;  /* Don't mark as background! */
		}
		
		tctx->is_background = 1;
		/* Discord UI/renderer gets background penalty (8x slower) when not in foreground */
		/* This prevents Discord from competing with games for CPU time */
	}
}

/*
 * Voice chat application audio classification
 * Detects and prioritizes voice threads from any voice chat app
 * Called during classify_audio to catch voice threads early
 */
static __always_inline void classify_voice_chat_audio(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_system_audio && is_voice_chat_audio_thread(p->comm)) {
		tctx->is_system_audio = 1;
		apply_class_boost(tctx, 2);  /* Same priority as USB audio for clear comms */
	}
}

/*
 * Chromium detection - Web browser application
 * Chromium is a web browser that can consume significant CPU for rendering
 * Should be throttled when not in foreground to preserve game performance
 */
static __always_inline bool is_chromium_name(const char *comm)
{
	/* Chromium process name pattern */
	if (comm[0] == 'c' && comm[1] == 'h' && comm[2] == 'r' && comm[3] == 'o' &&
	    comm[4] == 'm' && comm[5] == 'i' && comm[6] == 'u' && comm[7] == 'm')
		return true;  /* chromium */

	return false;
}

static __always_inline void classify_chromium(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_background && is_chromium_name(p->comm)) {
		tctx->is_background = 1;
		/* Chromium gets background penalty (8x slower) when not in foreground */
		/* This prevents web browser from competing with games for CPU time */
	}
}

static __always_inline void classify_background(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_background && is_background_name(p->comm))
		tctx->is_background = 1;

	/* COMPILER DETECTION: Flag compilers for extra-heavy penalty
	 * This is critical for preventing cargo/rustc/gcc builds from
	 * causing game lockups. Compilers get 32x penalty vs 8x for regular background. */
	if (!tctx->is_compiler && is_compiler_name(p->comm)) {
		tctx->is_compiler = 1;
		tctx->is_background = 1;  /* Also mark as background */
	}
}

/**
 * classify_task - Classify thread into categories
 * @p: Task struct pointer
 * @tctx: Task context to update
 *
 * TIER 1: Thread classification (called during thread wakeup)
 * - Multiple name pattern checks: Tier 0 (~5-30ns each)
 * - Struct field writes: Tier 0 (~1-2ns each)
 * - Map lookups (for audio): Tier 1 (~20-50ns)
 * - Total: ~100-500ns (depending on matches)
 *
 * Frequency: Called during thread classification (thousands/sec during startup,
 *            then cached in task_ctx for subsequent checks)
 * Net overhead: Minimal (results cached in task_ctx)
 */
static __always_inline void classify_task(struct task_struct *p, struct task_ctx *tctx)
{
    classify_input_handler(p, tctx);
    classify_gpu_submit(p, tctx);
    classify_audio(p, tctx);
    classify_network(p, tctx);
    classify_gaming_peripheral(p, tctx);
    classify_gaming_traffic(p, tctx);
    classify_audio_pipeline(p, tctx);
    classify_storage_hot_path(p, tctx);
    classify_ethernet_nic_interrupt(p, tctx);
    classify_steam_webhelper(p, tctx);  /* Steam WebHelper throttling */
    classify_cursor(p, tctx);           /* Cursor/VS Code throttling */
    classify_plasma_systemmonitor(p, tctx); /* Plasma System Monitor throttling */
    classify_discord(p, tctx);          /* Discord throttling */
    classify_chromium(p, tctx);         /* Chromium throttling */
    classify_background(p, tctx);

    if (unlikely(!tctx->input_lane))
        tctx->input_lane = INPUT_LANE_OTHER;
 }

#endif /* __GAMER_TASK_CLASS_BPF_H */
