#!/usr/bin/env python3
"""Analyze a rust-mos linker map file: find biggest symbols per section."""
import re
import sys


def parse_map(path):
    """Parse linker map into a list of (section, symbol_name, vma, size) tuples.

    The map format is:
        VMA      LMA     Size Align Out     In      Symbol

    Section headers have the section name in column 5 (e.g. ".text").
    Input sections are indented 8 spaces and contain file paths.
    Symbols are indented 16 spaces and contain the symbol name.
    Some symbols appear as "name = expr" (linker-defined); we skip those.

    For .noinit static stacks, the map aggregates them into one blob.
    We can infer per-function frame sizes from the .text symbols' companion
    .noinit entries, but the map doesn't break those out. Instead, we look
    for named symbols and input sections with non-zero sizes.
    """
    symbols = []
    current_section = None

    # Regex for section header: non-deeply-indented line with a section name
    # e.g. "     80d      80d     9f94     1 .text"
    section_re = re.compile(
        r"^\s+([0-9a-f]+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s+\d+\s+(\.\S+)\s*$"
    )

    # Regex for input section (file path line, 8-space indent)
    # e.g. "     830      830       26     1         /project/..."
    input_re = re.compile(
        r"^\s+([0-9a-f]+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s+\d+\s+\S+.*\.(o|obj):"
    )

    # Regex for symbol line (16-space indent, symbol name)
    # e.g. "     830      830       26     1                 roguelike_c64::apply_offset"
    symbol_re = re.compile(
        r"^\s+([0-9a-f]+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s+\d+\s{16,}(\S+.*?)$"
    )

    # Regex for linker assignment lines (skip these)
    assign_re = re.compile(r"=")

    with open(path) as f:
        for line in f:
            line = line.rstrip()

            # Check for section header
            sm = section_re.match(line)
            if sm:
                current_section = sm.group(4)
                continue

            if current_section is None:
                continue

            # Check for symbol line (deeper indent)
            sym = symbol_re.match(line)
            if sym:
                name = sym.group(4).strip()
                size = int(sym.group(3), 16)
                vma = int(sym.group(1), 16)
                # Skip linker-defined symbols (contain '=')
                if "=" in name:
                    continue
                if size > 0:
                    symbols.append((current_section, name, vma, size))
                continue

            # Check if we hit a new section (input section line won't change current_section)

    return symbols


def demangle_name(name):
    """Clean up Rust symbol names for readability."""
    # Remove common prefixes
    name = name.replace("roguelike_core::", "core::")
    name = name.replace("roguelike_c64::", "c64::")
    # Shorten core:: Rust paths
    name = re.sub(r"<core::(.*?) as core::.*?>::", r"\1::", name)
    return name


def print_top(symbols, section, n, title):
    """Print top N symbols by size for a given section."""
    filtered = [(s, name, vma, size) for s, name, vma, size in symbols if s == section]
    filtered.sort(key=lambda x: -x[3])
    total = sum(size for _, _, _, size in filtered)

    print(f"\n{'='*80}")
    print(f" {title}")
    print(f" Section total: {total:,} bytes across {len(filtered)} symbols")
    print(f"{'='*80}")
    print(f"{'#':>3}  {'Address':>7}  {'Size':>6}  {'%':>5}  Symbol")
    print(f"{'-'*3}  {'-'*7}  {'-'*6}  {'-'*5}  {'-'*50}")

    top_total = 0
    for i, (_, name, vma, size) in enumerate(filtered[:n]):
        pct = 100.0 * size / total if total > 0 else 0
        top_total += size
        display = demangle_name(name)
        print(f"{i+1:>3}  ${vma:05X}  {size:>6}  {pct:>5.1f}  {display}")

    if n < len(filtered):
        rest = total - top_total
        print(f"     ...      {rest:>6}  {100.0*rest/total:>5.1f}  ({len(filtered)-n} more symbols)")

    print(f"\n     Top {min(n, len(filtered))} = {top_total:,} bytes ({100.0*top_total/total:.1f}% of section)")
    return filtered


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else "map.txt"
    symbols = parse_map(path)

    # Also gather input section entries for .noinit static stack breakdown
    # The static stack is one blob in the map, but we can look for
    # individual static stack symbols in a different way.
    # Let's also parse input sections that have function names in them.
    noinit_funcs = parse_noinit_input_sections(path)

    print("="*80)
    print(" C64 Linker Map Analysis")
    print(f" Source: {path}")
    print(f" Total symbols with size > 0: {len(symbols)}")
    print("="*80)

    # 1. Top 20 .text
    text_syms = print_top(symbols, ".text", 20, "TOP 20 .text SYMBOLS (code)")

    # 2. Top 20 .noinit
    noinit_syms = print_top(symbols, ".noinit", 20, "TOP 20 .noinit SYMBOLS (static stacks + globals)")

    # Show .noinit input section breakdown (named globals + static stack aggregate)
    if noinit_funcs:
        print(f"\n{'='*80}")
        print(f" .noinit INPUT SECTIONS")
        print(f" Named globals + aggregate static stack (map does not break out per-function frames)")
        print(f"{'='*80}")
        noinit_funcs.sort(key=lambda x: -x[2])
        total_ni = sum(s for _, _, s in noinit_funcs)
        print(f"{'#':>3}  {'Size':>6}  {'%':>5}  Section")
        print(f"{'-'*3}  {'-'*6}  {'-'*5}  {'-'*50}")
        for i, (name, vma, size) in enumerate(noinit_funcs):
            pct = 100.0 * size / total_ni if total_ni > 0 else 0
            display = demangle_name(name)
            print(f"{i+1:>3}  {size:>6}  {pct:>5.1f}  {display}")
        print(f"\n     Total .noinit: {total_ni:,} bytes across {len(noinit_funcs)} input sections")

    # 3. Top 10 .rodata
    print_top(symbols, ".rodata", 10, "TOP 10 .rodata SYMBOLS (constants, lookup tables)")

    # Also show input sections for .rodata since many are anonymous
    rodata_inputs = parse_rodata_input_sections(path)
    if rodata_inputs:
        print(f"\n  .rodata INPUT SECTIONS (includes anonymous constants):")
        rodata_inputs.sort(key=lambda x: -x[2])
        total_ri = sum(s for _, _, s in rodata_inputs)
        for i, (name, vma, size) in enumerate(rodata_inputs[:15]):
            pct = 100.0 * size / total_ri if total_ri > 0 else 0
            display = demangle_name(name)
            print(f"  {i+1:>3}  {size:>6}  {pct:>5.1f}  {display}")

    # 4. Top 5 .hiramcode
    print_top(symbols, ".hiramcode", 5, "TOP 5 .hiramcode SYMBOLS (hiram overlay code)")

    # 5. Top 5 .noinit.state
    print_top(symbols, ".noinit.state", 5, "TOP 5 .noinit.state SYMBOLS (game state)")

    # 6. Summary
    text_total = sum(size for s, _, _, size in symbols if s == ".text")
    text_sorted = sorted(
        [(name, size) for s, name, _, size in symbols if s == ".text"],
        key=lambda x: -x[1]
    )
    top10_text = sum(size for _, size in text_sorted[:10])
    top20_text = sum(size for _, size in text_sorted[:20])

    print(f"\n{'='*80}")
    print(f" SUMMARY")
    print(f"{'='*80}")
    print(f"  .text total:      {text_total:>6,} bytes")
    print(f"  Top 10 functions: {top10_text:>6,} bytes ({100.0*top10_text/text_total:.1f}%)")
    print(f"  Top 20 functions: {top20_text:>6,} bytes ({100.0*top20_text/text_total:.1f}%)")
    print(f"  Total functions:  {len(text_sorted)}")
    print()

    noinit_total = sum(size for s, _, _, size in symbols if s == ".noinit")
    noinit_input_total = sum(s for _, _, s in noinit_funcs) if noinit_funcs else 0
    static_stack_size = sum(
        s for name, _, s in noinit_funcs if "Lstatic_stack" in name
    ) if noinit_funcs else 0
    globals_size = noinit_input_total - static_stack_size
    print(f"  .noinit total:    {noinit_input_total:>6,} bytes")
    print(f"    Static stacks:  {static_stack_size:>6,} bytes (aggregate, per-function breakdown requires ELF)")
    print(f"    Globals:        {globals_size:>6,} bytes (SAVE_BUF, DIFF, etc.)")
    print()

    # Code concentration analysis
    print(f"  Code concentration:")
    cumulative = 0
    thresholds = [25, 50, 75, 90]
    thresh_idx = 0
    for i, (name, size) in enumerate(text_sorted):
        cumulative += size
        pct = 100.0 * cumulative / text_total
        while thresh_idx < len(thresholds) and pct >= thresholds[thresh_idx]:
            print(f"    {thresholds[thresh_idx]}% of .text in top {i+1} functions")
            thresh_idx += 1
        if thresh_idx >= len(thresholds):
            break


def parse_noinit_input_sections(path):
    """Parse .noinit input sections to report aggregate static stack size.

    rust-mos merges all per-function static stack frames into a single
    .noinit..Lstatic_stack blob. The linker map does not break out individual
    function frames -- that requires ELF analysis via nm/objdump. We collect
    whatever named input sections exist within .noinit so the caller can
    report them alongside the named globals (SAVE_BUF, DIFF).
    """
    section_re = re.compile(
        r"^\s+([0-9a-f]+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s+\d+\s+(\.\S+)\s*$"
    )
    input_re = re.compile(
        r"^\s+([0-9a-f]+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s+(\d+)\s+\S+.*?\.o:\((.+)\)\s*$"
    )

    current_section = None
    entries = []

    with open(path) as f:
        for line in f:
            line = line.rstrip()
            sm = section_re.match(line)
            if sm:
                current_section = sm.group(4)
                continue

            if current_section != ".noinit":
                continue

            im = input_re.match(line)
            if im:
                vma = int(im.group(1), 16)
                size = int(im.group(3), 16)
                input_section_name = im.group(5)
                if size > 0:
                    name = re.sub(r"^\.noinit\.?", "", input_section_name)
                    if not name:
                        name = "(anonymous)"
                    entries.append((name, vma, size))

    return entries


def parse_rodata_input_sections(path):
    """Parse .rodata input sections to identify anonymous constants."""
    section_re = re.compile(
        r"^\s+([0-9a-f]+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s+\d+\s+(\.\S+)\s*$"
    )
    input_re = re.compile(
        r"^\s+([0-9a-f]+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s+(\d+)\s+\S+.*?\.o:\((.+)\)\s*$"
    )

    current_section = None
    entries = []

    with open(path) as f:
        for line in f:
            line = line.rstrip()
            sm = section_re.match(line)
            if sm:
                current_section = sm.group(4)
                continue

            if current_section != ".rodata":
                continue

            im = input_re.match(line)
            if im:
                vma = int(im.group(1), 16)
                size = int(im.group(3), 16)
                input_section_name = im.group(5)
                if size > 0:
                    # Clean up the section name to get function context
                    name = input_section_name
                    name = re.sub(r"^\.rodata\.?", "", name)
                    if not name:
                        name = "(anonymous)"
                    entries.append((name, vma, size))

    return entries


if __name__ == "__main__":
    main()
