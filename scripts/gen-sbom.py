#!/usr/bin/env python3
"""Generate a minimal CycloneDX-style SBOM from Cargo.lock.

Usage: python scripts/gen-sbom.py [--out target/sbom.json]
"""
import argparse
import json
import sys
from pathlib import Path


def parse_cargo_lock(path: Path):
    packages = []
    current = None
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line == "[[package]]":
            if current:
                packages.append(current)
            current = {}
        elif current is not None and "=" in line:
            key, value = line.split("=", 1)
            current[key.strip().strip('"')] = value.strip().strip('"')
    if current:
        packages.append(current)
    return packages


def main():
    parser = argparse.ArgumentParser(prog="gen-sbom")
    parser.add_argument("--out", default="target/sbom.json")
    parser.add_argument("--lock", default="Cargo.lock")
    args = parser.parse_args()

    lock = Path(args.lock)
    if not lock.exists():
        print("Cargo.lock not found", file=sys.stderr)
        return 1

    packages = parse_cargo_lock(lock)
    components = [
        {
            "type": "library",
            "name": p.get("name", ""),
            "version": p.get("version", ""),
            "source": p.get("source", "local"),
        }
        for p in packages
        if p.get("name")
    ]
    sbom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": "urn:uuid:zaion-sbom",
        "version": 1,
        "components": components,
        "component_count": len(components),
    }
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(sbom, indent=2), encoding="utf-8")
    print("SBOM written: %s (%d components)" % (out, len(components)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
