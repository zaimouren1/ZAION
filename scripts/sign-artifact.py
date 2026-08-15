#!/usr/bin/env python3
"""Sign and verify release artifacts with an Ed25519 key (openssl CLI).

Usage:
  python scripts/sign-artifact.py gen-key --key release-ed25519.pem --pub release-ed25519.pub.pem
  python scripts/sign-artifact.py sign --key release-ed25519.pem --in ARTIFACT --out ARTIFACT.sig
  python scripts/sign-artifact.py verify --pub release-ed25519.pub.pem --in ARTIFACT --sig ARTIFACT.sig

Exit: 0 ok; 1 signature invalid / error.
"""
import argparse
import subprocess
import sys


def openssl(args):
    return subprocess.run(["openssl"] + args, capture_output=True, text=True)


def cmd_gen(args):
    r = openssl(["genpkey", "-algorithm", "Ed25519", "-out", args.key])
    if r.returncode != 0:
        print(r.stderr, file=sys.stderr)
        return 1
    r2 = openssl(["pkey", "-in", args.key, "-pubout", "-out", args.pub])
    if r2.returncode != 0:
        print(r2.stderr, file=sys.stderr)
        return 1
    print("key written: %s (private) + %s (public)" % (args.key, args.pub))
    return 0


def cmd_sign(args):
    r = openssl(
        ["pkeyutl", "-sign", "-inkey", args.key, "-rawin", "-in", args.infile, "-out", args.out]
    )
    if r.returncode != 0:
        print(r.stderr, file=sys.stderr)
        return 1
    print("signature written: %s" % args.out)
    return 0


def cmd_verify(args):
    r = openssl(
        [
            "pkeyutl", "-verify", "-pubin", "-inkey", args.pub,
            "-rawin", "-in", args.infile, "-sigfile", args.sig,
        ]
    )
    if r.returncode == 0:
        print("signature OK")
        return 0
    print("signature INVALID", file=sys.stderr)
    return 1


def main():
    p = argparse.ArgumentParser(prog="sign-artifact")
    sub = p.add_subparsers(dest="cmd", required=True)

    g = sub.add_parser("gen-key")
    g.add_argument("--key", required=True)
    g.add_argument("--pub", required=True)
    g.set_defaults(fn=cmd_gen)

    s = sub.add_parser("sign")
    s.add_argument("--key", required=True)
    s.add_argument("--in", dest="infile", required=True)
    s.add_argument("--out", required=True)
    s.set_defaults(fn=cmd_sign)

    v = sub.add_parser("verify")
    v.add_argument("--pub", required=True)
    v.add_argument("--in", dest="infile", required=True)
    v.add_argument("--sig", required=True)
    v.set_defaults(fn=cmd_verify)

    args = p.parse_args()
    sys.exit(args.fn(args))


if __name__ == "__main__":
    main()
