#!/usr/bin/env python3
"""verify_receipt.py - verify a receipt's sha256 against its payload.

Usage: python verify_receipt.py <receipt.json>
Exits 0 if the checksum matches (valid), 1 if tampered, 2 on error.
Prints {"id": ..., "valid": bool, "computed": "..."}.
"""
import hashlib, json, sys


def checksum(payload):
    return hashlib.sha256(json.dumps(payload, sort_keys=True).encode()).hexdigest()


def main():
    if len(sys.argv) != 2:
        print("usage: verify_receipt.py <receipt.json>", file=sys.stderr)
        return 2
    with open(sys.argv[1], encoding="utf-8") as fh:
        r = json.load(fh)
    computed = checksum(r["payload"])
    valid = computed == r["sha256"]
    print(json.dumps({"id": r["id"], "valid": valid, "computed": computed[:16] + "..."}))
    return 0 if valid else 1


if __name__ == "__main__":
    sys.exit(main())
