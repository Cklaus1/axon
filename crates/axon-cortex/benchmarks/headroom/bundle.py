#!/usr/bin/env python3
"""Build the hx3 per-target bundles (hx3-spec HX3-B): files over cortex's view cap.

    python3 bundle.py --pool POOL.json --seed N --min-chars 26000 --out DIR
    python3 bundle.py --check DIR/manifest.json

One bundle per program P of the pool (`meta.sources`, stratum C or D, either
split side): P's source, unedited, followed by filler programs from P's
recorded split side only (`meta.headroom.split`), in
`random.Random(f"{seed}:{P}:bundle")` order, until the bundle is at least
`--min-chars` long. Each filler is added with its top-level `fn main` removed,
and with every top-level fn whose text equals an already-included definition
removed (a removed item takes its attached `//` comment lines with it). A
filler is skipped when a remaining top-level name (fn, test, type, enum,
struct, trait) collides with the bundle so far.

Program texts are read from the tree (and must equal the pool's recorded
text). Output: `<out>/<side>/<basename of P>` and `<out>/manifest.json` (P,
its side, the fillers in order, the skipped fillers with their collisions,
sha256 of every input and of every bundle). `--check` regenerates every
bundle from the tree and exits 1 on any difference. No binary is called;
`build.py --bundles` type-checks and tests each bundle (`Program`)."""
import argparse
import hashlib
import json
import os
import random
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
sys.path.insert(0, HERE)
from build import code_end  # noqa: E402 (pure)

ITEM = re.compile(r"(?:pub\s+)?(fn|type|enum|struct|trait)\s+(\w+)")


def sha256(text):
    return hashlib.sha256(text.encode()).hexdigest()


def _item_end(text, i, kind):
    """Index just past the line that ends the item whose header starts at
    `i`: string-aware braces must balance; a fn needs its body brace."""
    o = text.find("{", i)
    nl = text.find("\n", i)
    nl = len(text) if nl < 0 else nl
    if kind == "fn" or (0 <= o < nl):
        end = code_end(text, o) if o >= 0 else None
        if end is None:
            raise ValueError(f"unclosed top-level {kind} at offset {i}")
        nl = text.find("\n", end)
    return len(text) if nl < 0 else nl + 1


def items(text):
    """[(kind, name, start, core, end)] for every top-level item: `start`
    includes the `//` comment lines directly above it, `core` starts at its
    first attribute (or its header) and `end` is past its last line."""
    out, pos, lines, offs = [], 0, text.splitlines(keepends=True), []
    for line in lines:
        offs.append(pos)
        pos += len(line)
    i, taken = 0, 0  # `taken`: first line not inside an earlier item
    while i < len(lines):
        j = i
        while j < len(lines) and lines[j].startswith("@["):
            j += 1
        m = ITEM.match(lines[j]) if j < len(lines) else None
        if not m:
            i += 1
            continue
        s = i
        while s > taken and lines[s - 1].startswith("//"):
            s -= 1
        end = _item_end(text, offs[j], m.group(1))
        out.append((m.group(1), m.group(2), offs[s], offs[i], end))
        while i < len(lines) and offs[i] < end:
            i += 1
        taken = i
    return out


def names(text):
    return {n for _, n, _, _, _ in items(text)}


def cores(text):
    return {text[c:e] for _, _, _, c, e in items(text)}


def strip(text, defs):
    """`text` without its top-level `fn main` and without each top-level fn
    whose text is in `defs`."""
    for kind, n, s, c, e in reversed(items(text)):
        if kind == "fn" and (n == "main" or text[c:e] in defs):
            text = text[:s] + text[e:]
    return text


def make_bundle(program, srcs, split, seed, min_chars):
    """One bundle (dict): `text` starts with srcs[program], unedited."""
    side = split[program]
    text = srcs[program]
    have, defs, fillers, skipped = names(text), cores(text), [], {}
    order = sorted(p for p in srcs if p != program and split.get(p) == side)
    random.Random(f"{seed}:{program}:bundle").shuffle(order)
    for f in order:
        if len(text) >= min_chars:
            break
        t = strip(srcs[f], defs)
        clash = names(t) & have
        if clash:
            skipped[f] = sorted(clash)
            continue
        text = text.rstrip("\n") + "\n\n" + t.lstrip("\n")
        have |= names(t)
        defs |= cores(t)
        fillers.append(f)
    return {"program": program, "side": side, "text": text, "fillers": fillers, "skipped": skipped,
            "chars": len(text), "reached": len(text) >= min_chars}


def _bundles(srcs, split, seed, min_chars):
    """Every bundle; `path` is `<side>/<basename of P>`, relative to the
    manifest's directory."""
    made, seen = [], {}
    for p in sorted(srcs):
        b = make_bundle(p, srcs, split, seed, min_chars)
        b["path"] = os.path.join(b["side"], os.path.basename(p))
        if b["path"] in seen:
            raise SystemExit(f"bundle path {b['path']} is shared by {seen[b['path']]} and {p}")
        seen[b["path"]] = p
        made.append(b)
    return made


def write_bundles(srcs, split, seed, min_chars, out):
    """Write every bundle and the manifest; returns the manifest path.
    `srcs` = {program path relative to the tree root: text}."""
    made = _bundles(srcs, split, seed, min_chars)
    for b in made:
        f = os.path.join(out, b["path"])
        os.makedirs(os.path.dirname(f), exist_ok=True)
        with open(f, "w") as fh:
            fh.write(b["text"])
    man = {"schema": "hx3-bundles/1", "seed": seed, "min_chars": min_chars,
           "split": {p: split[p] for p in sorted(srcs)},
           "sources": {p: sha256(t) for p, t in sorted(srcs.items())},
           "bundles": [{k: v for k, v in b.items() if k != "text"} | {"sha256": sha256(b["text"])}
                       for b in made]}
    path = os.path.join(out, "manifest.json")
    with open(path, "w") as f:
        json.dump(man, f, indent=1)
    return path


def check_manifest(path, root):
    """[difference] between the manifest, the bundles on disk and bundles
    regenerated from the program texts under `root`; [] when they agree."""
    man = json.load(open(path))
    bad, srcs = [], {}
    for p, h in man["sources"].items():
        f = os.path.join(root, p)
        srcs[p] = open(f).read() if os.path.exists(f) else None
        if srcs[p] is None or sha256(srcs[p]) != h:
            bad.append(f"source {p} differs from the recorded sha256")
    if bad:
        return bad
    out = os.path.dirname(os.path.abspath(path))
    want = {b["path"]: b for b in man["bundles"]}
    for b in _bundles(srcs, man["split"], man["seed"], man["min_chars"]):
        rec = want.pop(b["path"], None)
        if rec is None:
            bad.append(f"{b['path']}: not in the manifest")
            continue
        if sha256(b["text"]) != rec["sha256"] or b["fillers"] != rec["fillers"]:
            bad.append(f"{b['path']}: regenerated bundle differs from the manifest")
        f = os.path.join(out, b["path"])
        if not os.path.exists(f) or sha256(open(f).read()) != rec["sha256"]:
            bad.append(f"{b['path']}: file on disk differs from the manifest")
    bad += [f"{p}: in the manifest, not regenerated" for p in want]
    return bad


def pool_sources(pool_path, root=ROOT):
    """({program: tree text}, {program: side}) for the pool's programs; a
    tree text that is not the pool's recorded text is refused."""
    pool = json.load(open(pool_path))
    split = pool["meta"]["headroom"]["split"]
    srcs = {}
    for p, t in pool["meta"]["sources"].items():
        srcs[p] = open(os.path.join(root, p)).read()
        if srcs[p] != t:
            raise SystemExit(f"{p}: the tree differs from the pool's recorded source")
    return srcs, {p: split[p] for p in srcs}


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--pool")
    ap.add_argument("--seed", type=int)
    ap.add_argument("--min-chars", type=int, default=26000)
    ap.add_argument("--out")
    ap.add_argument("--check", metavar="MANIFEST")
    a = ap.parse_args(argv)
    if a.check:
        bad = check_manifest(a.check, ROOT)
        for b in bad:
            print("FAIL " + b, file=sys.stderr)
        print(f"hx3 bundle check: {len(bad)} differences", file=sys.stderr)
        return 1 if bad else 0
    if not a.pool or a.seed is None or not a.out:
        ap.error("--pool, --seed and --out are required to build")
    srcs, split = pool_sources(a.pool)
    man = write_bundles(srcs, split, a.seed, a.min_chars, a.out)
    bs = json.load(open(man))["bundles"]
    for side in ("dev", "test"):
        v = [b for b in bs if b["side"] == side]
        print(f"{side}: {sum(b['reached'] for b in v)}/{len(v)} reach {a.min_chars}; chars "
              f"{min(b['chars'] for b in v)}-{max(b['chars'] for b in v)}; fillers "
              f"{min(len(b['fillers']) for b in v)}-{max(len(b['fillers']) for b in v)}", file=sys.stderr)
    print(man)
    return 0


if __name__ == "__main__":
    sys.exit(main())
