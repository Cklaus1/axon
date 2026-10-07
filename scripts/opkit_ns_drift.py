#!/usr/bin/env python3
"""opkit_ns_drift.py -- no test may run the operator deployment kit, or any other
state-changing verb, outside the namespace helper (amendments 92, 97).

Incident 2026-10-06: a guard-removal experiment made a kit refusal test a REAL
root `--apply` on the dev host. The structural cure is scripts/lib/opkit_ns.sh
(`ns_run`: private mount/PID/UTS/IPC/NET namespaces, tmpfs over every destination,
isolation PROVED before the command starts). This gate keeps it the only door:

  1. every SIMPLE COMMAND, in any test script, that RUNS the kit (`$KIT`,
     `operator_deploy_protected_host.sh`), carries `--apply`, or runs a controlled-build
     verb (`guest_build_env.py begin|cargo|host-build|...`, the fixture script) must
     itself BE an `ns_run`/`kit`/`inns` command: the line is split into simple commands
     at `; && || | & $( ( ` ` { } then do else`, comments dropped, and the wrapper must be
     the command's first word (after VAR=value prefixes, and after the `refused LABEL
     PATTERN` test helper). `ns_run true; bash "$KIT" --apply` has an `ns_run` on the
     line and is REFUSED: the second command is not inside it (amendment 97, f);
  2. a state-changing command (mkdir, mktemp, rm, cp, install, chown, chmod, ln, mv, tee,
     useradd, groupadd) whose operand is under a real destination is held to the same rule;
  3. the kit-running scripts must source/use opkit_ns.sh at all;
  4. DESTINATIONS (amendment 97, a): every write target the kit makes -- `act_dir`/`act_install`
     operands, and the operands of its own mkdir/install/cp/mv/ln/chown/chmod/tee/useradd/groupadd
     -- resolves (kit variables expanded) under a destination ns_run shadows
     (OPKIT_DEFAULT_DESTS in lib/opkit_ns.sh, the one list). A new kit write to /opt or
     /usr/lib/systemd fails HERE instead of reaching the host;
  5. `--selftest` plants the bypass shapes in scratch copies and requires every one refused, and
     requires the real scripts and the controls to pass.

Exit 0 clean, 1 a violation.  Usage: opkit_ns_drift.py [--selftest] [ROOT]
"""
import os, re, shlex, sys

KIT_TOKEN = re.compile(r'\$\{?KIT\}?(?![\w])|operator_deploy_protected_host\.sh')
BUILD_VERB = re.compile(r'guest_build_env\.py|opkit_fixture\.sh')
WRAPPERS = {"ns_run", "kit", "inns"}
# the first word of a simple command that EXECUTES (or hands to an interpreter) what it is given
RUNNERS = {"bash", "sh", "dash", ".", "source", "exec", "env", "setpriv", "sudo", "nohup", "timeout", "unshare",
           "nsenter", "chroot", "python3", "python", "setsid", "xargs", "command", "time", "nice", "flock"}
HELPER_COMMANDS = {"refused": 2}        # `refused LABEL PATTERN cmd...`: skip its two leading args
MUTATORS = {"mkdir", "mktemp", "rm", "cp", "install", "chown", "chmod", "ln", "mv", "tee", "useradd", "groupadd",
            "touch", "truncate", "rmdir"}
REAL = ("/etc", "/usr/local", "/var/lib", "/var/log", "/var/spool", "/var/mail", "/run", "/srv", "/opt", "/usr/lib",
        "/usr/share", "/var/cache", "/boot", "/root", "/home", "/lib")
SPLIT = re.compile(r'(\|\||&&|;;|[;&|(){}`\n]|\$\()')
KEYWORDS = {"then", "do", "else", "elif", "if", "while", "until", "!", "time"}


def logical_lines(text):
    """(first physical line number, joined text): backslash-continued lines joined; comments dropped; heredoc
    BODIES kept (an in-namespace script's body is judged like any other shell)."""
    out, cur, start = [], "", 0
    for n, raw in enumerate(text.splitlines(), 1):
        if not cur:
            start = n
        if raw.rstrip().endswith("\\"):
            cur += raw.rstrip()[:-1] + " "
            continue
        cur += raw
        out.append((start, cur))
        cur = ""
    if cur:
        out.append((start, cur))
    return out


def strip_comment(line):
    q, i = None, 0
    while i < len(line):
        c = line[i]
        if q:
            if c == q:
                q = None
            elif c == "\\" and q == '"':
                i += 1
        elif c in "'\"":
            q = c
        elif c == "\\":
            i += 1
        elif c == "#" and (i == 0 or line[i - 1] in " \t;&|(){}"):
            return line[:i]
        i += 1
    return line


def split_simple(line):
    """Simple commands of `line`: split at control operators OUTSIDE quotes."""
    cmds, cur, q, i = [], "", None, 0
    while i < len(line):
        c = line[i]
        if q:
            cur += c
            if c == q:
                q = None
            elif c == "\\" and q == '"' and i + 1 < len(line):
                i += 1
                cur += line[i]
        elif c in "'\"":
            q = c
            cur += c
        elif c == "\\" and i + 1 < len(line):
            cur += c + line[i + 1]
            i += 1
        elif line.startswith("$(", i):
            cmds.append(cur)
            cur = ""
            i += 1
        elif line.startswith("&&", i) or line.startswith("||", i) or line.startswith(";;", i):
            cmds.append(cur)
            cur = ""
            i += 1
        elif c in ";&|(){}`":
            # `${x}` / `$((` are not control operators
            if c == "{" and i > 0 and line[i - 1] == "$":
                cur += c
            elif c in "{}" and not (cur.strip() == "" and (i + 1 >= len(line) or line[i + 1] in " \t")):
                cur += c
            elif c == "&" and i + 1 < len(line) and line[i + 1] in ">":
                cur += c
            elif c == "&" and i > 0 and line[i - 1] in "<>":
                cur += c
            else:
                cmds.append(cur)
                cur = ""
        else:
            cur += c
        i += 1
    cmds.append(cur)
    return [c for c in cmds if c.strip()]


def words(cmd):
    try:
        w = shlex.split(cmd, posix=True, comments=False)
    except ValueError:
        w = cmd.split()
    while w and w[0] in KEYWORDS:
        w = w[1:]
    while w and re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*=.*', w[0]):
        w = w[1:]
    return w


def strip_helper(w):
    n = HELPER_COMMANDS.get(w[0]) if w else None
    return w[1 + n:] if n is not None and len(w) > 1 + n else w


def command_class(w):
    """(kind, wrapped): what a simple command does that needs the helper, and whether it IS the helper."""
    w = strip_helper(w)
    if not w:
        return None, True
    first = w[0]
    wrapped = first in WRAPPERS
    # the part the wrapper runs (or the whole command when unwrapped)
    rest = w[1:] if wrapped else w
    cmd0 = rest[0] if rest else ""
    flat = " ".join(rest)
    kind = None
    if "--apply" in rest and (cmd0 in RUNNERS or KIT_TOKEN.search(flat) or "ARGS" in flat or cmd0 == "setpriv"):
        kind = "--apply"
    elif KIT_TOKEN.search(flat) and (cmd0 in RUNNERS or KIT_TOKEN.fullmatch(cmd0 or "x")):
        # a kit COPY named differently is caught by its --apply; reading the kit (python3 - "$KIT", cp, sed) runs nothing
        if cmd0 not in ("python3", "python"):
            kind = "kit"
    if kind is None and BUILD_VERB.search(flat):
        verb = re.search(r'guest_build_env\.py\s+(begin|cargo|host-build|kernel|rootfs|finish|dist|discard)\b', flat)
        if verb or "opkit_fixture.sh" in flat:
            kind = "build"
    if kind is None and (cmd0 in MUTATORS or cmd0 in ("python3", "python", "bash", "sh")):
        ops = [x for x in rest[1:] if not x.startswith("-")]
        if cmd0 in ("cp", "mv", "install", "ln"):
            ops = ops[-1:]                    # only the DESTINATION of a copy is written
        elif cmd0 in ("chown", "chmod"):
            ops = ops[1:]
        elif cmd0 in ("python3", "python", "bash", "sh"):
            ops = [x for x in ops if "/" in x or "KEYPARENT" in x or "FORGE" in x]
        for a in ops:
            if any(a == r or a.startswith(r + "/") for r in REAL) or re.search(r'\$\{?(KEYPARENT|FORGE)\b', a):
                kind = "write"
    if kind is None and cmd0 in ("unshare",) and "--pid" in rest and any("python" in x for x in rest):
        kind = "build"
    return kind, wrapped


HEREDOC = re.compile(r"<<-?\s*(['\"]?)(\w+)\1\s*$")
DEST_LITERAL = re.compile(r"""\b(makedirs|mkdir|chown|chmod|rename|copy\w*|copytree|move|open|symlink|run|call)\(\s*\[?\s*f?["'](/etc|/usr/local|/var/lib|/var/log|/var/spool|/run|/srv|/opt)\b""")


def check_text(path, text):
    bad = []
    kit_helper = re.compile(r'^\s*kit\(\)\s*\{\s*opkit_ns_assert\s*\|\|[^;{}]*;\s*bash "\$KIT" "\$@";\s*\}\s*$')
    lines = logical_lines(text)
    i = 0
    while i < len(lines):
        n, line = lines[i]
        i += 1
        # a heredoc: gather its body; an IN-NAMESPACE body (it starts by sourcing the helper) is judged for
        # kit/--apply/build commands only, any other body is data -- but the command that feeds it to an
        # interpreter must itself be wrapped when the body names a real destination
        m = HEREDOC.search(strip_comment(line))
        body, ns_body = [], False
        if m:
            while i < len(lines) and lines[i][1].strip() != m.group(2):
                body.append(lines[i])
                i += 1
            i += 1
            first = next((b[1].strip() for b in body if b[1].strip() and not b[1].strip().startswith(("#", "set "))), "")
            ns_body = bool(re.match(r'^\.\s+"?\$\{?OPKIT_LIB\}?"?', first))
            names_dest = bool(DEST_LITERAL.search("\n".join(b[1] for b in body)))
        if kit_helper.match(line):           # the helper itself: asserts before every call
            continue
        for cmd in split_simple(strip_comment(line)):
            w = words(cmd)
            if not w:
                continue
            kind, wrapped = command_class(w)
            if m and not kind and not ns_body and names_dest and strip_helper(w)[0] in ("python3", "python", "bash", "sh") and not wrapped:
                kind = "write"
            if kind and not wrapped:
                bad.append(f"{path}:{n}: [{kind}] not itself an ns_run/kit command: {cmd.strip()[:100]}")
        if m and ns_body:
            for bn, bline in body:
                if kit_helper.match(bline):
                    continue
                for cmd in split_simple(strip_comment(bline)):
                    w = words(cmd)
                    if not w:
                        continue
                    kind, wrapped = command_class(w)
                    if kind in ("kit", "--apply", "build") and not wrapped:
                        bad.append(f"{path}:{bn}: [{kind}] in-namespace body, not itself a kit/ns_run command: {cmd.strip()[:100]}")
    if KIT_TOKEN.search(text) and "opkit_ns.sh" not in text:
        bad.append(f"{path}: runs the kit but never uses scripts/lib/opkit_ns.sh")
    return bad


def test_scripts(root):
    for d, _, fs in os.walk(os.path.join(root, "scripts")):
        for f in fs:
            if f.startswith("test_") and f.endswith(".sh"):
                yield os.path.join(d, f)


# ── destinations (amendment 97, a) ───────────────────────────────────────────────────────────────
def shadowed(root):
    lib = open(os.path.join(root, "scripts", "lib", "opkit_ns.sh")).read()
    m = re.search(r'^OPKIT_DEFAULT_DESTS="([^"]+)"', lib, re.M)
    return m.group(1).split() if m else []


def kit_vars(text):
    env = {}
    for _ in range(4):                       # a few passes: later vars name earlier ones
        for m in re.finditer(r'^([A-Z][A-Z0-9_]*)=(\S*)\s*(?:#.*)?$', text, re.M):
            v = m.group(2).strip('"')
            v2 = re.sub(r'\$\{?([A-Z][A-Z0-9_]*)\}?', lambda x: env.get(x.group(1), x.group(0)), v)
            env[m.group(1)] = v2
    return env


def expand(arg, env):
    return re.sub(r'\$\{?([A-Z][A-Z0-9_]*)\}?', lambda x: env.get(x.group(1), x.group(0)), arg)


SCRATCH_VARS = {"WORK", "TREE", "GUEST_STAGE", "CLONE", "TMPDIR", "BIN_DIR", "MANIFEST_SRC", "HOSTBINS"}
DEST_PARAM_FUNCS = {"install_registry": 2}      # function -> which argument is a DESTINATION directory (judged at its call sites)


def kit_write_targets(text):
    """[(line, command, target)] for every write the kit makes, kit variables expanded, `for X in LIST`
    loop variables fanned out, a function's destination parameter judged at its call sites. A target
    that still begins with a lowercase local or a scratch variable is scratch; an UNRESOLVED uppercase
    variable is itself a finding (define it at column 0 so this gate can see where it points)."""
    env = kit_vars(text)
    out, fn, loops = [], None, {}
    for n, line in logical_lines(text):
        s = strip_comment(line)
        m = re.match(r'^(\w+)\(\)\s*\{', s)
        if m:
            fn = m.group(1)
        elif s.startswith("}"):
            fn = None
        if fn in ("act_dir", "act_install", "act_group", "act_user", "pin_of", "stat_ugm", "norm_mode"):
            continue                          # the helpers' own bodies take $p/$dst: their CALLS are judged
        for m in re.finditer(r'\bfor (\w+) in ([^;]+);\s*do', s):
            loops[m.group(1)] = [expand(x.strip('"'), env) for x in shlex_split(m.group(2))]
        for cmd in split_simple(s):
            w = words(cmd)
            if not w:
                continue
            c0, args = w[0], [a for a in w[1:] if not a.startswith("-")]
            tgt = []
            if c0 == "act_dir" and args:
                tgt = [args[0]]
            elif c0 == "act_install" and len(args) > 1:
                tgt = [args[1]]
            elif c0 in DEST_PARAM_FUNCS and len(args) >= DEST_PARAM_FUNCS[c0]:
                tgt = [args[DEST_PARAM_FUNCS[c0] - 1]]
            elif c0 in ("cp", "mv", "install", "ln") and args:
                tgt = args[-1:]               # only the DESTINATION of a copy is written
            elif c0 in ("mkdir", "chown", "chmod", "tee", "touch", "truncate", "rm", "rmdir") and args:
                tgt = args[1:] if c0 in ("chown", "chmod") else args
            elif c0 in ("useradd", "groupadd"):
                tgt = ["/etc/passwd" if c0 == "useradd" else "/etc/group"]
            for t in tgt:
                if fn in DEST_PARAM_FUNCS and re.match(r'^\$\{?(dst|d)\b', t):
                    continue                  # derived from the destination parameter: judged at the callers
                lm = re.match(r'^\$\{?(\w+)\}?(/.*)?$', t)
                if lm and lm.group(1) in loops:
                    for item in loops[lm.group(1)]:
                        out.append((n, cmd.strip()[:90], item + (lm.group(2) or "")))
                    continue
                if fn in DEST_PARAM_FUNCS and re.match(r'^\$\{?(dst|d)\b', t):
                    continue                  # derived from the destination parameter: judged at the callers
                t = expand(t, env)
                hm = re.match(r'^\$\{?(\w+)', t)
                if hm and (hm.group(1) in SCRATCH_VARS or hm.group(1).islower() or hm.group(1)[0].isdigit()):
                    continue
                if t.startswith("$(") or not (t.startswith("/") or t.startswith("$")):
                    continue
                out.append((n, cmd.strip()[:90], t))
        # redirections into a path
        for m in re.finditer(r'>>?\s*"?(\$\{?[A-Za-z_]+\}?[^\s"]*|/[^\s"]+)', s):
            t = expand(m.group(1), env)
            hm = re.match(r'^\$\{?(\w+)', t)
            if t.startswith(("/dev/", "/proc/")) or (hm and (hm.group(1) in SCRATCH_VARS or hm.group(1).islower())):
                continue
            out.append((n, s.strip()[:90], t))
    return out


def shlex_split(x):
    try:
        return shlex.split(x)
    except ValueError:
        return x.split()


def destination_problems(root, kit_text=None, label="operator_deploy_protected_host.sh"):
    dests = shadowed(root)
    if not dests:
        return [f"{label}: lib/opkit_ns.sh names no OPKIT_DEFAULT_DESTS"]
    text = kit_text if kit_text is not None else open(os.path.join(root, "scripts", label)).read()
    bad = []
    for n, cmd, t in kit_write_targets(text):
        if not t.startswith("/"):
            bad.append(f"{label}:{n}: writes {t}, which this gate cannot resolve to an absolute path (define the variable at column 0): {cmd}")
            continue
        if not any(t == d or t.startswith(d + "/") for d in dests):
            bad.append(f"{label}:{n}: writes {t} (not under a shadowed destination {dests}): {cmd}")
    return bad


def check(root):
    bad = []
    for p in test_scripts(root):
        bad += check_text(p, open(p).read())
    fx = os.path.join(root, "scripts", "lib", "opkit_fixture.sh")
    if os.path.exists(fx):
        t = open(fx).read()
        if "opkit_ns_assert" not in t.split("\n", 12)[0:12].__str__():
            bad.append(f"{fx}: an in-namespace script must call opkit_ns_assert in its first lines")
    bad += destination_problems(root)
    return bad


BYPASSES = [
    # (label, text): each must be REFUSED
    ("ns_run true; kit", '. scripts/lib/opkit_ns.sh\nns_run true; bash "$KIT" --apply\n'),
    ("echo ns_run; kit", '. scripts/lib/opkit_ns.sh\necho ns_run; bash "$KIT" --apply\n'),
    ("trailing comment", '. scripts/lib/opkit_ns.sh\nbash "$KIT" --apply # ns_run\n'),
    ("ns_run as an argument", '. scripts/lib/opkit_ns.sh\nbash "$KIT" --apply ns_run\n'),
    ("ns_run as a quoted argument", '. scripts/lib/opkit_ns.sh\nbash "$KIT" "ns_run" --apply\n'),
    ("env prefix naming ns_run", '. scripts/lib/opkit_ns.sh\nx=ns_run bash "$KIT" --apply\n'),
    ("ns_run piped into the kit", '. scripts/lib/opkit_ns.sh\nns_run true | bash "$KIT" --apply\n'),
    ("&& after ns_run", '. scripts/lib/opkit_ns.sh\nns_run true && bash "$KIT" --apply\n'),
    ("command substitution then kit", '. scripts/lib/opkit_ns.sh\n$(ns_run true) ; bash "$KIT" --from x --apply\n'),
    ("if ns_run; then kit", '. scripts/lib/opkit_ns.sh\nif ns_run true; then bash "$KIT" --apply; fi\n'),
    ("kit without --apply, same shapes", '. scripts/lib/opkit_ns.sh\nns_run true; bash "$KIT" --from x\n'),
    ("a function that runs the kit", '. scripts/lib/opkit_ns.sh\nrunit() { bash "$KIT" --apply; }\n'),
    ("a kit COPY applied", '. scripts/lib/opkit_ns.sh\nns_run true; bash "$WORK/other-kit.sh" --apply\n'),
    ("host build outside", '. scripts/lib/opkit_ns.sh\nns_run true; python3 -B scripts/guest_build_env.py host-build "$OUT"\n'),
    ("fixture outside", '. scripts/lib/opkit_ns.sh\nbash scripts/lib/opkit_fixture.sh a b\n'),
    ("a mkdir under /var/lib outside", '. scripts/lib/opkit_ns.sh\nns_run true; mkdir -p /var/lib/axon-x\n'),
    ("a mktemp under /var/lib outside", '. scripts/lib/opkit_ns.sh\nK=$(mktemp -d /var/lib/axon-opkit-keys.XXXXXX)\n'),
]
CONTROLS = [
    ('. scripts/lib/opkit_ns.sh\nns_run bash "$KIT" --from c --apply\n'),
    ('. scripts/lib/opkit_ns.sh\nrefused "x" "y" ns_run bash "$KIT" --from c\n'),
    ('. scripts/lib/opkit_ns.sh\no=$(ns_run bash "$KIT" --from c 2>&1); rc=$?\n'),
    ('. scripts/lib/opkit_ns.sh\nRUSTC_WRAPPER=/usr/bin/true ns_run bash "$KIT" --from c\n'),
    ('. scripts/lib/opkit_ns.sh\nOPKIT_NET=host ns_run bash scripts/lib/opkit_fixture.sh a b\n'),
    ('. scripts/lib/opkit_ns.sh\nns_run mkdir -p /var/lib/axon-x\n'),
    ('. scripts/lib/opkit_ns.sh\npython3 - "$KIT" <<PY\nPY\ncp -- "$KIT" "$WORK/k"\nKIT=$CLONE/k\n'),
]


def selftest(root):
    src = open(os.path.join(root, "scripts", "test_operator_deploy.sh")).read()
    base = check_text("real", src)
    if base:
        print("selftest: the real test is not clean:\n" + "\n".join(base)); return 1
    if destination_problems(root):
        print("selftest: the real kit writes outside the shadowed destinations:\n" + "\n".join(destination_problems(root))); return 1
    # the real file with its first wrapped kit --apply unwrapped
    attack = src.replace('ns_run bash "$KIT" "${ARGS[@]}" --no-systemctl --apply',
                         'bash "$KIT" "${ARGS[@]}" --no-systemctl --apply', 1)
    assert attack != src, "selftest: attack line not found"
    if not check_text("attack", attack):
        print("selftest: an unwrapped kit --apply was ACCEPTED"); return 1
    for label, text in BYPASSES:
        if not check_text("bypass", text):
            print(f"selftest: the bypass shape '{label}' was ACCEPTED"); return 1
    for i, text in enumerate(CONTROLS):
        got = check_text("control", text)
        if got:
            print(f"selftest: control {i} (a wrapped call) was REFUSED: {got}"); return 1
    # destinations: planted kit writes outside the shadowed list are each refused; inside ones accepted
    kit = open(os.path.join(root, "scripts", "operator_deploy_protected_host.sh")).read()
    for label, extra in [
        ("act_dir under /opt", 'act_dir /opt/axon root root 0755\n'),
        ("act_install under /usr/lib/systemd", 'act_install "$x" /usr/lib/systemd/system/axon.service root root 0644\n'),
        ("install -m under /opt", 'install -m 0755 "$x" /opt/bin/axon\n'),
        ("cp under /usr/share", 'cp -- "$x" /usr/share/axon/x\n'),
        ("a redirect into /boot", 'echo x > /boot/axon\n'),
        ("a variable that resolves outside", 'NEWDIR=/opt/axon\nact_dir "$NEWDIR" root root 0755\n'),
        ("tee into /usr/lib", 'printf x | tee /usr/lib/axon.conf\n'),
    ]:
        if not destination_problems(root, kit + "\n" + extra):
            print(f"selftest: a kit write outside the shadowed destinations was ACCEPTED ({label})"); return 1
    for label, extra in [("act_dir under /var/lib", 'act_dir /var/lib/axon-new root root 0755\n'),
                         ("act_install under /etc/systemd", 'act_install "$x" /etc/systemd/system/axon-new.service root root 0644\n')]:
        if destination_problems(root, kit + "\n" + extra):
            print(f"selftest: a kit write inside the shadowed destinations was REFUSED ({label})"); return 1
    print("selftest: ok"); return 0


if __name__ == "__main__":
    a = [x for x in sys.argv[1:] if x != "--selftest"]
    root = a[0] if a else os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
    if "--selftest" in sys.argv:
        sys.exit(selftest(root))
    bad = check(root)
    if bad:
        print("opkit_ns_drift: a violation:\n" + "\n".join(bad)); sys.exit(1)
    print("opkit_ns_drift: ok (every kit run, --apply and controlled-build verb in a test script is itself an ns_run command; every kit write target is under a shadowed destination)")
