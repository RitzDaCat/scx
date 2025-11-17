# Map / BSS Reference Heatmap

| Symbol | Total refs | Top functions |
| --- | ---: | --- |
| power_hint_level | 46 | gamer_stopping (18), gamer_runnable (10), gamer_select_cpu_slowpath (4), task_dl_with_ctx_cached (4), detect_audio_submit (4) |
| power_hint_remaining_ns | 44 | gamer_stopping (18), gamer_runnable (10), task_dl_with_ctx_cached (4), detect_audio_submit (4), gamer_select_cpu_slowpath (3) |
| power_hint_expiry_ns | 23 | gamer_stopping (9), gamer_runnable (5), gamer_select_cpu_slowpath (2), task_dl_with_ctx_cached (2), detect_audio_submit (2) |
| interrupt_threads_map | 16 | gamer_runnable (4), detect_interrupt_hardware (2), detect_interrupt_hardware_exit (2), detect_interrupt_softirq (2), detect_interrupt_softirq_exit (2) |
| slice_ns | 12 | gamer_select_cpu_slowpath (6), wakeup_timerfn (2), task_slice (1), gamer_enqueue_slowpath (1), gamer_stopping (1) |
| gpu_vendor_by_tgid_map | 11 | gamer_stopping (7), gamer_select_cpu_slowpath (1), wakeup_timerfn (1), detect_gpu_submit_drm (1), detect_gpu_submit_nvidia (1) |
| input_lane_last_trigger_ns | 11 | input_event_raw (7), wakeup_timerfn (3), set_input_lane (1) |
| input_lane_dynamic_ns | 11 | input_event_raw (7), wakeup_timerfn (2), set_input_lane (2) |
| memory_threads_map | 11 | gamer_runnable (3), detect_memory_page_fault (2), detect_memory_mm_fault (2), detect_memory_allocation (2), detect_memory_deallocation (2) |
| input_force_dispatch_latency_ns | 10 | input_event_raw (6), wakeup_timerfn (2), set_input_window (1), set_input_lane (1) |
| input_force_dispatch_latency_max_ns | 10 | input_event_raw (6), wakeup_timerfn (2), set_input_window (1), set_input_lane (1) |
| last_input_trigger_ns | 10 | input_event_raw (4), wakeup_timerfn (3), set_input_window (2), set_input_lane (1) |
| network_threads_map | 10 | detect_network_send (2), detect_network_recv (2), detect_network_tcp_send (2), detect_network_udp_send (2), gamer_runnable (2) |
| filesystem_threads_map | 10 | detect_filesystem_read (2), detect_filesystem_write (2), detect_filesystem_open (2), detect_filesystem_close (2), gamer_runnable (2) |
| storage_threads_map | 9 | detect_storage_block_io (2), detect_storage_nvme_io (2), detect_storage_fs_read (2), gamer_runnable (2), gamer_disable (1) |
| system_busy_state | 8 | is_system_busy (4), gamer_enqueue_slowpath (4) |
| keyboard_boost_ns | 8 | input_event_raw (6), wakeup_timerfn (1), set_input_lane (1) |
| gpu_threads_map | 8 | gamer_stopping (4), detect_gpu_submit_drm (2), detect_gpu_submit_nvidia (2) |
| frame_interval_ns | 6 | gamer_runnable (2), gamer_select_cpu_slowpath (1), task_dl_with_ctx_cached (1), detect_gpu_submit_drm (1), detect_gpu_submit_nvidia (1) |
| mouse_boost_ns | 6 | input_event_raw (4), wakeup_timerfn (1), set_input_lane (1) |
| wakeup_timer_ns | 6 | wakeup_timerfn (4), gamer_init (2) |
| input_window_ns | 6 | wakeup_timerfn (2), input_event_raw (2), set_input_window (1), set_napi_softirq_window (1) |
| game_audio_threads_map | 6 | detect_audio_submit (2), gamer_disable (2), gamer_runnable (1), gamer_running (1) |
| system_audio_tgids_map | 6 | detect_audio_submit (2), gamer_disable (2), gamer_runnable (1), gamer_init_task (1) |
| interrupt_map_full_errors | 6 | detect_interrupt_hardware (1), detect_interrupt_hardware_exit (1), detect_interrupt_softirq (1), detect_interrupt_softirq_exit (1), detect_interrupt_tasklet (1) |
| hotpath_signals | 5 | gamer_enqueue_slowpath (3), input_event_raw (2) |
| graphics_api_map | 5 | gamer_runnable (4), gamer_enqueue_slowpath (1) |
| compositor_threads_map | 5 | detect_compositor_mode_set (2), detect_compositor_plane_set (2), gamer_runnable (1) |
| input_window_dynamic_ns | 4 | input_event_raw (2), wakeup_timerfn (1), set_input_window (1) |
| network_map_full_errors | 4 | detect_network_send (1), detect_network_recv (1), detect_network_tcp_send (1), detect_network_udp_send (1) |
| system_audio_threads_map | 4 | detect_audio_submit (2), gamer_runnable (1), gamer_disable (1) |
| memory_map_full_errors | 4 | detect_memory_page_fault (1), detect_memory_mm_fault (1), detect_memory_allocation (1), detect_memory_deallocation (1) |
| filesystem_map_full_errors | 4 | detect_filesystem_read (1), detect_filesystem_write (1), detect_filesystem_open (1), detect_filesystem_close (1) |
| napi_last_softirq_ns | 3 | pick_idle_cpu_cached (2), track_net_softirq (1) |
| game_threads_map | 3 | track_thread_runtime (3) |
| thread_activity_map | 3 | track_thread_runtime (3) |
| thread_runtime_map | 3 | track_thread_runtime (3) |
| storage_map_full_errors | 3 | detect_storage_block_io (1), detect_storage_nvme_io (1), detect_storage_fs_read (1) |
| input_handler_cpu_map | 3 | input_event_raw (2), gamer_disable (1) |
| engine_profile_map | 3 | gamer_stopping (2), gamer_runnable (1) |
| gpu_submit_detect_ringbuf | 3 | gamer_stopping (3) |
| dispatch_event_ringbuf | 2 | gamer_enqueue_slowpath (1), gamer_dispatch (1) |
| mig_window_ns | 2 | gamer_enqueue_slowpath (2) |
| gpu_map_full_errors | 2 | detect_gpu_submit_drm (1), detect_gpu_submit_nvidia (1) |
| compositor_map_full_errors | 2 | detect_compositor_mode_set (1), detect_compositor_plane_set (1) |
| input_events_ringbuf_0 | 1 | get_distributed_ringbuf_reserve (1) |
| input_events_ringbuf_1 | 1 | get_distributed_ringbuf_reserve (1) |
| input_events_ringbuf_9 | 1 | get_distributed_ringbuf_reserve (1) |
| input_events_ringbuf_5 | 1 | get_distributed_ringbuf_reserve (1) |
| input_events_ringbuf_13 | 1 | get_distributed_ringbuf_reserve (1) |
| input_events_ringbuf_3 | 1 | get_distributed_ringbuf_reserve (1) |
| input_events_ringbuf_11 | 1 | get_distributed_ringbuf_reserve (1) |
| input_events_ringbuf_7 | 1 | get_distributed_ringbuf_reserve (1) |
| input_events_ringbuf_15 | 1 | get_distributed_ringbuf_reserve (1) |
| input_events_ringbuf_8 | 1 | get_distributed_ringbuf_reserve (1) |
| input_events_ringbuf_4 | 1 | get_distributed_ringbuf_reserve (1) |
| input_events_ringbuf_12 | 1 | get_distributed_ringbuf_reserve (1) |
| input_events_ringbuf_2 | 1 | get_distributed_ringbuf_reserve (1) |
| input_events_ringbuf_10 | 1 | get_distributed_ringbuf_reserve (1) |
| input_events_ringbuf_6 | 1 | get_distributed_ringbuf_reserve (1) |
| input_events_ringbuf_14 | 1 | get_distributed_ringbuf_reserve (1) |
| last_page_flip_ns | 1 | task_dl_with_ctx_cached (1) |
| thread_track_map_full | 1 | track_thread_runtime (1) |
| current_game_map | 1 | game_detect_exit (1) |
| raw_input_stats_map | 1 | input_event_raw (1) |
| input_sample_seq_map | 1 | input_event_raw (1) |
| input_events_ringbuf | 1 | input_event_raw (1) |
| tailcall_map | 1 | gamer_select_cpu (1) |
| deadline_miss_ringbuf | 1 | gamer_stopping (1) |
| frame_phase_cpu_ns | 1 | gamer_stopping (1) |
| frame_phase_gpu_ns | 1 | gamer_stopping (1) |

