// Copyright (c) Andrea Righi <arighi@nvidia.com>
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2.

fn main() {
    // Check if profiling should be enabled (via SCX_GAMER_ENABLE_PROFILING env var)
    // This is set by build.sh when building debug+profiling
    let enable_profiling = std::env::var("SCX_GAMER_ENABLE_PROFILING").is_ok();
    
    // Create profiling_config.h in the source directory (alongside main.bpf.c)
    // This ensures BPF compiler can find it since it compiles from source directory context
    // Use CARGO_MANIFEST_DIR to get the crate root directory
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR not set");
    let config_header_path = std::path::Path::new(&manifest_dir)
        .join("src")
        .join("bpf")
        .join("profiling_config.h");
    
    if enable_profiling {
        println!("cargo:warning=Profiling enabled (ENABLE_PROFILING flag set)");
        println!("cargo:warning=This adds ~50-150ns overhead per scheduling decision");
        std::fs::write(&config_header_path, "#define ENABLE_PROFILING\n")
            .expect("Failed to write profiling_config.h");
    } else {
        // Create empty header when profiling is disabled
        std::fs::write(&config_header_path, "/* Profiling disabled */\n")
            .expect("Failed to write profiling_config.h");
    }
    
    // Tell cargo to rerun if env var changes
    println!("cargo:rerun-if-env-changed=SCX_GAMER_ENABLE_PROFILING");
    println!("cargo:rerun-if-changed={}", config_header_path.display());
    
    scx_cargo::BpfBuilder::new()
        .unwrap()
        .enable_intf("src/bpf/intf.h", "bpf_intf.rs")
        .enable_skel("src/bpf/main.bpf.c", "bpf")
        .build()
        .unwrap();
}
