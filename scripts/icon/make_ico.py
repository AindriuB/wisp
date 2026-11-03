#!/usr/bin/env python3
"""make_ico.py <out.ico> <png>...  Pack PNG files (any order) into a PNG-compressed .ico."""
import struct, sys

out, pngs = sys.argv[1], sys.argv[2:]
entries = []
for p in pngs:
    d = open(p, "rb").read()
    assert d[:8] == b"\x89PNG\r\n\x1a\n", p
    w, h = struct.unpack(">II", d[16:24])
    entries.append((w, h, d))
entries.sort(key=lambda e: e[0])
buf = struct.pack("<HHH", 0, 1, len(entries))
off = 6 + 16 * len(entries)
for w, h, d in entries:
    buf += struct.pack("<BBBBHHII", w % 256, h % 256, 0, 0, 1, 32, len(d), off)
    off += len(d)
open(out, "wb").write(buf + b"".join(d for _, _, d in entries))
