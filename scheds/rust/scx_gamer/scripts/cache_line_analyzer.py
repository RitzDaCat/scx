#!/usr/bin/env python3
"""
Cache Line Alignment Analyzer for scx_gamer BPF structs

Analyzes struct layouts to detect fields that straddle 64-byte cache line boundaries,
which can cause:
- False sharing (multiple CPUs invalidating each other's caches)
- Memory bus locking
- Significant performance degradation

Usage: python3 cache_line_analyzer.py
"""

import re
from typing import List, Dict, Tuple
from dataclasses import dataclass
from pathlib import Path

CACHE_LINE_SIZE = 64  # bytes

@dataclass
class Field:
    """Represents a field in a struct"""
    name: str
    type: str
    size: int
    offset: int
    bitfield_bits: int = 0  # Non-zero if this is a bitfield
    
    def end_offset(self) -> int:
        """Returns the offset where this field ends"""
        if self.bitfield_bits > 0:
            return self.offset  # Bitfields share the same byte(s)
        return self.offset + self.size
    
    def cache_line(self) -> int:
        """Returns which cache line this field starts in (0-indexed)"""
        return self.offset // CACHE_LINE_SIZE
    
    def end_cache_line(self) -> int:
        """Returns which cache line this field ends in"""
        return (self.end_offset() - 1) // CACHE_LINE_SIZE if self.size > 0 else self.cache_line()
    
    def straddles_cache_line(self) -> bool:
        """Returns True if this field crosses a cache line boundary"""
        return self.cache_line() != self.end_cache_line() and self.size > 0


@dataclass
class Struct:
    """Represents a C struct"""
    name: str
    fields: List[Field]
    is_cache_aligned: bool = False
    file_path: str = ""
    line_number: int = 0
    
    def total_size(self) -> int:
        """Calculate total struct size"""
        if not self.fields:
            return 0
        return max(f.end_offset() for f in self.fields)
    
    def padding_bytes(self) -> int:
        """Calculate total padding bytes"""
        total = self.total_size()
        used = sum(f.size for f in self.fields if f.bitfield_bits == 0)
        return total - used
    
    def is_cache_line_aligned(self) -> bool:
        """Check if struct size is a multiple of cache line size"""
        return self.total_size() % CACHE_LINE_SIZE == 0
    
    def straddling_fields(self) -> List[Field]:
        """Returns list of fields that straddle cache line boundaries"""
        return [f for f in self.fields if f.straddles_cache_line()]


# Type sizes in bytes (assuming 64-bit architecture)
TYPE_SIZES = {
    'u8': 1, 's8': 1, 'char': 1, 'bool': 1,
    'u16': 2, 's16': 2, 'short': 2,
    'u32': 4, 's32': 4, 'int': 4,
    'u64': 8, 's64': 8, 'long': 8,
    '__uint': 4, '__type': 8,  # BPF map types
    'void*': 8, '__kptr*': 8,  # Pointers
}


def get_type_size(type_str: str) -> int:
    """Get size of a type in bytes"""
    type_str = type_str.strip()
    
    # Handle pointers
    if '*' in type_str or 'ptr' in type_str.lower():
        return 8
    
    # Handle arrays
    array_match = re.search(r'\[(\d+)\]', type_str)
    if array_match:
        count = int(array_match.group(1))
        base_type = re.sub(r'\[\d+\]', '', type_str).strip()
        return get_type_size(base_type) * count
    
    # Remove 'volatile', 'const', 'struct', etc.
    type_str = re.sub(r'\b(volatile|const|struct)\b', '', type_str).strip()
    
    # Check known types
    for known_type, size in TYPE_SIZES.items():
        if type_str.startswith(known_type):
            return size
    
    # Default to 8 bytes for unknown types (assume pointer)
    print(f"Warning: Unknown type '{type_str}', assuming 8 bytes")
    return 8


def parse_struct(content: str, struct_name: str, file_path: str = "") -> Struct:
    """Parse a struct from C code and calculate field offsets"""
    
    # Find struct definition
    pattern = rf'struct\s+(?:CACHE_ALIGNED\s+)?{re.escape(struct_name)}\s*\{{'
    match = re.search(pattern, content)
    
    if not match:
        return None
    
    is_cache_aligned = 'CACHE_ALIGNED' in match.group(0)
    
    # Extract struct body
    start = match.end()
    brace_count = 1
    end = start
    
    while brace_count > 0 and end < len(content):
        if content[end] == '{':
            brace_count += 1
        elif content[end] == '}':
            brace_count -= 1
        end += 1
    
    struct_body = content[start:end-1]
    
    # Parse fields
    fields = []
    offset = 0
    bitfield_offset = 0
    current_bitfield_type = None
    
    # Remove comments
    struct_body = re.sub(r'/\*.*?\*/', '', struct_body, flags=re.DOTALL)
    struct_body = re.sub(r'//.*?$', '', struct_body, flags=re.MULTILINE)
    
    # Parse each field
    for line in struct_body.split(';'):
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        
        # Match field: type name OR type name:bits
        field_match = re.match(r'(.+?)\s+(\w+)(?::(\d+))?$', line)
        if not field_match:
            continue
        
        type_str = field_match.group(1).strip()
        name = field_match.group(2)
        bitfield_bits = int(field_match.group(3)) if field_match.group(3) else 0
        
        if bitfield_bits > 0:
            # Bitfield handling
            if current_bitfield_type != type_str:
                # New bitfield group - align to type boundary
                type_size = get_type_size(type_str)
                offset = (offset + type_size - 1) // type_size * type_size
                bitfield_offset = 0
                current_bitfield_type = type_str
            
            field = Field(name, type_str, get_type_size(type_str), offset, bitfield_bits)
            bitfield_offset += bitfield_bits
            
            # If we've filled this type, move to next
            if bitfield_offset >= get_type_size(type_str) * 8:
                offset += get_type_size(type_str)
                bitfield_offset = 0
                current_bitfield_type = None
        else:
            # Regular field - reset bitfield tracking
            if current_bitfield_type:
                offset += get_type_size(current_bitfield_type)
                current_bitfield_type = None
                bitfield_offset = 0
            
            size = get_type_size(type_str)
            
            # Align to natural boundary
            if size > 1:
                offset = (offset + size - 1) // size * size
            
            field = Field(name, type_str, size, offset)
            offset += size
        
        fields.append(field)
    
    # Handle trailing bitfield
    if current_bitfield_type:
        offset += get_type_size(current_bitfield_type)
    
    # Final struct alignment (align to largest field, max 8 bytes)
    max_align = min(8, max((f.size for f in fields), default=1))
    total_size = (offset + max_align - 1) // max_align * max_align
    
    # Add padding field if needed
    if total_size > offset:
        fields.append(Field(f"_implicit_padding", "u8", total_size - offset, offset))
    
    return Struct(struct_name, fields, is_cache_aligned, file_path)


def analyze_struct(struct: Struct) -> None:
    """Analyze and print cache line alignment for a struct"""
    
    print(f"\n{'='*80}")
    print(f"Struct: {struct.name}")
    print(f"{'='*80}")
    print(f"File: {struct.file_path}")
    print(f"Cache-aligned attribute: {'✓ YES' if struct.is_cache_aligned else '✗ NO'}")
    print(f"Total size: {struct.total_size()} bytes ({struct.total_size() // CACHE_LINE_SIZE} cache lines + {struct.total_size() % CACHE_LINE_SIZE} bytes)")
    print(f"Padding: {struct.padding_bytes()} bytes")
    print(f"Multiple of {CACHE_LINE_SIZE} bytes: {'✓ YES' if struct.is_cache_line_aligned() else '✗ NO'}")
    
    # Check for straddling fields
    straddling = struct.straddling_fields()
    if straddling:
        print(f"\n⚠️  WARNING: {len(straddling)} field(s) straddle cache line boundaries!")
        for field in straddling:
            print(f"  - {field.name}: bytes {field.offset}-{field.end_offset()-1} "
                  f"(cache lines {field.cache_line()}-{field.end_cache_line()})")
    else:
        print(f"\n✓ No fields straddle cache line boundaries")
    
    # Print cache line layout
    print(f"\nCache Line Layout:")
    print(f"-" * 80)
    
    current_line = 0
    for field in struct.fields:
        # Print cache line header if we've moved to a new line
        if field.cache_line() > current_line:
            for line_num in range(current_line + 1, field.cache_line() + 1):
                print(f"\n--- Cache Line {line_num} ({line_num * CACHE_LINE_SIZE}-{(line_num + 1) * CACHE_LINE_SIZE - 1} bytes) ---")
            current_line = field.cache_line()
        elif field.offset == 0:
            print(f"\n--- Cache Line 0 (0-{CACHE_LINE_SIZE - 1} bytes) ---")
        
        # Print field info
        straddling_marker = " ⚠️  STRADDLES" if field.straddles_cache_line() else ""
        bitfield_info = f":{field.bitfield_bits}" if field.bitfield_bits > 0 else ""
        
        print(f"  [{field.offset:4d}] {field.type:20s} {field.name:30s} "
              f"({field.size} bytes){bitfield_info}{straddling_marker}")


def find_all_structs(base_path: Path) -> List[Tuple[str, str, str]]:
    """Find all struct definitions in the codebase"""
    structs = []
    
    for c_file in base_path.rglob('*.h'):
        content = c_file.read_text()
        
        # Find all struct names
        for match in re.finditer(r'struct\s+(?:CACHE_ALIGNED\s+)?(\w+)\s*\{', content):
            struct_name = match.group(1)
            structs.append((struct_name, content, str(c_file)))
    
    return structs


def main():
    """Main analysis function"""
    base_path = Path(__file__).parent.parent / 'src' / 'bpf'
    
    print(f"Cache Line Alignment Analyzer")
    print(f"Cache line size: {CACHE_LINE_SIZE} bytes")
    print(f"Analyzing structs in: {base_path}")
    
    # Priority structs (hot path)
    priority_structs = ['task_ctx', 'cpu_ctx', 'hot_path_cache']
    
    # Find all structs
    all_structs = find_all_structs(base_path)
    
    # Analyze priority structs first
    print(f"\n{'#'*80}")
    print(f"# PRIORITY STRUCTS (Hot Path)")
    print(f"{'#'*80}")
    
    for struct_name in priority_structs:
        for name, content, file_path in all_structs:
            if name == struct_name:
                struct = parse_struct(content, struct_name, file_path)
                if struct:
                    analyze_struct(struct)
                break
    
    # Analyze all other structs
    print(f"\n\n{'#'*80}")
    print(f"# ALL OTHER STRUCTS")
    print(f"{'#'*80}")
    
    analyzed = set(priority_structs)
    for struct_name, content, file_path in all_structs:
        if struct_name not in analyzed and not struct_name.startswith('__'):
            struct = parse_struct(content, struct_name, file_path)
            if struct:
                analyze_struct(struct)
            analyzed.add(struct_name)


if __name__ == '__main__':
    main()

