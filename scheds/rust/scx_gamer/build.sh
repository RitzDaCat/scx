#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

echo "================================================================================"
echo "                         BUILDING SCX_GAMER"
echo "================================================================================"
echo
echo "Repository root: ${REPO_ROOT}"
echo "Cleaning and building release binary..."
echo

cd "${REPO_ROOT}"

echo "Step 1: Removing target/ directory..."
rm -rf target/

echo "Step 2: Running cargo clean..."
cargo clean

echo "Step 3: Building scx_gamer (release)..."
cargo build -p scx_gamer --release

echo
echo "================================================================================"
echo "                         BUILD COMPLETE"
echo "================================================================================"
echo
echo "Binary location: ${REPO_ROOT}/target/release/scx_gamer"
echo

