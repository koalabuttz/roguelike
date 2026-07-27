#!/usr/bin/env python3
"""Parse a rust-mos linker map and enforce C64 memory budgets."""

import argparse
import json
import re
import sys
from pathlib import Path


REGIONS = {
    "ram": (0x0801, 0xCFFF, "RAM ($0801-$CFFF, 50 KB)"),
    "ioram": (0xD000, 0xDFFF, "I/O overlay ($D000-$DFFF, 4 KB)"),
    "hiram": (0xE000, 0xFFF7, "HIRAM ($E000-$FFF7, 8 KB)"),
}


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("map", nargs="?", default="map.txt", help="linker map")
    parser.add_argument(
        "--budget",
        type=Path,
        help="JSON file containing minimum-free floors and reference usage",
    )
    parser.add_argument("--json", type=Path, dest="json_path", help="write JSON report")
    return parser.parse_args()


def parse_sections(path):
    sections = {}
    with open(path, encoding="utf-8") as map_file:
        for line in map_file:
            match = re.match(
                r"\s+([0-9a-f]+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s+\d+\s+(\.\S+)",
                line,
            )
            if match and int(match.group(3), 16) > 0:
                sections[match.group(4)] = {
                    "address": int(match.group(1), 16),
                    "size": int(match.group(3), 16),
                }
    return sections


def load_budget(path):
    if path is None:
        return {"regions": {}}
    with open(path, encoding="utf-8") as budget_file:
        budget = json.load(budget_file)
    if budget.get("schema_version") != 1 or not isinstance(budget.get("regions"), dict):
        raise ValueError(f"{path}: unsupported C64 memory-budget schema")
    return budget


def measure_regions(sections, budget):
    measured = {}
    for name, (start, end, label) in REGIONS.items():
        members = {
            section: values
            for section, values in sections.items()
            if start <= values["address"] <= end
        }
        capacity = end - start + 1
        used = sum(values["size"] for values in members.values())
        high = max(
            (values["address"] + values["size"] - 1 for values in members.values()),
            default=start - 1,
        )
        placement_free = end - high if members else capacity
        free = min(capacity - used, placement_free)

        limits = budget.get("regions", {}).get(name, {})
        minimum_free = limits.get("minimum_free", 0)
        target_free = limits.get("target_free", minimum_free)
        reference_used = limits.get("reference_used")
        measured[name] = {
            "label": label,
            "start": start,
            "end": end,
            "capacity": capacity,
            "used": used,
            "free": free,
            "minimum_free": minimum_free,
            "target_free": target_free,
            "target_met": free >= target_free,
            "reference_used": reference_used,
            "delta_used": used - reference_used if reference_used is not None else None,
            "sections": members,
            "ok": free >= minimum_free,
        }
    return measured


def print_report(regions, budget_path):
    for region in regions.values():
        print(f"--- {region['label']} ---")
        for section, values in sorted(
            region["sections"].items(), key=lambda item: -item[1]["size"]
        ):
            size = values["size"]
            print(f"  {section:24s} {size:6d} B  ({size / 1024:.1f} KB)")
        print(
            f"  {'TOTAL':24s} {region['used']:6d} B"
            f"  ({region['used'] / 1024:.1f} KB)"
        )
        print(
            f"  {'FREE':24s} {region['free']:6d} B"
            f"  ({region['free'] / 1024:.1f} KB)"
        )
        if region["delta_used"] is not None:
            print(f"  {'DELTA USED':24s} {region['delta_used']:+6d} B")
        if budget_path is not None:
            print(f"  {'MINIMUM FREE':24s} {region['minimum_free']:6d} B")
            print(f"  {'TARGET FREE':24s} {region['target_free']:6d} B")
        print()

    for name, region in regions.items():
        display = "HIRAM" if name == "hiram" else name.upper()
        if region["ok"]:
            print(
                f"{display:7s} OK ({region['free']} bytes free;"
                f" floor {region['minimum_free']})"
            )
        else:
            shortfall = region["minimum_free"] - region["free"]
            print(
                f"{display:7s} BELOW BUDGET by {shortfall} bytes"
                f" ({region['free']} free; floor {region['minimum_free']})!"
            )
        if not region["target_met"]:
            print(
                f"{display:7s} target missed by"
                f" {region['target_free'] - region['free']} bytes"
            )


def main():
    args = parse_args()
    try:
        budget = load_budget(args.budget)
        sections = parse_sections(args.map)
        regions = measure_regions(sections, budget)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(error, file=sys.stderr)
        return 2

    report = {
        "schema_version": 1,
        "map": str(args.map),
        "budget": str(args.budget) if args.budget else None,
        "ok": all(region["ok"] for region in regions.values()),
        "regions": regions,
    }
    print_report(regions, args.budget)

    if args.json_path:
        with open(args.json_path, "w", encoding="utf-8") as report_file:
            json.dump(report, report_file, indent=2, sort_keys=True)
            report_file.write("\n")

    return 0 if report["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
