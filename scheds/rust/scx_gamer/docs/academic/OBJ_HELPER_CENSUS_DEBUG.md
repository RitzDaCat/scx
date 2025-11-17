# Helper Call Census (debug build)

## Aggregate helper usage
| Helper | Calls |
| --- | ---: |
| ktime_get_ns | 90 |
| map_lookup_elem | 80 |
| scx_bpf_now | 56 |
| map_lookup_percpu_elem | 44 |
| probe_read_kernel | 41 |
| map_update_elem | 41 |
| get_current_pid_tgid | 26 |
| task_storage_get | 25 |
| scx_bpf_test_and_clear_cpu_idle | 14 |
| get_current_task_btf | 12 |
| map_delete_elem | 12 |
| ringbuf_reserve | 11 |
| scx_bpf_dsq_insert | 11 |
| scx_bpf_dispatch___compat | 11 |
| bpf_cpumask_test_cpu | 11 |
| scx_bpf_task_cpu | 11 |
| scx_bpf_cpu_node | 11 |
| ringbuf_submit | 10 |
| get_smp_processor_id | 9 |
| scx_bpf_error_bstr | 9 |
| scx_bpf_kick_cpu | 8 |
| probe_read_kernel_str | 7 |
| scx_bpf_dsq_nr_queued | 6 |
| .text | 6 |
| bpf_iter_num_next | 5 |
| bpf_cpumask_weight | 4 |
| scx_bpf_cpu_rq | 4 |
| bpf_iter_num_destroy | 4 |
| bpf_cpumask_first | 3 |
| scx_bpf_put_cpumask | 3 |
| bpf_iter_num_new | 3 |
| scx_bpf_select_cpu_and | 2 |
| scx_bpf_get_idle_smtmask | 2 |
| bpf_cpumask_empty | 2 |
| scx_bpf_get_idle_smtmask_node | 2 |
| timer_start | 2 |
| probe_read_user | 2 |
| scx_bpf_nr_node_ids | 2 |
| scx_bpf_create_dsq | 2 |
| helper_id_-0x261 | 1 |
| helper_id_0x2df | 1 |
| helper_id_0x294 | 1 |
| helper_id_0x222 | 1 |
| helper_id_-0x3b5 | 1 |
| helper_id_0x17b | 1 |
| helper_id_0xfa | 1 |
| helper_id_0xea | 1 |
| scx_bpf_select_cpu_dfl | 1 |
| helper_id_-0x77f | 1 |
| helper_id_-0x7c1 | 1 |
| helper_id_-0x800 | 1 |
| helper_id_-0x83f | 1 |
| helper_id_-0x893 | 1 |
| helper_id_-0x494 | 1 |
| helper_id_-0x567 | 1 |
| helper_id_0x200 | 1 |
| scx_bpf_task_running | 1 |
| helper_id_-0x5a1 | 1 |
| helper_id_0x17d | 1 |
| helper_id_-0x623 | 1 |
| helper_id_0x11c | 1 |
| helper_id_-0x680 | 1 |
| skb_cgroup_id | 1 |
| helper_id_-0x757 | 1 |
| scx_bpf_dsq_insert_vtime | 1 |
| scx_bpf_dispatch_vtime___compat | 1 |
| scx_bpf_get_idle_cpumask | 1 |
| scx_bpf_put_idle_cpumask | 1 |
| bpf_cpumask_create | 1 |
| kptr_xchg | 1 |
| bpf_cpumask_release | 1 |
| bpf_rcu_read_lock | 1 |
| bpf_cpumask_set_cpu | 1 |
| bpf_rcu_read_unlock | 1 |
| tail_call | 1 |
| scx_bpf_dsq_move_to_local | 1 |
| scx_bpf_consume___compat | 1 |
| scx_bpf_reenqueue_local | 1 |
| scx_bpf_cpuperf_set | 1 |
| scx_bpf_nr_cpu_ids | 1 |
| timer_init | 1 |
| timer_set_callback | 1 |
| trace_printk | 1 |
| timer_cancel | 1 |

## Top functions by helper calls
### gamer_select_cpu_slowpath (63 calls)
| Helper | Calls |
| --- | ---: |
| task_storage_get | 8 |
| scx_bpf_test_and_clear_cpu_idle | 7 |
| scx_bpf_dsq_insert | 6 |
| scx_bpf_dispatch___compat | 6 |
| map_lookup_percpu_elem | 6 |
| scx_bpf_now | 4 |
| ktime_get_ns | 4 |
| bpf_cpumask_test_cpu | 4 |

### wakeup_timerfn (63 calls)
| Helper | Calls |
| --- | ---: |
| map_lookup_percpu_elem | 19 |
| scx_bpf_now | 8 |
| ktime_get_ns | 8 |
| scx_bpf_error_bstr | 5 |
| scx_bpf_dsq_nr_queued | 4 |
| scx_bpf_cpu_rq | 4 |
| scx_bpf_kick_cpu | 4 |
| bpf_iter_num_next | 2 |

### gamer_enqueue_slowpath (59 calls)
| Helper | Calls |
| --- | ---: |
| map_lookup_percpu_elem | 6 |
| scx_bpf_dsq_insert | 5 |
| scx_bpf_dispatch___compat | 5 |
| task_storage_get | 3 |
| scx_bpf_now | 3 |
| ktime_get_ns | 3 |
| get_current_task_btf | 3 |
| scx_bpf_kick_cpu | 3 |

### gamer_stopping (56 calls)
| Helper | Calls |
| --- | ---: |
| scx_bpf_now | 15 |
| ktime_get_ns | 15 |
| map_lookup_elem | 9 |
| ringbuf_reserve | 4 |
| ringbuf_submit | 4 |
| map_update_elem | 4 |
| task_storage_get | 2 |
| get_smp_processor_id | 1 |

### gamer_runnable (50 calls)
| Helper | Calls |
| --- | ---: |
| map_lookup_elem | 20 |
| scx_bpf_now | 8 |
| ktime_get_ns | 8 |
| probe_read_kernel | 6 |
| task_storage_get | 2 |
| map_update_elem | 2 |
| scx_bpf_task_cpu | 1 |
| map_lookup_percpu_elem | 1 |

### pick_idle_cpu_cached (28 calls)
| Helper | Calls |
| --- | ---: |
| scx_bpf_test_and_clear_cpu_idle | 7 |
| bpf_cpumask_test_cpu | 6 |
| scx_bpf_cpu_node | 6 |
| scx_bpf_select_cpu_and | 2 |
| task_storage_get | 1 |
| scx_bpf_select_cpu_dfl | 1 |
| helper_id_-0x77f | 1 |
| helper_id_-0x7c1 | 1 |

### gamer_init (24 calls)
| Helper | Calls |
| --- | ---: |
| scx_bpf_error_bstr | 4 |
| bpf_iter_num_next | 3 |
| bpf_iter_num_destroy | 3 |
| bpf_iter_num_new | 2 |
| map_lookup_percpu_elem | 2 |
| scx_bpf_nr_node_ids | 2 |
| scx_bpf_create_dsq | 2 |
| scx_bpf_nr_cpu_ids | 1 |

### detect_audio_submit (19 calls)
| Helper | Calls |
| --- | ---: |
| probe_read_kernel | 7 |
| map_update_elem | 3 |
| map_delete_elem | 2 |
| scx_bpf_now | 2 |
| ktime_get_ns | 2 |
| get_current_task_btf | 1 |
| task_storage_get | 1 |
| map_lookup_elem | 1 |

### input_event_raw (19 calls)
| Helper | Calls |
| --- | ---: |
| map_lookup_elem | 8 |
| get_smp_processor_id | 3 |
| probe_read_kernel | 2 |
| scx_bpf_now | 1 |
| ktime_get_ns | 1 |
| .text | 1 |
| ringbuf_reserve | 1 |
| ringbuf_submit | 1 |

### gamer_dispatch (19 calls)
| Helper | Calls |
| --- | ---: |
| map_lookup_percpu_elem | 3 |
| scx_bpf_cpu_node | 3 |
| scx_bpf_dsq_nr_queued | 2 |
| scx_bpf_dsq_move_to_local | 1 |
| scx_bpf_get_idle_smtmask | 1 |
| bpf_cpumask_empty | 1 |
| scx_bpf_put_cpumask | 1 |
| .text | 1 |

### track_thread_runtime (16 calls)
| Helper | Calls |
| --- | ---: |
| map_lookup_elem | 7 |
| ktime_get_ns | 4 |
| probe_read_kernel | 2 |
| map_update_elem | 2 |
| get_current_pid_tgid | 1 |

### tp_sys_enter_futex (15 calls)
| Helper | Calls |
| --- | ---: |
| probe_read_kernel | 4 |
| map_lookup_elem | 2 |
| task_storage_get | 2 |
| get_current_task_btf | 1 |
| map_delete_elem | 1 |
| scx_bpf_now | 1 |
| ktime_get_ns | 1 |
| get_smp_processor_id | 1 |

### affinity_detect_set_cpus_allowed_ptr (14 calls)
| Helper | Calls |
| --- | ---: |
| probe_read_kernel | 7 |
| ktime_get_ns | 2 |
| map_lookup_elem | 1 |
| map_delete_elem | 1 |
| ringbuf_reserve | 1 |
| probe_read_kernel_str | 1 |
| ringbuf_submit | 1 |

### gamer_init_task (12 calls)
| Helper | Calls |
| --- | ---: |
| probe_read_kernel | 8 |
| task_storage_get | 1 |
| map_lookup_elem | 1 |
| scx_bpf_now | 1 |
| ktime_get_ns | 1 |

### task_dl_with_ctx_cached (10 calls)
| Helper | Calls |
| --- | ---: |
| scx_bpf_now | 4 |
| ktime_get_ns | 4 |
| scx_bpf_task_cpu | 1 |
| map_lookup_percpu_elem | 1 |

