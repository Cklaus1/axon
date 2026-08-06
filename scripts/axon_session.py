#!/usr/bin/env python3
"""SPIKE — `AXON_FOR_RLM.md` §5, the declarations-only accumulating session.

Tests the thesis, not the plumbing: **every prior binding is re-type-checked
before the new cell runs**, which a Python kernel structurally cannot offer.

# The finding this spike exists to establish

A declarations-only session needs **no compiler changes at all**. It is host-side
composition over the CLI that already exists:

    module = <accumulated declarations>  +  fn main() { <this cell's statements> }
    axon check module     # re-type-checks EVERY prior declaration
    axon run   module     # executes only the new tail, because prior cells
                          # contributed declarations, never statements

Prior cells cannot re-execute, because a declaration has no runtime effect —
the "execute only the new tail" semantics that looked like a second execution
mode falls out of the decomposition for free.

What this does NOT do, and where the compiler work actually starts: top-level
VALUES do not persist. `let x = expensive()` in cell 1 is gone in cell 2. Keeping
values needs a live interpreter process holding the environment, which is the
strictly larger version of this feature. Declarations-only is the slice that
tests whether the property is worth having.

# Usage

    axon_session.py new     <session.ax>
    axon_session.py eval    <session.ax> <cell.ax>     # or - for stdin
    axon_session.py show    <session.ax>

Exit codes: 0 ok · 2 the cell was refused (type/parse error) · 1 harness error.
A refused cell leaves the session **unchanged** — that is the point: the session
cannot be corrupted by a cell that would not compile.
"""

import subprocess
import sys
from pathlib import Path

AXON = Path(__file__).resolve().parent.parent / "target" / "debug" / "axon"

# A line opening one of these begins a DECLARATION; anything else at top level is
# a statement belonging to this cell's `main`.
DECL_KEYWORDS = ("fn ", "type ", "enum ", "trait ", "impl ", "mod ", "use ", "handler ")


def split_cell(src: str):
    """Return (declarations, statements) for one cell.

    Brace-counted rather than line-based, so a multi-line `fn` stays intact.
    Attributes (`@[...]`) attach to the declaration that follows them.
    """
    decls, stmts, pending = [], [], []
    lines = src.splitlines()
    i = 0
    while i < len(lines):
        line = lines[i]
        stripped = line.strip()
        if stripped.startswith("@["):
            pending.append(line)
            i += 1
            continue
        if any(stripped.startswith(k) for k in DECL_KEYWORDS):
            block, depth = pending + [line], line.count("{") - line.count("}")
            i += 1
            while i < len(lines) and depth > 0:
                block.append(lines[i])
                depth += lines[i].count("{") - lines[i].count("}")
                i += 1
            decls.extend(block)
            pending = []
            continue
        if stripped:
            stmts.extend(pending + [line])
            pending = []
        i += 1
    return "\n".join(decls), "\n".join(stmts)


def compose(accumulated: str, cell_decls: str, cell_stmts: str) -> str:
    body = cell_stmts if cell_stmts.strip() else ""
    parts = [p for p in (accumulated.strip(), cell_decls.strip()) if p]
    parts.append("fn main() -> i64 {\n" + body + "\n    0\n}")
    return "\n\n".join(parts) + "\n"


def run_axon(verb: str, path: Path):
    return subprocess.run(
        [str(AXON), verb, str(path)], capture_output=True, text=True
    )


def cmd_eval(session: Path, cell_src: str) -> int:
    accumulated = session.read_text() if session.exists() else ""
    decls, stmts = split_cell(cell_src)
    composed = compose(accumulated, decls, stmts)

    tmp = session.with_suffix(".cell.ax")
    tmp.write_text(composed)

    # 1. Type-check the WHOLE accumulated module, not just this cell. This is the
    #    property Python's kernel cannot offer: a prior binding used at the wrong
    #    type is an error BEFORE anything executes.
    chk = run_axon("check", tmp)
    if chk.returncode != 0:
        sys.stderr.write(chk.stderr)
        sys.stderr.write("session: cell REFUSED — session unchanged\n")
        tmp.unlink(missing_ok=True)
        return 2

    # 2. Run only the new tail.
    out = run_axon("run", tmp)
    sys.stdout.write(out.stdout)
    if out.returncode != 0:
        sys.stderr.write(out.stderr)
        tmp.unlink(missing_ok=True)
        return 2

    # 3. Commit this cell's DECLARATIONS to the session. Statements are not kept:
    #    they already ran, and re-running them next cell is the side-effect
    #    replay problem this design avoids by construction.
    if decls.strip():
        session.write_text((accumulated.rstrip() + "\n\n" + decls.strip() + "\n").lstrip())
    tmp.unlink(missing_ok=True)
    return 0


def main() -> int:
    if len(sys.argv) < 3:
        sys.stderr.write(__doc__ or "")
        return 1
    verb, session = sys.argv[1], Path(sys.argv[2])
    if not AXON.exists():
        sys.stderr.write(f"axon binary not found at {AXON}\n")
        return 1
    if verb == "new":
        session.write_text("")
        return 0
    if verb == "show":
        sys.stdout.write(session.read_text() if session.exists() else "")
        return 0
    if verb == "eval":
        src = sys.stdin.read() if sys.argv[3] == "-" else Path(sys.argv[3]).read_text()
        return cmd_eval(session, src)
    sys.stderr.write(f"unknown verb {verb}\n")
    return 1


if __name__ == "__main__":
    sys.exit(main())
