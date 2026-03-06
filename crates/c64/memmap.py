#!/usr/bin/env python3
"""Parse a rust-mos linker map and report RAM/hiram usage."""
import re
import sys

def main():
    path = sys.argv[1] if len(sys.argv) > 1 else "map.txt"
    sections = {}
    with open(path) as f:
        for line in f:
            m = re.match(
                r"\s+([0-9a-f]+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s+\d+\s+(\.\S+)",
                line,
            )
            if m and int(m.group(3), 16) > 0:
                sections[m.group(4)] = (int(m.group(1), 16), int(m.group(3), 16))

    ram = {n: (v, s) for n, (v, s) in sections.items() if 0x0800 <= v < 0xD000}
    hiram = {n: (v, s) for n, (v, s) in sections.items() if v >= 0xE000}
    ram_total = sum(s for _, s in ram.values())
    hiram_total = sum(s for _, s in hiram.values())
    ram_cap = 0xCFFF - 0x0801 + 1
    hiram_cap = 0xFFF7 - 0xE000 + 1
    ram_free = ram_cap - ram_total
    hiram_free = hiram_cap - hiram_total

    print("--- RAM ($0801-$CFFF, 50 KB) ---")
    for n, (_, s) in sorted(ram.items(), key=lambda x: -x[1][1]):
        print(f"  {n:24s} {s:6d} B  ({s/1024:.1f} KB)")
    print(f"  {'TOTAL':24s} {ram_total:6d} B  ({ram_total/1024:.1f} KB)")
    print(f"  {'FREE':24s} {ram_free:6d} B  ({ram_free/1024:.1f} KB)")
    print()
    print("--- hiram ($E000-$FFF7, 8 KB) ---")
    for n, (_, s) in sorted(hiram.items(), key=lambda x: -x[1][1]):
        print(f"  {n:24s} {s:6d} B  ({s/1024:.1f} KB)")
    print(f"  {'TOTAL':24s} {hiram_total:6d} B  ({hiram_total/1024:.1f} KB)")
    print(f"  {'FREE':24s} {hiram_free:6d} B  ({hiram_free/1024:.1f} KB)")
    print()

    ok = ram_free >= 0 and hiram_free >= 0
    if ram_free >= 0:
        print(f"RAM:   OK ({ram_free} bytes free)")
    else:
        print(f"RAM:   OVERFLOW by {-ram_free} bytes!")
    if hiram_free >= 0:
        print(f"hiram: OK ({hiram_free} bytes free)")
    else:
        print(f"hiram: OVERFLOW by {-hiram_free} bytes!")
    sys.exit(0 if ok else 1)

if __name__ == "__main__":
    main()
