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
 * Game audio threads (lower priority than system audio)
 * Game audio is important for immersion but shouldn't delay input processing.
 * Examples: AudioDeviceBuff, FMODThread, AudioEncoder, OpenAL
 */
static __always_inline bool is_game_audio_name(const char *comm)
{
	/* Unreal Engine audio threads */
	if (comm[0] == 'A' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'i' &&
	    comm[4] == 'o' && comm[5] == 'D' && comm[6] == 'e' && comm[7] == 'v')
		return true;  /* AudioDeviceBuff */

	if (comm[0] == 'A' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'i' &&
	    comm[4] == 'o' && comm[5] == 'E' && comm[6] == 'n' && comm[7] == 'c')
		return true;  /* AudioEncoder */

	if (comm[0] == 'F' && comm[1] == 'A' && comm[2] == 'u' && comm[3] == 'd')
		return true;  /* FAudio_AudioCli */

	/* Bink audio (common video codec in games) */
	if (comm[0] == 'B' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'k' &&
	    comm[4] == ' ' && comm[5] == 'S' && comm[6] == 'n' && comm[7] == 'd')
		return true;  /* Bink Snd */

	/* Generic game audio threads: "audio", "sound", "snd_" */
	if (comm[0] == 'a' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'i')
		return true;

	if (comm[0] == 's' && comm[1] == 'o' && comm[2] == 'u' && comm[3] == 'n')
		return true;

	if (comm[0] == 's' && comm[1] == 'n' && comm[2] == 'd' && comm[3] == '_')
		return true;

	/* OpenAL (common game audio library) */
	if (comm[0] == 'o' && comm[1] == 'p' && comm[2] == 'e' && comm[3] == 'n' &&
	    comm[4] == 'a' && comm[5] == 'l')
		return true;

	/* FMOD (game audio engine) */
	if (comm[0] == 'f' && comm[1] == 'm' && comm[2] == 'o' && comm[3] == 'd')
		return true;

	/* Wwise (game audio engine) */
	if (comm[0] == 'w' && comm[1] == 'w' && comm[2] == 'i' && comm[3] == 's')
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
	if (comm[0] == 'i' && comm[1] == 'n' && comm[2] == 'p' && comm[3] == 'u')
		return true;

	if (comm[0] == 'e' && comm[1] == 'v' && comm[2] == 'e' && comm[3] == 'n')
		return true;

	/* GLFW input (common game library) */
	if (comm[0] == 'g' && comm[1] == 'l' && comm[2] == 'f' && comm[3] == 'w')
		return true;

	/* Qt/GTK input threads (less common in games but possible) */
	if (comm[0] == 'Q' && comm[1] == 't' && comm[2] == 'I' && comm[3] == 'n')
		return true;

	/* Wine XInput controller handling (critical for gamepad input latency) */
	if (comm[0] == 'w' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'e' &&
	    comm[4] == '_' && comm[5] == 'x' && comm[6] == 'i' && comm[7] == 'n')
		return true;  /* wine_xinput_hid */

	/* Wine Windows Gaming Input (WGI) worker threads - critical for Sea of Thieves input */
	if (comm[0] == 'w' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'e' &&
	    comm[4] == '_' && comm[5] == 'w' && comm[6] == 'g')
		return true;  /* wine_wginput_worker, wine_wginput_wo, etc. */

	/* Wine DirectInput worker threads - critical for Warframe and legacy games
	 * DirectInput is the legacy Windows input API (pre-XInput/WGI) used by:
	 * - Older games (pre-2010): CS:GO, TF2, Portal 2, L4D2
	 * - Games prioritizing mouse precision: Warframe, Counter-Strike series
	 * - Games with complex input configurations: MMOs, simulators
	 * - Games that need raw input data without XInput abstraction
	 */
	if (comm[0] == 'w' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'e' &&
	    comm[4] == '_' && comm[5] == 'd' && comm[6] == 'i' && comm[7] == 'n')
		return true;  /* wine_dinput_worker, wine_dinput_wor, etc. */

	/* Wine raw input threads (Windows Raw Input API)
	 * Used by competitive FPS games for unfiltered mouse input */
	if (comm[0] == 'w' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'e' &&
	    comm[4] == '_' && comm[5] == 'r' && comm[6] == 'a' && comm[7] == 'w')
		return true;  /* wine_rawinput_*, wine_raw_*, etc. */

	return false;
}

#endif /* __GAMER_TASK_CLASS_BPF_H */
