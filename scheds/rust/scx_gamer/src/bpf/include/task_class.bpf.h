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

/*
 * GPU submission threads - critical for frame presentation
 * Examples: vkd3d-swapchain, dxvk-submit, RenderThread 0, RHIThread
 *
 * CRITICAL FOR SPLITGATE: Unreal Engine 4 uses RenderThread for GPU submission.
 * This thread must get physical cores (no SMT) to avoid frame pacing issues.
 */
static __always_inline bool is_gpu_submit_name(const char *comm)
{
	/* DXVK threads (DX9/10/11 to Vulkan translation - VERY common with Proton) */
	if (comm[0] == 'd' && comm[1] == 'x' && comm[2] == 'v' && comm[3] == 'k' && comm[4] == '-')
		return true;  /* dxvk-submit, dxvk-queue, dxvk-frame, dxvk-cs, dxvk-shader-* */

	/* Unreal Engine RHI (Render Hardware Interface) threads */
	if (comm[0] == 'R' && comm[1] == 'H' && comm[2] == 'I')
		return true;  /* RHIThread, RHISubmissionTh, RHIInterruptThr */

	/* Unreal Engine RenderThread (Splitgate, Fortnite, etc.) - CRITICAL PATH */
	if (comm[0] == 'R' && comm[1] == 'e' && comm[2] == 'n' && comm[3] == 'd' &&
	    comm[4] == 'e' && comm[5] == 'r' && comm[6] == 'T')
		return true;  /* RenderThread 0 */

	/* vkd3d threads (Vulkan/D3D12 translation layer for Proton) */
	if (comm[0] == 'v' && comm[1] == 'k' && comm[2] == 'd' && comm[3] == '3')
		return true;  /* vkd3d_queue, vkd3d_fence, vkd3d-swapchain */

	/* Bracketed Vulkan threads (WoW, etc.) */
	if (comm[0] == '[' && comm[1] == 'v' && comm[2] == 'k')
		return true;  /* [vkrt] Analysis, [vkps] Update, [vkcf] Analysis */

	/* Unity render threads */
	if (comm[0] == 'U' && comm[1] == 'n' && comm[2] == 'i' && comm[3] == 't' &&
	    comm[4] == 'y' && comm[5] == 'G' && comm[6] == 'f' && comm[7] == 'x')
		return true;  /* UnityGfxDevice */

	/* Generic "render" or "gpu" thread names */
	if (comm[0] == 'r' && comm[1] == 'e' && comm[2] == 'n' && comm[3] == 'd' &&
	    comm[4] == 'e' && comm[5] == 'r')
		return true;

	if (comm[0] == 'g' && comm[1] == 'p' && comm[2] == 'u')
		return true;

	return false;
}

/*
 * Compositor/window manager threads
 * Examples: kwin_wayland, mutter, weston
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
	/* PipeWire audio server (modern Linux standard) */
	if (comm[0] == 'p' && comm[1] == 'i' && comm[2] == 'p' && comm[3] == 'e')
		return true;

	/* Check for "pipewire" or "pw-" prefix */
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
	
	/* Higher boost for smaller buffers (lower latency requirements) */
	if (buffer_size <= 32) boost += 3;      /* Ultra-low latency */
	else if (buffer_size <= 64) boost += 2; /* Low latency */
	else if (buffer_size <= 128) boost += 1; /* Medium latency */
	
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
 * Game audio threads (lower priority than system audio)
 * Game audio is important for immersion but shouldn't delay input processing.
 * Examples: AudioDeviceBuff, FMODThread, AudioEncoder, OpenAL
 */
static __always_inline bool is_game_audio_name(const char *comm)
{
	/* Unreal Engine audio threads */
	if (comm[0] == 'A' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'i' && comm[4] == 'o')
		return true;  /* AudioDeviceBuff, AudioThread0, etc. */

	if (comm[0] == 'F' && comm[1] == 'A' && comm[2] == 'u' && comm[3] == 'd')
		return true;  /* FAudio_AudioCli */

	/* Bink audio (common video codec in games) */
	if (comm[0] == 'B' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'k')
		return true;  /* Bink Snd */

	/* Generic game audio threads: "audio", "sound", "snd_" */
	if (comm[0] == 'a' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'i' && comm[4] == 'o')
		return true;

	if (comm[0] == 's' && comm[1] == 'o' && comm[2] == 'u' && comm[3] == 'n' && comm[4] == 'd')
		return true;

	if (comm[0] == 's' && comm[1] == 'n' && comm[2] == 'd' && comm[3] == '_')
		return true;

	/* OpenAL (common game audio library) */
	if (comm[0] == 'o' && comm[1] == 'p' && comm[2] == 'e' && comm[3] == 'n' && comm[4] == 'a')
		return true;

	/* FMOD (game audio engine) */
	if (comm[0] == 'f' && comm[1] == 'm' && comm[2] == 'o' && comm[3] == 'd')
		return true;

	/* Wwise (game audio engine) */
	if (comm[0] == 'w' && comm[1] == 'w' && comm[2] == 'i' && comm[3] == 's' && comm[4] == 'e')
		return true;

	return false;
}

/*
 * Input handler threads - HIGHEST priority for gaming
 * Mouse/keyboard lag is THE WORST experience for gamers.
 * Examples: GameThread (Unreal), InputThread, SDL, EventHandler
 *
 * CRITICAL FOR SPLITGATE: UE4 processes input on GameThread, not a separate thread!
 * At 480Hz (2083µs/frame), input must reach GameThread in <500µs for responsive aim.
 */
static __always_inline bool is_input_handler_name(const char *comm)
{
	/* Unreal Engine GameThread (handles input + game logic) - HIGHEST PRIORITY */
	if (comm[0] == 'G' && comm[1] == 'a' && comm[2] == 'm' && comm[3] == 'e' &&
	    comm[4] == 'T' && comm[5] == 'h' && comm[6] == 'r')
		return true;  /* GameThread - gets 10× boost during input window */

	/* SDL input threads (very common in games) */
	if (comm[0] == 'S' && comm[1] == 'D' && comm[2] == 'L')
		return true;

	/* Input/event processing threads */
	if (comm[0] == 'i' && comm[1] == 'n' && comm[2] == 'p' && comm[3] == 'u' && comm[4] == 't')
		return true;

	if (comm[0] == 'e' && comm[1] == 'v' && comm[2] == 'e' && comm[3] == 'n' && comm[4] == 't')
		return true;

	/* GLFW input (common game library) */
	if (comm[0] == 'g' && comm[1] == 'l' && comm[2] == 'f' && comm[3] == 'w')
		return true;

	/* Qt/GTK input threads (less common in games but possible) */
	if (comm[0] == 'Q' && comm[1] == 't' && comm[2] == 'I' && comm[3] == 'n')
		return true;

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

	return false;
}

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

static __always_inline void classify_input_handler(struct task_struct *p, struct task_ctx *tctx)
{
    if (is_input_handler_name(p->comm)) {
        tctx->is_input_handler = 1;
		tctx->boost_shift = MAX(tctx->boost_shift, 7);
		if (tctx->input_lane == INPUT_LANE_OTHER) {
			if (comm_contains(p->comm, "mouse", 5))
                tctx->input_lane = INPUT_LANE_MOUSE;
			else if (comm_contains(p->comm, "kbd", 3) ||
				 comm_contains(p->comm, "keyboard", 8))
                tctx->input_lane = INPUT_LANE_KEYBOARD;
        }
    }
}

static __always_inline void classify_gpu_submit(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_gpu_submit && is_gpu_submit_name(p->comm))
		tctx->is_gpu_submit = 1;
}

static __always_inline void classify_audio(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_system_audio && is_system_audio_name(p->comm))
		tctx->is_system_audio = 1;
	if (!tctx->is_game_audio && is_game_audio_name(p->comm))
		tctx->is_game_audio = 1;
}

static __always_inline void classify_network(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_network && is_network_name(p->comm))
		tctx->is_network = 1;
}

static __always_inline bool is_background_name(const char *comm)
{
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
	if (!tctx->is_gaming_peripheral && is_gaming_peripheral_thread(p->comm))
		tctx->is_gaming_peripheral = 1;
}

static __always_inline void classify_gaming_traffic(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_gaming_traffic && is_gaming_traffic_pattern(p, tctx))
		tctx->is_gaming_traffic = 1;
}

static __always_inline void classify_audio_pipeline(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_audio_pipeline && is_audio_pipeline_thread(p->comm))
		tctx->is_audio_pipeline = 1;
}

static __always_inline void classify_storage_hot_path(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_storage_hot_path && is_storage_hot_path_thread(p, tctx))
		tctx->is_storage_hot_path = 1;
}

static __always_inline void classify_ethernet_nic_interrupt(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_ethernet_nic_interrupt && is_ethernet_nic_interrupt_thread(p->comm))
		tctx->is_ethernet_nic_interrupt = 1;
}

static __always_inline void classify_background(struct task_struct *p, struct task_ctx *tctx)
{
	if (!tctx->is_background && is_background_name(p->comm))
		tctx->is_background = 1;
}

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
    classify_background(p, tctx);

    if (!tctx->input_lane)
        tctx->input_lane = INPUT_LANE_OTHER;
 }

#endif /* __GAMER_TASK_CLASS_BPF_H */
