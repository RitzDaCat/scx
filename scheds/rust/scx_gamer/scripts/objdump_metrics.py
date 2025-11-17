#!/usr/bin/env python3
"""
Utilities for summarizing llvm-objdump output generated for scx_gamer.

Example:
    python scripts/objdump_metrics.py \
        --disasm scheds/rust/scx_gamer/docs/academic/OBJ_DISASM_DEBUG.txt \
        --symbols scheds/rust/scx_gamer/docs/academic/OBJ_SYMBOLS_DEBUG.txt \
        --relocs-debug scheds/rust/scx_gamer/docs/academic/OBJ_RELOCS_DEBUG.txt \
        --relocs-release scheds/rust/scx_gamer/docs/academic/OBJ_RELOCS_RELEASE.txt \
        --helper-out scheds/rust/scx_gamer/docs/academic/OBJ_HELPER_CENSUS_DEBUG.md \
        --reloc-out scheds/rust/scx_gamer/docs/academic/OBJ_RELOC_PROFILE.md \
        --map-out scheds/rust/scx_gamer/docs/academic/OBJ_MAP_BSS_HEATMAP.md \
        --hist-out scheds/rust/scx_gamer/docs/academic/OBJ_FUNCTION_HISTOGRAM.md
"""

from __future__ import annotations

import argparse
import re
from collections import Counter, defaultdict
from operator import itemgetter
from pathlib import Path
from typing import Dict, List, Tuple


def load_helper_map() -> Dict[int, str]:
    """Parse /usr/include/linux/bpf.h for helper ID to name mapping."""
    header = Path("/usr/include/linux/bpf.h")
    mapping: Dict[int, str] = {}
    pattern = re.compile(r"FN\(([^,]+),\s*(\d+),")
    for name, num in pattern.findall(header.read_text()):
        mapping[int(num)] = name
    return mapping


def parse_disasm(
    disasm_path: Path, helper_map: Dict[int, str]
) -> Tuple[Counter[str], Dict[str, Counter[str]], Dict[str, Counter[str]]]:
    helper_totals: Counter[str] = Counter()
    per_func: Dict[str, Counter[str]] = defaultdict(Counter)
    symbol_refs: Dict[str, Counter[str]] = defaultdict(Counter)

    func_re = re.compile(r"^[0-9a-f]+\s+<([^>]+)>:\s*$")
    call_re = re.compile(r"\bcall\s+([^\s]+)")

    lines = disasm_path.read_text().splitlines()
    current_func = None
    for idx, raw in enumerate(lines):
        stripped = raw.strip()
        if not stripped:
            continue

        if "R_BPF" in stripped and current_func:
            parts = stripped.split()
            symbol = parts[-1] if parts else ""
            if symbol:
                symbol_refs[symbol][current_func] += 1
            continue

        func_match = func_re.match(stripped)
        if func_match:
            current_func = func_match.group(1)
            continue

        call_match = call_re.search(stripped)
        if not call_match:
            continue

        target = call_match.group(1)
        symbol = None
        if idx + 1 < len(lines) and "R_BPF" in lines[idx + 1]:
            rel_parts = lines[idx + 1].strip().split()
            if rel_parts:
                symbol = rel_parts[-1]

        if symbol:
            helper_name = symbol
        else:
            try:
                imm = int(target, 0)
            except ValueError:
                imm = None
            helper_name = helper_map.get(imm, f"helper_id_{target}") if imm is not None else target

        helper_totals[helper_name] += 1
        if current_func:
            per_func[current_func][helper_name] += 1

    return helper_totals, per_func, symbol_refs


def write_helper_markdown(
    helper_totals: Counter[str], per_func: Dict[str, Counter[str]], out_path: Path
) -> None:
    lines: List[str] = []
    lines.append("# Helper Call Census (debug build)")
    lines.append("")
    lines.append("## Aggregate helper usage")
    lines.append("| Helper | Calls |")
    lines.append("| --- | ---: |")
    for name, count in helper_totals.most_common():
        lines.append(f"| {name} | {count} |")
    lines.append("")
    lines.append("## Top functions by helper calls")
    func_totals = sorted(((func, sum(counter.values())) for func, counter in per_func.items()),
                         key=itemgetter(1), reverse=True)
    for func, total in func_totals[:15]:
        lines.append(f"### {func} ({total} calls)")
        lines.append("| Helper | Calls |")
        lines.append("| --- | ---: |")
        for helper, count in per_func[func].most_common(8):
            lines.append(f"| {helper} | {count} |")
        lines.append("")

    out_path.write_text("\n".join(lines) + "\n")


def parse_relocs(path: Path) -> Tuple[Counter[str], Counter[str]]:
    type_counts: Counter[str] = Counter()
    symbol_counts: Counter[str] = Counter()

    for raw in path.read_text().splitlines():
        line = raw.strip()
        if not line or "R_BPF" not in line:
            continue
        parts = line.split()
        if len(parts) < 3:
            continue
        reloc_type = next((p for p in parts if p.startswith("R_BPF")), None)
        if reloc_type is None:
            continue
        symbol = parts[-1]
        type_counts[reloc_type] += 1
        symbol_counts[symbol] += 1

    return type_counts, symbol_counts


def write_reloc_markdown(
    debug_types: Counter[str],
    debug_symbols: Counter[str],
    release_types: Counter[str],
    release_symbols: Counter[str],
    out_path: Path,
) -> None:
    lines: List[str] = []
    lines.append("# CO-RE Relocation Profile (debug vs release)")
    lines.append("")
    lines.append("## Relocation types")
    lines.append("| Type | Debug | Release |")
    lines.append("| --- | ---: | ---: |")
    for reloc_type in sorted(set(debug_types) | set(release_types)):
        lines.append(
            f"| {reloc_type} | {debug_types.get(reloc_type, 0)} | "
            f"{release_types.get(reloc_type, 0)} |"
        )
    lines.append("")
    lines.append("## Top relocated symbols (debug)")
    lines.append("| Symbol | Count |")
    lines.append("| --- | ---: |")
    for symbol, count in debug_symbols.most_common(25):
        lines.append(f"| {symbol} | {count} |")
    lines.append("")
    lines.append("## Top relocated symbols (release)")
    lines.append("| Symbol | Count |")
    lines.append("| --- | ---: |")
    for symbol, count in release_symbols.most_common(25):
        lines.append(f"| {symbol} | {count} |")
    lines.append("")
    out_path.write_text("\n".join(lines) + "\n")


def build_map_heatmap(symbol_refs: Dict[str, Counter[str]], out_path: Path) -> None:
    interesting = {
        sym: counts
        for sym, counts in symbol_refs.items()
        if "_map" in sym
        or "_ringbuf" in sym
        or sym.endswith("_ns")
        or sym.endswith("_state")
        or sym in {"hotpath_signals", "power_hint_level", "power_hint_expiry_ns"}
    }

    lines: List[str] = []
    lines.append("# Map / BSS Reference Heatmap")
    lines.append("")
    lines.append("| Symbol | Total refs | Top functions |")
    lines.append("| --- | ---: | --- |")
    for symbol, counts in sorted(interesting.items(), key=lambda item: sum(item[1].values()), reverse=True):
        total = sum(counts.values())
        top_funcs = ", ".join(f"{func} ({cnt})" for func, cnt in counts.most_common(5))
        lines.append(f"| {symbol} | {total} | {top_funcs} |")
    lines.append("")
    out_path.write_text("\n".join(lines) + "\n")


def build_function_histogram(symbols_path: Path, out_path: Path) -> None:
    entries: List[Tuple[str, int]] = []
    for raw in symbols_path.read_text().splitlines():
        parts = raw.split()
        if len(parts) < 6:
            continue
        _, bind, sym_type, section, size_hex, name = parts[:6]
        if sym_type != "F" or section != ".text":
            continue
        try:
            size = int(size_hex, 16)
        except ValueError:
            continue
        entries.append((name, size))

    entries.sort(key=itemgetter(1), reverse=True)
    lines: List[str] = []
    lines.append("# Function Size Histogram (.text)")
    lines.append("")
    lines.append("| Function | Bytes | Approx. insns |")
    lines.append("| --- | ---: | ---: |")
    for name, size in entries:
        lines.append(f"| {name} | {size} | {size // 8} |")
    lines.append("")
    out_path.write_text("\n".join(lines) + "\n")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate objdump-derived metrics.")
    parser.add_argument("--disasm", type=Path, required=True)
    parser.add_argument("--symbols", type=Path, required=True)
    parser.add_argument("--relocs-debug", type=Path, required=True)
    parser.add_argument("--relocs-release", type=Path, required=True)
    parser.add_argument("--helper-out", type=Path, required=True)
    parser.add_argument("--reloc-out", type=Path, required=True)
    parser.add_argument("--map-out", type=Path, required=True)
    parser.add_argument("--hist-out", type=Path, required=True)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    helper_map = load_helper_map()
    helper_totals, per_func, symbol_refs = parse_disasm(args.disasm, helper_map)
    write_helper_markdown(helper_totals, per_func, args.helper_out)

    debug_types, debug_symbols = parse_relocs(args.relocs_debug)
    release_types, release_symbols = parse_relocs(args.relocs_release)
    write_reloc_markdown(debug_types, debug_symbols, release_types, release_symbols, args.reloc_out)

    build_map_heatmap(symbol_refs, args.map_out)
    build_function_histogram(args.symbols, args.hist_out)


if __name__ == "__main__":
    main()