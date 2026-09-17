#!/usr/bin/env python3
"""Render one dated release section, rejecting missing notes and encoding artifacts."""
import argparse
from pathlib import Path
import re
import sys

SPECIALS = {cp: byte for byte, cp in {
    0x80:0x20AC, 0x82:0x201A, 0x83:0x0192, 0x84:0x201E, 0x85:0x2026,
    0x86:0x2020, 0x87:0x2021, 0x88:0x02C6, 0x89:0x2030, 0x8A:0x0160,
    0x8B:0x2039, 0x8C:0x0152, 0x8E:0x017D, 0x91:0x2018, 0x92:0x2019,
    0x93:0x201C, 0x94:0x201D, 0x95:0x2022, 0x96:0x2013, 0x97:0x2014,
    0x98:0x02DC, 0x99:0x2122, 0x9A:0x0161, 0x9B:0x203A, 0x9C:0x0153,
    0x9E:0x017E, 0x9F:0x0178,
}.items()}

def cp1252_byte(ch):
    o = ord(ch)
    if 0xA0 <= o <= 0xFF:
        return o
    return SPECIALS.get(o)

def scan(text):
    chars = list(text)
    runs, c1 = [], 0
    i = 0
    while i < len(chars):
        ch = chars[i]
        o = ord(ch)
        if 0x80 <= o <= 0x9F:
            c1 += 1
            i += 1
            continue
        lead = cp1252_byte(ch)
        if lead is None:
            i += 1
            continue
        cont = 1 if 0xC2 <= lead <= 0xDF else 2 if 0xE0 <= lead <= 0xEF else 3 if 0xF0 <= lead <= 0xF4 else 0
        if cont == 0:
            i += 1
            continue
        seq = [lead]
        for k in range(1, cont + 1):
            if i + k >= len(chars):
                break
            b = cp1252_byte(chars[i + k])
            if b is None or not (0x80 <= b <= 0xBF):
                break
            seq.append(b)
        if len(seq) != cont + 1:
            i += 1
            continue
        try:
            dec = bytes(seq).decode('utf-8')
        except UnicodeDecodeError:
            i += 1
            continue
        strong = cont >= 2 or lead in (0xC2, 0xC3)
        runs.append([i, cont + 1, strong, ''.join(chars[i:i+cont+1]), dec])
        i += cont + 1
    kept = []
    for j, r in enumerate(runs):
        end = r[0] + r[1]
        neighbor = (j > 0 and runs[j-1][0] + runs[j-1][1] == r[0]) or \
                   (j+1 < len(runs) and runs[j+1][0] == end)
        if r[2] or neighbor:
            kept.append(r)
    return kept, c1

def render(version, text):
    kept, c1 = scan(text)
    if kept or c1:
        raise ValueError("CHANGELOG.md contains encoding artifacts; repair its UTF-8 text before release")
    lines = text.splitlines()
    heading = re.compile(r"^## \[" + re.escape(version) + r"\] - \d{4}-\d{2}-\d{2}$")
    indices = [i for i, line in enumerate(lines) if heading.fullmatch(line)]
    if len(indices) != 1:
        raise ValueError(f"CHANGELOG.md requires exactly one dated section for {version}")
    body = []
    for line in lines[indices[0]+1:]:
        if line.startswith("## "):
            break
        body.append(line)
    if not any(line.strip() and not line.startswith("#") for line in body):
        raise ValueError(f"CHANGELOG.md has no release notes for {version}; add user-visible changes")
    return "\n".join(body).strip() + "\n\n" + (
        "Prebuilt CLI archives are attached for Windows x64, macOS Intel, macOS ARM, and Linux x64.\n"
        "Merge `.agents`, `.claude`, and `tools` into the project root. Skills are ready to discover; executables, documentation, licenses and the source manifest live under `tools/contextmink`.\n"
        "Verify the adjacent SHA-256 checksum before extraction.\n"
    )

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("changelog", nargs="?", type=Path, default=Path("CHANGELOG.md"))
    args = parser.parse_args()
    try:
        sys.stdout.write(render(args.version, args.changelog.read_text(encoding="utf-8")))
    except (ValueError, OSError) as error:
        sys.exit(str(error))
