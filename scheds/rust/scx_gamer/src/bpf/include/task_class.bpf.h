/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Thread Classification
 * Copyright (c) 2025 Andrea Righi <arighi@nvidia.com>
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
 * Examples: vkd3d-swapchain, dxvk-submit, RenderThread 0
 */
static __always_inline bool is_gpu_submit_name(const char *comm)
{
	/* DXVK (DX9/10/11 → Vulkan, very common with Proton) */
	if (comm[0] == 'd' && comm[1] == 'x' && comm[2] == 'v' && comm[3] == 'k' && comm[4] == '-')
		return true;

	/* Unreal Engine render thread */
	if (comm[0] == 'R' && comm[1] == 'e' && comm[2] == 'n' && comm[3] == 'd' &&
	    comm[4] == 'e' && comm[5] == 'r' && comm[6] == 'T')
		return true;

	/* vkd3d (Vulkan/D3D12 for Proton) */
	if (comm[0] == 'v' && comm[1] == 'k' && comm[2] == 'd' && comm[3] == '3')
		return true;  /* vkd3d_queue, vkd3d_fence, vkd3d-swapchain */

	/* Bracketed Vulkan threads (WoW, etc.) */
	if (comm[0] == '[' && comm[1] == 'v' && comm[2] == 'k')
		return true;  /* [vkrt] Analysis, [vkps] Update */

	/* Mesa GPU threads */
	if (comm[0] == 'g' && comm[1] == 'p' && comm[2] == 'u')
		return true;

	/* RADV Vulkan threads */
	if (comm[0] == 'r' && comm[1] == 'a' && comm[2] == 'd' && comm[3] == 'v')
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
 * Examples: WebSocketClient, UdpSocket, netcode
 */
static __always_inline bool is_network_name(const char *comm)
{
	/* Unreal Engine network threads */
	if (comm[0] == 'W' && comm[1] == 'e' && comm[2] == 'b' && comm[3] == 'S' &&
	    comm[4] == 'o' && comm[5] == 'c' && comm[6] == 'k')
		return true;  /* WebSocketClient */

	if (comm[0] == 'U' && comm[1] == 'd' && comm[2] == 'p' && comm[3] == 'S')
		return true;  /* UdpSocket */

	if (comm[0] == 'R' && comm[1] == 't' && comm[2] == 'c')
		return true;  /* RtcWorkerThread */

	if (comm[0] == 'H' && comm[1] == 't' && comm[2] == 't' && comm[3] == 'p' &&
	    comm[4] == 'M' && comm[5] == 'a' && comm[6] == 'n')
		return true;  /* HttpManagerThre */

	if (comm[0] == 'I' && comm[1] == 'o' && comm[2] == 'S')
		return true;  /* IoService */

	/* Generic patterns */
	if (comm[0] == 'n' && comm[1] == 'e' && comm[2] == 't')
		return true;  /* network, netcode */

	if (comm[0] == 'N' && comm[1] == 'e' && comm[2] == 't')
		return true;  /* NetThread (WoW) */

	if (comm[0] == 'r' && comm[1] == 'e' && comm[2] == 'c' && comm[3] == 'v')
		return true;

	if (comm[0] == 's' && comm[1] == 'e' && comm[2] == 'n' && comm[3] == 'd')
		return true;

	return false;
}

/*
 * System audio threads (PipeWire/PulseAudio/ALSA)
 * Examples: pipewire, pw-*, pulseaudio
 */
static __always_inline bool is_system_audio_name(const char *comm)
{
	/* PipeWire */
	if (comm[0] == 'p' && comm[1] == 'i' && comm[2] == 'p' && comm[3] == 'e')
		return true;

	if (comm[0] == 'p' && comm[1] == 'w' && comm[2] == '-')
		return true;  /* pw-* threads */

	/* PulseAudio */
	if (comm[0] == 'p' && comm[1] == 'u' && comm[2] == 'l' && comm[3] == 's')
		return true;

	/* ALSA */
	if (comm[0] == 'a' && comm[1] == 'l' && comm[2] == 's' && comm[3] == 'a')
		return true;

	return false;
}

/*
 * Game audio threads (lower priority than system audio)
 * Examples: AudioThread, FMODThread
 */
static __always_inline bool is_game_audio_name(const char *comm)
{
	/* Skip system audio threads */
	if (is_system_audio_name(comm))
		return false;

	if (comm[0] == 'A' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'i' &&
	    comm[4] == 'o')
		return true;  /* AudioThread, AudioMixer */

	if (comm[0] == 'F' && comm[1] == 'M' && comm[2] == 'O' && comm[3] == 'D')
		return true;  /* FMODThread */

	if (comm[0] == 's' && comm[1] == 'o' && comm[2] == 'u' && comm[3] == 'n' &&
	    comm[4] == 'd')
		return true;

	return false;
}

/*
 * Input handler threads - HIGHEST priority for gaming
 * Examples: InputThread, EventHandler
 */
static __always_inline bool is_input_handler_name(const char *comm)
{
	if (comm[0] == 'I' && comm[1] == 'n' && comm[2] == 'p' && comm[3] == 'u' &&
	    comm[4] == 't')
		return true;  /* InputThread */

	if (comm[0] == 'E' && comm[1] == 'v' && comm[2] == 'e' && comm[3] == 'n' &&
	    comm[4] == 't')
		return true;  /* EventHandler */

	return false;
}

#endif /* __GAMER_TASK_CLASS_BPF_H */
