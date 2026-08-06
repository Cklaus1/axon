#!/usr/bin/env python3
"""SPIKE — `AXON_FOR_RLM.md` §5, the accumulating typed session (values persist).

Tests the thesis, not the plumbing: **every prior binding is re-type-checked
before the new cell runs**, which a Python kernel structurally cannot offer.

# How values persist without a live process

The declarations-only version needed no compiler changes but had no values, so
`Engine::read` could only be implemented by CALLING a binding — which executes
model-written code and re-fires side effects. That was the blocker.

Values now persist as **literals**, not as expressions:

    module = <bindings, as literals>      # let v = 42     ← NOT let v = expensive()
           + <accumulated declarations>
           + fn main() { <this cell's statements> }

    axon check module    # re-type-checks EVERY prior binding and declaration
    AXON_DUMP_BINDINGS=… axon run module   # runs the tail, dumps final values

After each cell the interpreter writes its module-level bindings back out as
Axon literals, and those become the next cell's prelude. So `let v = expensive()`
in cell 1 becomes `let v = 42` in cell 2: the value survives and the computation
does not repeat. That keeps the no-replay property the declarations-only version
got for free, and it makes `read(name)` a **lookup in the session file** — no
execution at all, which is what `Engine::read` is documented to require.

A binding that has no literal form (a channel, a closure, a struct) is reported
as `// SKIPPED name: reason` rather than silently dropped — the same
saved/skipped contract `Engine::Snapshot` models and CPython's `dill` uses.

# Usage

    axon_session.py new     <session.ax>
    axon_session.py eval    <session.ax> <cell.ax>     # or - for stdin
    axon_session.py show    <session.ax>
    axon_session.py read    <session.ax> <name>        # slot lookup, runs nothing

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


def compose(bindings: str, decls: str, cell_lets: str, cell_stmts: str) -> str:
    """bindings (literals) + declarations + this cell's new lets + its statements.

    `cell_lets` are emitted as MODULE-level items, not inside `main`, so their
    values land in the interpreter's globals and survive the dump. A `let` left
    inside `main` would be a local and vanish with the frame.
    """
    body = cell_stmts if cell_stmts.strip() else ""
    parts = [p for p in (bindings.strip(), decls.strip(), cell_lets.strip()) if p]
    parts.append("fn main() -> i64 {\n" + body + "\n    0\n}")
    return "\n\n".join(parts) + "\n"


def split_lets(stmts: str):
    """Separate a cell's top-level `let` statements from its other statements.

    Only unindented `let`s at the cell's top level are promoted; anything nested
    inside a block belongs to that block and is left alone.
    """
    lets, rest, depth = [], [], 0
    for line in stmts.splitlines():
        if depth == 0 and line.lstrip().startswith("let ") and line == line.lstrip():
            lets.append(line)
        else:
            rest.append(line)
        depth += line.count("{") - line.count("}")
    return "\n".join(lets), "\n".join(rest)


def session_files(session: Path):
    """(declarations file, bindings file). The session is two plain files on
    purpose: both are readable, diffable and hand-editable."""
    return session, session.with_suffix(".bindings.ax")


def run_axon(verb: str, path: Path, dump: Path | None = None):
    env = dict(**__import__("os").environ)
    if dump is not None:
        env["AXON_DUMP_BINDINGS"] = str(dump)
    return subprocess.run(
        [str(AXON), verb, str(path)], capture_output=True, text=True, env=env
    )


def cmd_eval(session: Path, cell_src: str) -> int:
    decl_file, bind_file = session_files(session)
    decls_acc = decl_file.read_text() if decl_file.exists() else ""
    binds_acc = bind_file.read_text() if bind_file.exists() else ""

    cell_decls, cell_stmts = split_cell(cell_src)
    cell_lets, cell_rest = split_lets(cell_stmts)
    composed = compose(binds_acc, decls_acc + "\n" + cell_decls, cell_lets, cell_rest)

    tmp = session.with_suffix(".cell.ax")
    tmp.write_text(composed)

    # 1. Type-check the WHOLE accumulated module — every prior binding AND
    #    declaration. This is the property Python's kernel cannot offer: a prior
    #    binding used at the wrong type is an error BEFORE anything executes.
    chk = run_axon("check", tmp)
    if chk.returncode != 0:
        sys.stderr.write(chk.stderr)
        sys.stderr.write("session: cell REFUSED — session unchanged\n")
        tmp.unlink(missing_ok=True)
        return 2

    # 2. Run the tail, and have the interpreter hand back its final bindings.
    new_binds = session.with_suffix(".next.ax")
    out = run_axon("run", tmp, dump=new_binds)
    sys.stdout.write(out.stdout)
    if out.returncode != 0:
        sys.stderr.write(out.stderr)
        tmp.unlink(missing_ok=True)
        new_binds.unlink(missing_ok=True)
        return 2

    # 3. Commit. Declarations accumulate as source; bindings are replaced
    #    wholesale by the dump, which is already the post-run truth for every
    #    name — including ones this cell reassigned.
    if cell_decls.strip():
        decl_file.write_text((decls_acc.rstrip() + "\n\n" + cell_decls.strip() + "\n").lstrip())
    if new_binds.exists():
        bind_file.write_text(new_binds.read_text())
        new_binds.unlink(missing_ok=True)
    tmp.unlink(missing_ok=True)
    return 0


def cmd_read(session: Path, name: str) -> int:
    """Read one binding — a LOOKUP, executing nothing.

    This is the method the declarations-only version could not implement
    honestly: with no stored value, `read` had to call the binding, which ran
    model-written code and re-fired its side effects. Here it greps a file.
    """
    _, bind_file = session_files(session)
    if not bind_file.exists():
        return 1
    for line in bind_file.read_text().splitlines():
        if line.startswith(f"let {name} = "):
            sys.stdout.write(line.split(" = ", 1)[1] + "\n")
            return 0
        if line.startswith(f"// SKIPPED {name}:"):
            sys.stderr.write(line + "\n")
            return 1
    return 1


def main() -> int:
    if len(sys.argv) < 3:
        sys.stderr.write(__doc__ or "")
        return 1
    verb, session = sys.argv[1], Path(sys.argv[2])
    if not AXON.exists():
        sys.stderr.write(f"axon binary not found at {AXON}\n")
        return 1
    if verb == "new":
        d, b = session_files(session)
        d.write_text("")
        b.write_text("")
        return 0
    if verb == "show":
        d, b = session_files(session)
        sys.stdout.write(b.read_text() if b.exists() else "")
        sys.stdout.write(d.read_text() if d.exists() else "")
        return 0
    if verb == "read":
        return cmd_read(session, sys.argv[3])
    if verb == "eval":
        src = sys.stdin.read() if sys.argv[3] == "-" else Path(sys.argv[3]).read_text()
        return cmd_eval(session, src)
    sys.stderr.write(f"unknown verb {verb}\n")
    return 1


if __name__ == "__main__":
    sys.exit(main())
