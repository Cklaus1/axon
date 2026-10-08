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
  4b. DENY BY MENTION (amendment 101): any simple command that mentions the kit (`$KIT`, a variable
     assigned from it, its file name) or the controlled-build API (`g.begin(` ...) and is not itself an
     ns_run/kit/inns command is a violation unless its first word is a plainly read-only one (cp, cat,
     grep, echo, [, test, python3 - "$KIT" reading it, ...). So `eval`, `ionice`, `stdbuf`, `K=$KIT; bash
     "$K"`, `python3 -c 'g.begin(1)'` and every wrapper nobody listed are refused without being named; an
     assignment that builds a COMMAND STRING out of the kit / `--apply` / a build verb is refused where it
     is made; a variable assigned a real destination path is a destination when a mutator names it;
     a redirection into a real destination is a write even on an ns_run command (the OUTER shell opens it);
  5. `--selftest` plants the bypass shapes in scratch copies and requires every one refused, and
     requires the real scripts and the controls to pass.

Exit 0 clean, 1 a violation.  Usage: opkit_ns_drift.py [--selftest] [ROOT]
"""
import os, re, shlex, sys

KIT_TOKEN = re.compile(r'\$\{?KIT\}?(?![\w])|operator_deploy_protected_host\.sh')
BUILD_VERB = re.compile(r'guest_build_env\.py|opkit_fixture\.sh')
# the python API of guest_build_env.py that STARTS a build step
BUILD_API = re.compile(r'\b\w+\.(begin|cargo_step|run_cargo|host_build|finish|dist_record|rootfs|kernel|discard)\(')
# first words that read, print or compare and run nothing they are handed
BENIGN = {"cp", "cat", "grep", "egrep", "head", "tail", "wc", "sha256sum", "stat", "ls", "test", "[", "[[", "echo", "printf",
          "diff", "cmp", "realpath", "dirname", "basename", "readlink", "for", "case", "fail", "ok", "die", "warn", "true",
          ":", "return", "export", "local", "declare", "readonly", "unset", "set", "shift", "wait", "kill", "trap", "rm", "mv"}
ASSIGN = re.compile(r'^\s*(?:(?:export|local|readonly|declare(?:\s+-\w+)?)\s+)?([A-Za-z_]\w*)=(.*)$')
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


class Ctx:
    """Names the text gives the kit and the real destinations (flow-insensitive, to a fixpoint)."""
    def __init__(self, text=""):
        self.kit = {"KIT"}
        self.real = {"KEYPARENT", "FORGE"}
        self.cmdvars = set()
        real_root = "(?:" + "|".join(re.escape(r) for r in REAL) + ")"
        for _ in range(4):
            for raw in (c for _, ln in logical_lines(text) for c in split_simple(strip_comment(ln))):
                m = ASSIGN.match(raw)
                if not m:
                    continue
                name, val = m.group(1), m.group(2).strip().strip("\"'")
                if name not in self.kit and (self.kit_re().search(val) or re.search(r'operator_deploy_protected_host\.sh', val)):
                    if not re.search(r'--apply|\s', val):
                        self.kit.add(name)
                if re.match(r'^(?:\$\(mktemp\s+(?:-\w+\s+)*)?' + real_root + r'(?:/|$|[.\s"\')])', val) or \
                        any(re.match(r'^\$\{?' + r + r'\}?(?:/|$)', val) for r in self.real):
                    self.real.add(name)

    def kit_re(self):
        names = "|".join(sorted(self.kit))
        return re.compile(r'\$\{?(?:' + names + r')\}?(?![\w])|operator_deploy_protected_host\.sh')

    def real_arg(self, a):
        return any(a == r or a.startswith(r + "/") for r in REAL) or \
            bool(re.search(r'\$\{?(' + "|".join(sorted(self.real)) + r')\b', a))


def command_class(w, ctx=None):
    """(kind, wrapped): what a simple command does that needs the helper, and whether it IS the helper."""
    ctx = ctx or Ctx()
    kit_re = ctx.kit_re()
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
    if "--apply" in rest and cmd0 not in ("echo", "printf", "grep", "[", "[[", "test"):
        kind = "--apply"        # deny by default (amendment 101): any command carrying --apply, whatever its first word
    elif kit_re.search(flat) and (cmd0 in RUNNERS or kit_re.fullmatch(cmd0 or "x")):
        # a kit COPY named differently is caught by its --apply; reading the kit (python3 - "$KIT", cp, sed) runs nothing
        if cmd0 not in ("python3", "python"):
            kind = "kit"
    elif kit_re.search(flat) and cmd0 not in BENIGN and not (cmd0 in ("python3", "python") and rest[1:2] == ["-"]):
        kind = "kit"            # deny by MENTION: eval, ionice, stdbuf, an unlisted wrapper, bash -c, a variable alias
    if kind is None and cmd0 == "eval" and (BUILD_API.search(flat) or ctx.cmdvars and any(("$" + v) in flat or ("${" + v) in flat for v in ctx.cmdvars)):
        kind = "kit"
    if kind is None and cmd0 in ctx.cmdvars | {"$" + v for v in ctx.cmdvars}:
        kind = "kit"
    if kind is None and BUILD_API.search(flat) and cmd0 not in ("echo", "printf", "grep"):
        kind = "build"
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
            ops = [x for x in ops if "/" in x or "$" in x]
        for a in ops:
            if ctx.real_arg(a):
                kind = "write"
    if kind is None and cmd0 in ("unshare",) and "--pid" in rest and any("python" in x for x in rest):
        kind = "build"
    return kind, wrapped


HEREDOC = re.compile(r"<<-?\s*(['\"]?)(\w+)\1\s*$")
DEST_LITERAL = re.compile(r"""\b(makedirs|mkdir|chown|chmod|rename|copy\w*|copytree|move|open|symlink|run|call|write_text|write_bytes|unlink|remove)\(\s*\[?\s*f?["'](/etc|/usr/local|/var/lib|/var/log|/var/spool|/run|/srv|/opt|/usr/lib|/usr/share|/var/cache|/boot|/root|/home|/lib)\b""")
REDIRECT = re.compile(r"""(?:^|[^<>&\d])(?:\d|&)?>>?\s*["']?([^\s"'|&;<>()]+)""")


def redirect_writes(cmd, ctx):
    """Real destinations the OUTER shell writes through a redirection (it opens the file before any wrapper runs)."""
    out = []
    for m in REDIRECT.finditer(cmd):
        t = m.group(1)
        if t.startswith(("/dev/", "/proc/", "&")):
            continue
        if ctx.real_arg(t):
            out.append(t)
    return out


def assigned_command_strings(cmd, ctx):
    """Assignments that build a COMMAND STRING out of the kit / --apply / a build verb (to be eval'd or expanded later)."""
    out = []
    try:
        ws = shlex.split(cmd, posix=True, comments=False)
    except ValueError:
        ws = cmd.split()
    kit_re = ctx.kit_re()
    for x in ws:
        m = re.fullmatch(r'([A-Za-z_]\w*)=(.*)', x, re.S)
        if not m:
            break
        v = m.group(2)
        if (re.search(r'\s', v) and (kit_re.search(v) or re.search(r'--apply', v) or BUILD_API.search(v)
                                      or re.search(r'guest_build_env\.py\s+(begin|cargo|host-build|kernel|rootfs|finish|dist|discard)\b', v)
                                      or "opkit_fixture.sh" in v)):
            out.append(m.group(1))
    return out


def check_text(path, text):
    bad = []
    kit_helper = re.compile(r'^\s*kit\(\)\s*\{\s*opkit_ns_assert\s*\|\|[^;{}]*;\s*bash "\$KIT" "\$@";\s*\}\s*$')
    lines = logical_lines(text)
    ctx = Ctx(text)
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
            for t in redirect_writes(cmd, ctx):
                bad.append(f"{path}:{n}: [write] a redirection into a real destination ({t}); the OUTER shell opens it even on an ns_run command: {cmd.strip()[:100]}")
            for v in assigned_command_strings(cmd, ctx):
                ctx.cmdvars.add(v)
                bad.append(f"{path}:{n}: [kit] {v} is assigned a command string naming the kit / --apply / a build verb; run it with ns_run, not through a variable: {cmd.strip()[:100]}")
            w = words(cmd)
            if not w:
                continue
            kind, wrapped = command_class(w, ctx)
            if not kind and not wrapped and not ns_body:
                # python -c / a heredoc body that names a real destination as a write target
                if strip_helper(w)[0] in ("python3", "python", "bash", "sh") and DEST_LITERAL.search(cmd):
                    kind = "write"
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
                    kind, wrapped = command_class(w, ctx)
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
            # amendment 101 -- belt and braces behind the read-only root: the verbs the first extractor did not know
            elif c0 == "sed" and any(a.startswith("-i") or a == "--in-place" for a in w[1:]) and len(args) > 1:
                tgt = args[1:]
            elif c0 in ("setfacl", "chattr", "chcon", "setcap", "truncate", "shred") and args:
                tgt = args[-1:]
            elif c0 == "dd":
                tgt = [a[3:] for a in w[1:] if a.startswith("of=")]
            elif c0 in ("curl", "wget"):
                tgt = [w[i + 1] for i, a in enumerate(w[:-1]) if a in ("-o", "--output", "-O", "--output-document")]
            elif c0 == "git" and "clone" in w[1:] and args:
                tgt = args[-1:]
            elif c0 == "tar" and any(re.match(r'^-\w*x', a) or a in ("--extract", "x") for a in w[1:]):
                tgt = [w[i + 1] for i, a in enumerate(w[:-1]) if a in ("-C", "--directory")] or ["$unresolved-cwd-extract"]
            elif c0 == "systemctl":
                tgt = [a.split("=", 1)[1] for a in w[1:] if a.startswith("--root=")]
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
        # a python write to a literal path (open(path, "w"), makedirs, rename, ...)
        for m in re.finditer(r"""\b(open|makedirs|mkdir|chown|chmod|rename|copyfile|copy2|copytree|symlink|write_text|write_bytes|unlink|remove)\(\s*f?["'](/[^"']*)["'](\s*,\s*["']([^"']*)["'])?""", s):
            if m.group(1) == "open" and not (m.group(4) and re.search(r'[wax+]', m.group(4))):
                continue
            out.append((n, s.strip()[:90], expand(m.group(2), env)))
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
    # amendment 101: the shapes the round-10 FIELD-ORIGIN reviewer executed past the gate, and the wrappers it did not list
    ("eval of a kit command string", '. scripts/lib/opkit_ns.sh\neval "bash $KIT --apply"\n'),
    ("a command string assigned then eval'd", '. scripts/lib/opkit_ns.sh\nc=\'bash "$KIT" --apply\'\neval "$c"\n'),
    ("a command string assigned then expanded", '. scripts/lib/opkit_ns.sh\nc=\'bash "$KIT" --apply\'\n$c\n'),
    ("the kit through a variable alias", '. scripts/lib/opkit_ns.sh\nK=$KIT; bash "$K" --from c\n'),
    ("the kit through a chained alias", '. scripts/lib/opkit_ns.sh\nK=$KIT\nJ=$K\nbash "$J" --from c\n'),
    ("the kit through an alias of its file name", '. scripts/lib/opkit_ns.sh\nP=scripts/operator_deploy_protected_host.sh\nbash "$P" --from c\n'),
    ("ionice wrapper", '. scripts/lib/opkit_ns.sh\nionice -c3 bash "$KIT" --from c\n'),
    ("stdbuf wrapper", '. scripts/lib/opkit_ns.sh\nstdbuf -o0 bash "$KIT" --from c\n'),
    ("chrt wrapper", '. scripts/lib/opkit_ns.sh\nchrt -i 0 bash "$KIT" --from c\n'),
    ("taskset wrapper", '. scripts/lib/opkit_ns.sh\ntaskset 1 bash "$KIT" --from c\n'),
    ("nice wrapper", '. scripts/lib/opkit_ns.sh\nnice -n 5 bash "$KIT" --from c\n'),
    ("timeout wrapper", '. scripts/lib/opkit_ns.sh\ntimeout 60 bash "$KIT" --from c\n'),
    ("env wrapper", '. scripts/lib/opkit_ns.sh\nenv -i bash "$KIT" --from c\n'),
    ("setsid wrapper", '. scripts/lib/opkit_ns.sh\nsetsid bash "$KIT" --from c\n'),
    ("sudo wrapper", '. scripts/lib/opkit_ns.sh\nsudo bash "$KIT" --from c\n'),
    ("command wrapper", '. scripts/lib/opkit_ns.sh\ncommand bash "$KIT" --from c\n'),
    ("exec wrapper", '. scripts/lib/opkit_ns.sh\nexec bash "$KIT" --from c\n'),
    ("nohup wrapper", '. scripts/lib/opkit_ns.sh\nnohup bash "$KIT" --from c\n'),
    ("xargs wrapper", '. scripts/lib/opkit_ns.sh\necho x | xargs bash "$KIT"\n'),
    ("su -c wrapper", '. scripts/lib/opkit_ns.sh\nsu -c "bash $KIT --from c" root\n'),
    ("systemd-run wrapper", '. scripts/lib/opkit_ns.sh\nsystemd-run --wait bash "$KIT" --from c\n'),
    ("bash -c string", '. scripts/lib/opkit_ns.sh\nbash -c "$KIT --from c"\n'),
    ("python3 -c calling the build API", '. scripts/lib/opkit_ns.sh\npython3 -c "import guest_build_env as g; g.begin(1)"\n'),
    ("a mkdir through a variable holding a real path", '. scripts/lib/opkit_ns.sh\nD=/var/lib/axon-x\nmkdir -p "$D"\n'),
    ("a mkdir through a variable derived from a real path", '. scripts/lib/opkit_ns.sh\nD=/var/lib/axon-x\nE=$D/sub\nmkdir -p "$E"\n'),
    ("a redirect into /etc", '. scripts/lib/opkit_ns.sh\necho x > /etc/axon/y\n'),
    ("an append into /usr/local", '. scripts/lib/opkit_ns.sh\necho x >>/usr/local/bin/y\n'),
    ("a redirect into a real path on an ns_run command (the outer shell opens it)", '. scripts/lib/opkit_ns.sh\nns_run true > /etc/axon/y\n'),
    ("python -c writing a file under /etc", '. scripts/lib/opkit_ns.sh\npython3 -c "open(\'/etc/axon/x\',\'w\').write(\'x\')"\n'),
    ("an unlisted wrapper on a differently named kit copy", '. scripts/lib/opkit_ns.sh\nionice -c3 bash "$WORK/other-kit.sh" --from c --apply\n'),
    ("python -c writing under /opt", '. scripts/lib/opkit_ns.sh\npython3 -c "open(\'/opt/x\',\'w\')"\n'),
]
CONTROLS = [
    ('. scripts/lib/opkit_ns.sh\nns_run bash "$KIT" --from c --apply\n'),
    ('. scripts/lib/opkit_ns.sh\nrefused "x" "y" ns_run bash "$KIT" --from c\n'),
    ('. scripts/lib/opkit_ns.sh\no=$(ns_run bash "$KIT" --from c 2>&1); rc=$?\n'),
    ('. scripts/lib/opkit_ns.sh\nRUSTC_WRAPPER=/usr/bin/true ns_run bash "$KIT" --from c\n'),
    ('. scripts/lib/opkit_ns.sh\nOPKIT_NET=host ns_run bash scripts/lib/opkit_fixture.sh a b\n'),
    ('. scripts/lib/opkit_ns.sh\nns_run mkdir -p /var/lib/axon-x\n'),
    ('. scripts/lib/opkit_ns.sh\npython3 - "$KIT" <<PY\nPY\ncp -- "$KIT" "$WORK/k"\nKIT=$CLONE/k\n'),
    ('. scripts/lib/opkit_ns.sh\nK=$KIT\nns_run bash "$K" --from c\n'),
    ('. scripts/lib/opkit_ns.sh\necho "$KIT"\n[ -f "$KIT" ] && grep -q x "$KIT"\n'),
    ('. scripts/lib/opkit_ns.sh\nD=/var/lib/axon-x\nns_run mkdir -p "$D"\n'),
    ('. scripts/lib/opkit_ns.sh\necho x > "$WORK/out" 2>/dev/null\nns_run true >"$WORK/o" 2>&1\n'),
    ('. scripts/lib/opkit_ns.sh\nOPKIT_NET=host ns_run python3 -c "import guest_build_env as g; g.begin(1)"\n'),
    ('. scripts/lib/opkit_ns.sh\nc=$(ns_run true)\neval "$c"\n'),
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
        ("a python open() for writing under /opt", 'python3 -c "open(\'/opt/axon/x\', \'w\').write(\'x\')"\n'),
        ("sed -i under /opt", 'sed -i s/a/b/ /opt/axon/x\n'),
        ("dd of= under /home", 'dd if=/dev/zero of=/home/x count=1\n'),
        ("setfacl under /opt", 'setfacl -m u:root:rw /opt/axon/x\n'),
        ("curl -o under /opt", 'curl -o /opt/axon/x http://example.invalid/\n'),
        ("git clone into /opt", 'git clone http://example.invalid/r /opt/axon/r\n'),
        ("tar -x -C /opt", 'tar -x -C /opt/axon -f "$x"\n'),
        ("systemctl --root=/opt", 'systemctl --root=/opt enable axon\n'),
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
