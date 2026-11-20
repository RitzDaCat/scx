#!/usr/bin/env python3
"""Check the deepest BPF-to-BPF call chain of every program in a BPF object.

The verifier refuses a chain deeper than MAX_CALL_FRAMES with "the call stack
of N frames is too deep". The limit is 8 on 6.13 to 6.18 and 16 on 7.2. It counts
every call instruction in the object, the arms it later removes as dead included,
so a probe arm behind an off toggle still counts. linux-6.18.y restarts the count
at a global function; linux-6.13.y and linux-6.16.y count straight through, which
is how this tool counts. A host on a newer kernel loads what those refuse.

    python3 bench/call_depth.py [bpf.bpf.o]

With no path it checks the newest release object under the repo's target directory.
"""

from __future__ import annotations

import argparse
import functools
import re
import struct
import subprocess
import sys
from pathlib import Path

# include/linux/bpf_verifier.h on the oldest kernels upstream CI verifies against.
MAX_CALL_FRAMES = 8
INSN_BYTES = 8
FUNC_SYMBOL = re.compile(r"([0-9a-f]{16}) [lgw ].{5}F \.text\s+[0-9a-f]+ (\S+)")
FUNC_HEADER = re.compile(r"[0-9a-f]{16} <(\S+)>:$")
# BPF_JMP | BPF_CALL with src_reg BPF_PSEUDO_CALL; the last four bytes are imm.
PSEUDO_CALL = re.compile(r"\s+(\d+):\s+85 10 00 00 ((?:[0-9a-f]{2} ){3}[0-9a-f]{2})")
RELOCATION = re.compile(r"\s(R_BPF_\S+)\s+(\S+)")


def objdump(*args):
    return subprocess.run(["llvm-objdump", *args], capture_output=True, text=True,
                          check=True).stdout.splitlines()


def call_graph(obj):
    """Return {function: callees} and the names of the functions in .text."""
    text_at = {int(m[1], 16): m[2] for m in map(FUNC_SYMBOL.match, objdump("-t", obj)) if m}
    graph, section, cur, pending = {}, None, None, None

    def settle():
        """A call with no relocation is relative to itself; it only occurs inside .text."""
        nonlocal pending
        if pending:
            if section != ".text":
                sys.exit(f"{cur}: call without a relocation outside .text")
            graph[cur].add(text_at[(pending[0] + pending[1] + 1) * INSN_BYTES])
        pending = None

    for line in objdump("-dr", obj):
        if line.startswith("Disassembly of section "):
            settle()
            section = line.split()[-1].rstrip(":")
        elif m := FUNC_HEADER.match(line):
            settle()
            cur = m[1]
            graph[cur] = set()
        elif m := RELOCATION.search(line):
            kind, sym = m[1], m[2]
            if kind == "R_BPF_64_64" and (sym == ".text" or sym in text_at.values()):
                sys.exit(f"{cur}: takes the address of {sym}; callback frames are not modelled")
            if pending and kind == "R_BPF_64_32":
                if sym == ".text":
                    sym = text_at[(pending[1] + 1) * INSN_BYTES]
                if sym in text_at.values():
                    graph[cur].add(sym)
                pending = None
        else:
            settle()
            if m := PSEUDO_CALL.match(line):
                imm = struct.unpack("<i", bytes.fromhex(m[2].replace(" ", "")))[0]
                pending = (int(m[1]), imm)
    settle()
    return graph, set(text_at.values())


def newest_release_object():
    build = Path(__file__).resolve().parents[4] / "target" / "release" / "build"
    found = sorted(build.glob("scx_cake-*/out/bpf.bpf.o"), key=lambda p: p.stat().st_mtime)
    if not found:
        sys.exit(f"no scx_cake object under {build}; build first or pass a path")
    return found[-1]


def main():
    cli = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    cli.add_argument("object", type=Path, nargs="?")
    cli.add_argument("--max-frames", type=int, default=MAX_CALL_FRAMES)
    args = cli.parse_args()
    obj = args.object or newest_release_object()
    graph, subprogs = call_graph(str(obj))

    @functools.cache
    def deepest(func):
        """The verifier forbids recursion, so the graph has no cycle."""
        return (func,) + max((deepest(c) for c in graph[func]), key=len, default=())

    chains = sorted((deepest(f) for f in graph if f not in subprogs), key=len, reverse=True)
    print(f"call_depth: {obj}")
    for chain in chains:
        print(f"{len(chain):2d}  {' > '.join(chain)}")
    over = [chain[0] for chain in chains if len(chain) > args.max_frames]
    if over:
        sys.exit(f"call_depth: over {args.max_frames} frames: {', '.join(over)}")
    print(f"call_depth: clean (limit {args.max_frames}, deepest {len(chains[0])})")


if __name__ == "__main__":
    main()
