#!/usr/bin/env bash
set -euo pipefail

cargo build -p scx_gamer --release
sudo env SCX_OPT="${SCX_OPT:-}" target/release/scx_gamer "$@"
