#!/usr/bin/env python3

import argparse
import sys
import urllib.request


def _main(argv: list[str]) -> None:
    cmd_name = "unicode-case-folding-map"

    parser = argparse.ArgumentParser(
        prog=cmd_name,
        description="Generate entries for phf map of unicode case folding.",
    )
    parser.add_argument("-u", default="16.0.0", help="unicode version")
    ns = parser.parse_args(argv)

    url = f"http://www.unicode.org/Public/{ns.u}/ucd/CaseFolding.txt"

    try:
        with urllib.request.urlopen(url) as resp:
            bs = resp.read()
    except Exception as e:
        print(f"Failed to get CaseFolding.txt: {e}", file=sys.stderr)
        raise SystemExit(1) from e

    text = bs.decode("utf-8", errors="replace")
    for line in text.splitlines():
        if line.startswith("#") or not line.strip():
            continue

        line = line.split("#", 1)[0]
        parts = [p.strip() for p in line.split(";")]
        if len(parts) < 3:
            continue

        from_cp = int(parts[0], 16)
        cls = parts[1][0] if parts[1] else ""

        if cls not in ("C", "F"):
            continue

        to_cps = [int(v, 16) for v in parts[2].split() if v]
        tos = "".join([f"\\u{{{v:x}}}" for v in to_cps])

        print(rf"""\u{{{from_cp:x}}} {tos}""")


if __name__ == "__main__":
    _main(sys.argv[1:])
