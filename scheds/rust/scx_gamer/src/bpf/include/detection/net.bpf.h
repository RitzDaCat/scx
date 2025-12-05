/* SPDX-License-Identifier: GPL-2.0 */
/*
 * net.bpf.h - Network thread detection hooks
 *
 * Detects game network threads via fentry hooks on:
 * - udp_recvmsg(): UDP receive (common for games)
 * - udp_sendmsg(): UDP send
 * - tcp_recvmsg(): TCP receive
 * - tcp_sendmsg(): TCP send
 *
 * Sets FLAG_NETWORK on task_ctx for foreground game threads.
 *
 * Priority: MEDIUM (boost_shift varies based on game context)
 */

#ifndef __DETECTION_NET_BPF_H
#define __DETECTION_NET_BPF_H

/* TODO: Implement in Phase 3
 *
 * Hooks to implement:
 * - SEC("fentry/udp_recvmsg") - Game UDP receive
 * - SEC("fentry/udp_sendmsg") - Game UDP send
 */

#endif /* __DETECTION_NET_BPF_H */

