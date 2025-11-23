/* Struct layout test for pahole analysis */
#define __BPF__
#include <stdint.h>
#include <stdbool.h>

/* BPF type definitions needed for compilation */
typedef uint8_t u8;
typedef int8_t s8;
typedef uint16_t u16;
typedef int16_t s16;
typedef uint32_t u32;
typedef int32_t s32;
typedef uint64_t u64;
typedef int64_t s64;

/* Mock BPF definitions */
#define SEC(name) __attribute__((section(name), used))
#define __uint(name, val) int (*name)[val]
#define __type(name, val) typeof(val) *name
#define NSEC_PER_MSEC 1000000ULL
#define NSEC_PER_USEC 1000ULL
#define SCX_CPUPERF_ONE 1024

/* Include the actual header */
#include "../src/bpf/intf.h"
#include "../src/bpf/include/types.bpf.h"

/* Dummy main to make this compilable */
int main(void) {
    struct task_ctx t;
    struct cpu_ctx c;
    struct hot_path_cache h;
    
    (void)t;
    (void)c;
    (void)h;
    
    return 0;
}

