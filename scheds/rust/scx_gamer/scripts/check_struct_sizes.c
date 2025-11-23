/* Quick struct size checker */
#include <stdio.h>
#include "../src/bpf/include/types.bpf.h"

int main() {
    printf("=== Actual Compiled Struct Sizes ===\n\n");
    
    printf("task_ctx:\n");
    printf("  Size: %zu bytes\n", sizeof(struct task_ctx));
    printf("  Cache lines: %zu full + %zu bytes\n", 
           sizeof(struct task_ctx) / 64, 
           sizeof(struct task_ctx) % 64);
    printf("  Aligned to 64 bytes: %s\n\n", 
           (sizeof(struct task_ctx) % 64 == 0) ? "YES" : "NO");
    
    printf("cpu_ctx:\n");
    printf("  Size: %zu bytes\n", sizeof(struct cpu_ctx));
    printf("  Cache lines: %zu full + %zu bytes\n", 
           sizeof(struct cpu_ctx) / 64, 
           sizeof(struct cpu_ctx) % 64);
    printf("  Aligned to 64 bytes: %s\n\n", 
           (sizeof(struct cpu_ctx) % 64 == 0) ? "YES" : "NO");
    
    printf("hot_path_cache:\n");
    printf("  Size: %zu bytes\n", sizeof(struct hot_path_cache));
    printf("  Cache lines: %zu full + %zu bytes\n", 
           sizeof(struct hot_path_cache) / 64, 
           sizeof(struct hot_path_cache) % 64);
    printf("  Aligned to 64 bytes: %s\n\n", 
           (sizeof(struct hot_path_cache) % 64 == 0) ? "YES" : "NO");
    
    return 0;
}

