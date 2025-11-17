# CO-RE Relocation Profile (debug vs release)

## Relocation types
| Type | Debug | Release |
| --- | ---: | ---: |
| R_BPF_64_32 | 208 | 208 |
| R_BPF_64_64 | 1386 | 1386 |
| R_BPF_64_ABS64 | 16 | 16 |

## Top relocated symbols (debug)
| Symbol | Count |
| --- | ---: |
| scx_bpf_now | 112 |
| detected_fg_tgid | 65 |
| power_hint_level | 46 |
| power_hint_remaining_ns | 44 |
| cpu_ctx_stor | 41 |
| foreground_tgid | 32 |
| no_stats | 32 |
| scx_bpf_dsq_insert | 27 |
| task_ctx_stor | 25 |
| power_hint_expiry_ns | 23 |
| nr_cpu_ids | 23 |
| scx_bpf_cpu_node | 22 |
| nr_boost_shift_0 | 21 |
| nr_boost_shift_1 | 19 |
| nr_boost_shift_5 | 19 |
| nr_boost_shift_3 | 19 |
| nr_boost_shift_7 | 19 |
| nr_boost_shift_4 | 19 |
| nr_boost_shift_2 | 19 |
| nr_boost_shift_6 | 19 |
| input_until_global | 17 |
| input_trigger_rate | 17 |
| nr_nvme_io_threads | 16 |
| interrupt_threads_map | 16 |
| numa_enabled | 15 |

## Top relocated symbols (release)
| Symbol | Count |
| --- | ---: |
| scx_bpf_now | 112 |
| detected_fg_tgid | 65 |
| power_hint_level | 46 |
| power_hint_remaining_ns | 44 |
| cpu_ctx_stor | 41 |
| foreground_tgid | 32 |
| no_stats | 32 |
| scx_bpf_dsq_insert | 27 |
| task_ctx_stor | 25 |
| power_hint_expiry_ns | 23 |
| nr_cpu_ids | 23 |
| scx_bpf_cpu_node | 22 |
| nr_boost_shift_0 | 21 |
| nr_boost_shift_1 | 19 |
| nr_boost_shift_5 | 19 |
| nr_boost_shift_3 | 19 |
| nr_boost_shift_7 | 19 |
| nr_boost_shift_4 | 19 |
| nr_boost_shift_2 | 19 |
| nr_boost_shift_6 | 19 |
| input_until_global | 17 |
| input_trigger_rate | 17 |
| nr_nvme_io_threads | 16 |
| interrupt_threads_map | 16 |
| numa_enabled | 15 |

