#!/usr/bin/env python3
"""fault_inject.py - fault injection toolkit for Zaion 300-task eval.

Subcommands (stdlib only):
  kill-after <cmd...> --after N --match PATTERN
      Run a command, kill it after N stdout lines match PATTERN (simulate crash).
  disk-full <path> --fill-mb MB
      Fill a directory with a sparse file to simulate disk-full conditions.
  reorder --file F [--seed S]
      Shuffle lines of a JSONL/event file (out-of-order events).
  repeat --file F --times N
      Duplicate each line N times (replay / idempotency).
  tamper --file F --offset N [--xor MASK]
      Corrupt a byte at offset (signature tampering).

Exit codes: 0 ok, 1 usage/error, 137 killed-by-signal for kill-after.
"""
import argparse, os, random, subprocess, sys


def cmd_kill_after(args):
    proc = subprocess.Popen(
        args.cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1
    )
    matches = 0
    for line in proc.stdout:
        if args.match and args.match in line:
            matches += 1
            if matches >= args.after:
                proc.kill()
                proc.wait()
                print("killed after %d matches" % matches, file=sys.stderr)
                return 137
    proc.wait()
    return proc.returncode


def cmd_disk_full(args):
    os.makedirs(args.path, exist_ok=True)
    target = os.path.join(args.path, "fill-%dmb.bin" % args.fill_mb)
    with open(target, "wb") as fh:
        fh.seek(args.fill_mb * 1024 * 1024 - 1)
        fh.write(b"\0")
    print("wrote sparse fill file: %s (%d MB)" % (target, args.fill_mb))
    return 0


def cmd_reorder(args):
    with open(args.file, encoding="utf-8") as fh:
        lines = fh.readlines()
    random.Random(args.seed or 1).shuffle(lines)
    out = args.file + ".reordered"
    with open(out, "w", encoding="utf-8") as fh:
        fh.writelines(lines)
    print("wrote %d shuffled lines to %s" % (len(lines), out))
    return 0


def cmd_repeat(args):
    with open(args.file, encoding="utf-8") as fh:
        lines = fh.readlines()
    out = args.file + ".replayed"
    with open(out, "w", encoding="utf-8") as fh:
        for line in lines:
            for _ in range(args.times):
                fh.write(line)
    print("wrote %d lines to %s" % (len(lines) * args.times, out))
    return 0


def cmd_tamper(args):
    with open(args.file, "rb") as fh:
        data = bytearray(fh.read())
    if args.offset >= len(data):
        print("offset %d out of range (%d bytes)" % (args.offset, len(data)), file=sys.stderr)
        return 1
    data[args.offset] ^= args.xor & 0xFF
    with open(args.file, "wb") as fh:
        fh.write(bytes(data))
    print("tampered byte at offset %d (xor %d)" % (args.offset, args.xor))
    return 0


def main():
    p = argparse.ArgumentParser(prog="fault_inject")
    sub = p.add_subparsers(dest="tool", required=True)

    k = sub.add_parser("kill-after")
    k.add_argument("cmd", nargs="+")
    k.add_argument("--after", type=int, default=1)
    k.add_argument("--match", default="")
    k.set_defaults(fn=cmd_kill_after)

    d = sub.add_parser("disk-full")
    d.add_argument("path")
    d.add_argument("--fill-mb", type=int, default=50)
    d.set_defaults(fn=cmd_disk_full)

    r = sub.add_parser("reorder")
    r.add_argument("--file", required=True)
    r.add_argument("--seed", type=int)
    r.set_defaults(fn=cmd_reorder)

    rp = sub.add_parser("repeat")
    rp.add_argument("--file", required=True)
    rp.add_argument("--times", type=int, default=2)
    rp.set_defaults(fn=cmd_repeat)

    t = sub.add_parser("tamper")
    t.add_argument("--file", required=True)
    t.add_argument("--offset", type=int, required=True)
    t.add_argument("--xor", type=int, default=255)
    t.set_defaults(fn=cmd_tamper)

    args, extra = p.parse_known_args()
    if args.tool == "kill-after" and extra:
        args.cmd = args.cmd + extra
    sys.exit(args.fn(args))


if __name__ == "__main__":
    main()
