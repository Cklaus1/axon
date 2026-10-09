#!/usr/bin/env python3
"""opkit_ns_drift.py -- a BEST-EFFORT SECOND LAYER that flags the shapes in which a test script runs the operator
deployment kit, a controlled-build verb, a verb of HOST_VERBS (mount, useradd, ...) or a verb of MUTATORS (mkdir, cp, ...) aimed
at a real destination, outside the namespace helper (amendments 92, 97, 101, 105, 109, 113).

EXACTLY WHICH VERBS (amendment 113). Earlier wording said "any other state-changing verb"; that was broader than the gate. In a
TEST SCRIPT the gate flags HOST_VERBS (whatever their operands) and MUTATORS whose operand is a real destination. It does NOT
flag `sed -i`, `tar -x -C`, `rsync`, `dd of=`, `curl -o`, `git clone/checkout/apply`, `patch`, `chattr` or `setfacl` aimed at
a real destination in a test script (some are HOST_VERBS and so flagged, but rsync, sed -i, tar, dd, curl, git, patch are not).
The KIT's own write targets are held to a wider list (`destination_problems`: its `sed -i`, `tar -x -C`, `dd of=`, `curl -o`,
`git clone` ... under a real destination are flagged). What holds for the rest is the namespace, not this file.

WHAT THIS IS AND IS NOT (amendment 109). It is a textual gate: it flags the shapes it lists (the must-flag shapes of
`--selftest`, which prints their count) and the mentions it derives from them, and it keeps the real test scripts clean. It is
NOT deny-by-default and NOT a guarantee that no text can run the kit: amendment 105 called it that, and a reviewer then ran
fourteen further shapes past it (an alias made by a command substitution, `command -p`, `${P@P}`, a decoded string, a
script written by the test and then run, a fake heredoc marker inside an assignment ...). The boundary that matters is the
NAMESPACE plus the helper's proof (scripts/lib/opkit_ns.sh): inside it the kit has nothing real to write. A test script that
wants to evade a textual gate can; the gate catches mistakes, not intent.

Incident 2026-10-06: a guard-removal experiment made a kit refusal test a REAL root `--apply` on the dev host. The structural
cure is scripts/lib/opkit_ns.sh (`ns_run`: private mount/PID/UTS/IPC/NET namespaces, tmpfs over every destination, isolation
PROVED before the command starts). This gate keeps it the usual door:

  1. every SIMPLE COMMAND, in any test script, that RUNS the kit (`$KIT`,
     `operator_deploy_protected_host.sh`), carries `--apply`, or runs a controlled-build verb
     (`guest_build_env.py begin|cargo|host-build|...`, the fixture script) must
     itself BE an `ns_run`/`kit`/`inns` command: the line is split into simple commands
     at `; && || | & $( ( ` ` { } then do else`, comments dropped, and the wrapper must be
     the command's first word (after VAR=value prefixes, and after the `refused LABEL
     PATTERN` test helper). `ns_run true; bash "$KIT" --apply` has an `ns_run` on the
     line and is REFUSED: the second command is not inside it (amendment 97, f);
  1b. DESTRUCTIVE HELPER PRIMITIVES (amendment 113). The helper's own primitives -- every function of scripts/lib/opkit_ns.sh that
     mounts, umounts, pivots or calls mount_setattr, plus opkit_ns_drop_host_fd (derived from the file, united with the five named in PRIM_BASE) -- change the
     mount table of whatever namespace they run in. A test script (other than the helper's own self-test child block, which
     unshares first) must not call one bare, nor mention one in a command that is not an ns_run command; and the helper must make
     each one start with `opkit_ns_precondition NAME || return 97` (the check that this is not the host's mount namespace,
     BEFORE any mount). A new primitive that mounts without it fails HERE;
  2. a state-changing command (mkdir, mktemp, rm, cp, install, chown, chmod, ln, mv, tee,
     useradd, groupadd) whose operand is under a real destination is held to the same rule;
  3. the kit-running scripts must source/use opkit_ns.sh at all;
  4. DESTINATIONS (amendment 97, a): every write target the kit makes -- `act_dir`/`act_install` operands, and the operands of
     its own mkdir/install/cp/mv/ln/chown/chmod/tee/useradd/groupadd -- resolves (kit variables expanded) under a destination
     ns_run shadows (OPKIT_DEFAULT_DESTS in lib/opkit_ns.sh, the one list). A new kit write to /opt or /usr/lib/systemd fails
     HERE instead of reaching the host;
  4b. REFUSE BY MENTION (amendment 101): any simple command that mentions the kit (`$KIT`, a variable assigned from it, its
     file name) or the controlled-build API (`g.begin(` ...) and is not itself an ns_run/kit/inns command is a violation
     unless its first word is a plainly read-only one (cp, cat, grep, echo, [, test, ...). So `eval`, `ionice`, `stdbuf`,
     `K=$KIT; bash "$K"`, `python3 -c 'g.begin(1)'` and every wrapper nobody listed are refused without being named; an
     assignment that builds a COMMAND STRING out of the kit / `--apply` / a build verb is refused where it is made; a variable
     assigned a real destination path is a destination when a mutator names it; a redirection into a real destination is a
     write even on an ns_run command (the OUTER shell opens it);
  4c. THE CONSERVATIVE LAYER (amendment 109), in a file that mentions the helper, the kit, `--apply` or a build verb: quote
     tricks inside a flag-like word are normalised first; `eval`, `source`, `.`, `bash -c`, `sh -c`, `command`, `builtin`,
     `alias` whose operand is not a plain literal (an `$(ns_run ...)` result is the one exception); an expansion in the
     command word; `${x@P}` and a transformation or `${!x}` handed to an interpreter; a decoding utility on a line that also
     feeds a shell; a file the script wrote that it later runs or sources; heredocs recognised by real syntax only (an
     `<<EOF` inside a quote or an assignment is not a heredoc);
  5. `--selftest` plants the bypass shapes in scratch copies and requires every one refused, and requires the real scripts
     and the controls to pass.

Exit 0 clean, 1 a violation.  Usage: opkit_ns_drift.py [--selftest | --check-quoted-counts] [ROOT]
"""
import codecs, fnmatch, os, re, shlex, sys

KIT_TOKEN = re.compile(r'\$\{?KIT\}?(?![\w])|operator_deploy_protected_host\.sh')
BUILD_VERB = re.compile(r'guest_build_env\.py|opkit_fixture\.sh')
# the python API of guest_build_env.py that STARTS a build step
BUILD_API = re.compile(r'\b\w+\.(begin|cargo_step|run_cargo|host_build|finish|dist_record|rootfs|kernel|discard)\(')
# first words that read, print or compare and run nothing they are handed
BENIGN = {"cp", "cat", "grep", "egrep", "head", "tail", "wc", "sha256sum", "stat", "ls", "test", "[", "[[", "echo", "printf",
          "diff", "cmp", "realpath", "dirname", "basename", "readlink", "for", "case", "fail", "ok", "die", "warn", "true",
          ":", "return", "unset", "set", "shift", "wait", "kill", "rm", "mv"}
# amendment 105: words that carry a STRING which runs later (trap, export/declare/local of a command string) or that
# hand their input to a shell are never plainly read-only when they mention the kit
STRING_CARRIERS = {"trap", "export", "declare", "local", "readonly", "typeset"}
SHELLS = {"bash", "sh", "dash", "zsh", "ksh", "ash", "source", ".", "eval"}
INTERPRETERS = {"python3", "python", "perl", "ruby", "node", "php", "lua", "awk", "gawk", "Rscript", "tclsh"}
# verbs that change the HOST (or its devices) whatever their operands; allowed only inside ns_run
HOST_VERBS = {"mount", "umount", "swapon", "swapoff", "losetup", "mknod", "mkfs", "mkswap", "fdisk", "sfdisk", "parted",
              "sysctl", "modprobe", "insmod", "rmmod", "useradd", "userdel", "usermod", "groupadd", "groupdel", "groupmod",
              "chpasswd", "passwd", "service", "iptables", "nft", "reboot", "shutdown", "halt", "poweroff", "setenforce",
              "chroot", "pivot_root", "wipefs", "blkdiscard", "hdparm", "dmsetup", "cryptsetup", "visudo", "crontab",
              "update-alternatives", "dpkg", "apt", "apt-get", "snap", "setcap", "setfacl", "chattr", "chcon", "ldconfig"}
SYSTEMCTL_READONLY = {"status", "show", "is-active", "is-enabled", "is-failed", "cat", "list-units", "list-unit-files", "list-sockets", "list-timers"}
# the words that run what they are given as a PROGRAM text
EXEC_PRIMITIVES = re.compile(r'child_process|execSync|spawnSync|IO\.popen|Kernel|subprocess|os\.system|os\.exec|os\.spawn|os\.popen|Popen|pty\.spawn|check_call|check_output|\bsystem\s*\(|\bexec\s*\(|\bexecv|`')
SANCTIONED_VARS = {"WORK", "TMPDIR", "STASHDIR", "W", "T", "CLONE", "KEYSTASH", "FORGESTASH", "HOSTOUT", "DIST", "OPKIT_SCRATCH", "OUT", "UNITS", "BIN", "HERE", "REPO"}
SANCTIONED_LITERALS = ("/tmp/", "/var/tmp/", "/dev/null", "/dev/stderr", "/dev/stdout", "/dev/fd/", "/proc/self/")
KIT_FILE = "operator_deploy_protected_host.sh"
# the helper's internal knobs: a test script other than the helper's own self-test never sets or reads them
INTERNAL_KNOBS = re.compile(r'OPKIT_(?:DROP_CAPS|BSET|STAMP|HOST_FD|HOST_NS|KEEP_FDS|RW_ROOTS_FOR_TEST)')
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

# amendment 113: the helper's DESTRUCTIVE primitives. Derived from scripts/lib/opkit_ns.sh (every function other than ns_run that
# mounts, umounts, pivots or calls mount_setattr) and unioned with these five (opkit_ns_drop_host_fd closes the host-root descriptor
# and mounts nothing, so only the union names it), so a primitive that is
# renamed or whose body is rewritten out of the pattern is still named.
PRIM_BASE = {"opkit_ns_isolate", "opkit_ns_make_ro", "opkit_ns_fresh_proc", "opkit_ns_private_dev", "opkit_ns_drop_host_fd"}
PRECONDITION = "opkit_ns_precondition"
PRIM_OUTER = {"ns_run"}                    # unshares BEFORE it mounts: the one sanctioned entry
HELPER_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "lib", "opkit_ns.sh")
_MOUNTING = re.compile(r'(?:^\s*|[;&|{(]\s*|\b(?:then|do|else)\s+)(?:mount|umount|pivot_root)\b|mount_setattr')
_FIRST = re.compile(r'^\s*' + PRECONDITION + r'\s+(\S+)\s*\|\|\s*return\s+97\b')
_prims_override = None


def helper_functions(text):
    """{name: [(lineno, line without its comment)]} for the top-level `name() {` ... `}` functions of the helper."""
    out, cur = {}, None
    for n, ln in enumerate(text.splitlines(), 1):
        m = re.match(r'^([A-Za-z_]\w*)\(\)\s*\{', ln)
        if m and cur is None:
            cur = m.group(1)
            out[cur] = []
            rest = ln[m.end():]
            if rest.strip():
                out[cur].append((n, re.sub(r'\s#.*$', '', rest)))
            continue
        if cur is not None:
            if ln == "}":
                cur = None
            else:
                out[cur].append((n, ln if not ln.lstrip().startswith("#") else ""))
    return out


def derived_primitives(text):
    return {name for name, body in helper_functions(text).items()
            if name not in PRIM_OUTER and any(_MOUNTING.search(re.sub(r'\s#.*$', '', l)) for _, l in body)}


def prims():
    if _prims_override is not None:
        return _prims_override
    try:
        return derived_primitives(open(HELPER_PATH).read()) | PRIM_BASE
    except OSError:
        return set(PRIM_BASE)


def primitive_problems(root, text=None, label="scripts/lib/opkit_ns.sh"):
    """The helper side of 1b: every primitive starts with `opkit_ns_precondition NAME || return 97`, the five named ones exist."""
    if text is None:
        text = open(os.path.join(root, "scripts", "lib", "opkit_ns.sh")).read()
    fns = helper_functions(text)
    bad = []
    if PRECONDITION not in fns:
        bad.append(f"{label}: {PRECONDITION} is not defined")
    for name in sorted(PRIM_BASE - set(fns)):
        bad.append(f"{label}: the destructive primitive {name} is not defined (PRIM_BASE names it)")
    for name in sorted((derived_primitives(text) | (PRIM_BASE & set(fns))) - {PRECONDITION}):
        body = [(n, l) for n, l in fns[name] if l.strip() and not re.match(r'^\s*local\b', l)]
        first = body[0] if body else None
        m = _FIRST.match(first[1]) if first else None
        if not m or m.group(1) != name:
            bad.append(f"{label}:{first[0] if first else '?'}: {name} mounts or closes a host descriptor but its FIRST statement is not "
                       f"`{PRECONDITION} {name} || return 97`: {(first[1].strip() if first else '')[:80]}")
    return bad


def prim_called(w):
    """A bare call of a destructive primitive, or a mention of one in a command that is not read-only (bash -c '...', env, sudo ...)."""
    ws = strip_helper(w)
    if not ws:
        return None
    ps = prims()
    if ws[0] in ps:
        return ws[0]
    if ws[0] in BENIGN or ws[0] in ("echo", "printf", "grep", "egrep"):
        return None
    flat = " ".join(ws)
    for p in sorted(ps):
        if re.search(r'(?<![\w])' + re.escape(p) + r'(?![\w])', flat):
            return p
    return None


_HEREDOC_RE = re.compile(r"<<-?[ \t]*(['\"]?)([^\s;&|<>()'\"]+)\1")


class _Heredoc:
    def __init__(self, delim):
        self.delim = delim

    def group(self, i):
        return self.delim if i == 2 else None


class _HeredocFinder:
    """Amendment 109: a heredoc operator is found by REAL syntax: `<<` outside every quote, not `<<<`, not `<<(`, in a
    command that has a word of its own (so `X="<<EOF"`, `echo "<<EOF"`, `X=<<EOF` are not heredocs and the lines after them
    are judged as the commands they are). The old search matched `<<EOF` anywhere in the line, so a fake marker in an
    assignment made the next lines 'data'."""

    def search(self, line):
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
            elif c == "<" and line.startswith("<<", i) and not line.startswith("<<<", i) and (i == 0 or line[i - 1] != "<"):
                m = _HEREDOC_RE.match(line, i)
                prefix = line[:i]
                last = re.split(r'\|\||&&|;|\||&|\(|\{|\$\(|`', prefix)[-1]
                try:
                    pw = shlex.split(last, posix=True, comments=False)
                except ValueError:
                    pw = last.split()
                while pw and re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*=.*', pw[0]):
                    pw = pw[1:]
                if m and (pw or line[m.end():].strip()) and not re.search(r'=\s*$', prefix):
                    return _Heredoc(m.group(2))
            i += 1
        return None


HEREDOC_ANY = _HeredocFinder()


def quote_state(text, q=None):
    """The open quote (or None) at the end of `text`, scanned like strip_comment: a comment is only a comment outside quotes."""
    i = 0
    while i < len(text):
        c = text[i]
        if q:
            if c == q:
                q = None
            elif c == "\\" and q == '"':
                i += 1
        elif c in "'\"":
            q = c
        elif c == "\\":
            i += 1
        elif c == "#" and (i == 0 or text[i - 1] in " \t;&|(){}"):
            return None
        i += 1
    return q


def logical_lines(text):
    """(first physical line number, joined text): backslash-continued lines joined; a line that ends INSIDE a quote
    is joined with the following ones until the quote closes (a multi-line `bash -c '...'` is ONE argument of one
    command, not a column of commands); heredoc BODIES stay line by line (an in-namespace script's body is judged
    like any other shell). A quote that never closes is not merged (nothing is swallowed into one line)."""
    phys = text.splitlines()
    out, k = [], 0
    while k < len(phys):
        start, cur = k + 1, ""
        while k < len(phys):
            raw = phys[k]
            k += 1
            if raw.rstrip().endswith("\\") and not raw.rstrip().endswith("\\\\"):
                cur += raw.rstrip()[:-1] + " "
                continue
            cur += raw
            break
        q = quote_state(cur)
        if q is not None:
            j, joined = k, cur
            while j < len(phys) and q is not None:
                joined += "\n" + phys[j]
                j += 1
                q = quote_state(joined)
            if q is None:
                cur, k = joined, j
        out.append((start, cur))
        hm = HEREDOC_ANY.search(strip_comment(cur))
        if hm:
            end = next((j for j in range(k, len(phys)) if phys[j].strip() == hm.group(2)), None)
            if end is not None:
                for j in range(k, end + 1):
                    out.append((j + 1, phys[j]))
                k = end + 1
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
        self.applyvars = set()
        self.kitcopy = set()                  # the literal text a copy of the kit was written to
        self.nsvars = set()                   # variables assigned from a `$(ns_run ...)` substitution (their text came out of the namespace)
        self.written = set()                  # amendment 109: files the test script itself writes (normalised), judged if later executed
        self.safe = set(SANCTIONED_VARS)      # scratch variables: assigned from mktemp / a sanctioned variable / a temp root
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

        # whole logical lines (a `$(` splits a simple command, so a value built by a substitution is seen here)
        for _ in range(4):
            for _, ln in logical_lines(text):
              for seg in re.split(r';|&&|\|\|', strip_comment(ln)):
                m = ASSIGN.match(seg)
                if not m:
                    continue
                name, val = m.group(1), m.group(2).strip()
                dq = val.strip("\"'")
                if name not in self.kit and re.fullmatch(r'["\']?(?:\$\((?:echo|printf %s|printf "%s"|cat)\s+[^|;&()]*\)|`(?:echo|printf %s|cat)\s+[^|;&`]*`)["\']?', val) \
                        and self.kit_re().search(val):
                    self.kit.add(name)             # amendment 109: K=$(echo $KIT) is the kit under another name
                if re.match(r'^["\']?\$\((?:refused\s+\S+\s+\S+\s+)?(?:ns_run|kit|inns)\b', val):
                    self.nsvars.add(name)
                if re.search(r'(?:^|[\s=])-{0,2}apply(?:\s|$)', dq):
                    self.applyvars.add(name)
                if re.match(r'^(?:\$\((?:mktemp)\s+(?:-\w+\s+)*)?["\']?(?:\$\{?(?:' + "|".join(sorted(self.safe)) + r')\}?|\$\{TMPDIR:-/var/tmp\}|/tmp/|/var/tmp/)', val):
                    self.safe.add(name)
                if re.match(r'^\$\(mktemp\s+(?:-\w+\s+)*["\']?\$\{TMPDIR:-/var/tmp\}', val):
                    self.safe.add(name)
              # an env prefix anywhere on the line (`HF=$W/hostfile bash -c ...`)
              for m3 in re.finditer(r'(?:^|[\s(])([A-Za-z_]\w*)=(["\']?\$\{?(?:' + "|".join(sorted(self.safe)) + r')\}?(?:/[^\s"\']*)?)', strip_comment(ln)):
                  if ".." not in m3.group(2).split("/"):
                      self.safe.add(m3.group(1))
        for raw in (c for _, ln in logical_lines(text) for c in split_simple(strip_comment(ln))):
            try:
                ws = shlex.split(raw, posix=True, comments=False)
            except ValueError:
                ws = raw.split()
            while ws and re.fullmatch(r'[A-Za-z_]\w*=.*', ws[0]):
                ws = ws[1:]
            if ws and ws[0] in ("cp", "mv", "ln", "install", "rsync") and len(ws) > 2:
                ops = [x for x in ws[1:] if not x.startswith("-")]
                if len(ops) >= 2 and (self.kit_re().search(" ".join(ops[:-1])) or KIT_FILE in " ".join(ops[:-1])):
                    self.kitcopy.add(ops[-1])

    def kit_re(self):
        names = "|".join(sorted(self.kit))
        lit = "".join("|" + re.escape(x) for x in sorted(self.kitcopy) if x)
        return re.compile(r'\$\{?(?:' + names + r')\}?(?![\w])|operator_deploy_protected_host\.sh' + lit)

    def real_arg(self, a):
        return any(a == r or a.startswith(r + "/") for r in REAL) or \
            bool(re.search(r'\$\{?(' + "|".join(sorted(self.real)) + r')\b', a))


def split_redirs(ws):
    """(operands, redirection targets) of a command's words: `> f`, `2>f`, `>>f`, `< f`, `<<EOF`, `<<<x`, `&>f`."""
    args, targets, i = [], [], 0
    while i < len(ws):
        x = ws[i]
        m = re.fullmatch(r'(\d*|&)([<>]{1,3})(&?\d*-?)(.*)', x)
        if m and (m.group(2) or m.group(4)):
            if m.group(4):
                targets.append(m.group(4))
            elif m.group(3) == "" and i + 1 < len(ws):
                targets.append(ws[i + 1]); i += 1
            i += 1
            continue
        args.append(x)
        i += 1
    return args, targets


def sanctioned(a, ctx):
    """A path operand that is scratch: under /tmp or /var/tmp, /dev/null..., or a scratch variable."""
    a = a.strip("\"'")
    if ".." in a.split("/"):
        return False                       # $WORK/../../etc is not scratch
    if a.startswith(SANCTIONED_LITERALS) or a in ("/dev/null", "-"):
        return True
    m = re.match(r'^\$\{?([A-Za-z_]\w*)(?:[:?+-][^}]*)?\}?(?:/|$)', a)
    if m and (m.group(1) in ctx.safe):
        return True
    if re.match(r'^\$\{TMPDIR:-[^}]*\}/', a):
        return True
    return False


def extra_kind(rest, cmd0, flat, ctx):
    """Amendment 105, DENY BY DEFAULT: what an UNWRAPPED command may not be, beyond mentioning the kit."""
    args, _ = split_redirs(rest)
    # an --apply carried in a variable (`A=--apply; bash "$W/k" $A`)
    for v in sorted(ctx.applyvars):
        if re.search(r'\$\{?' + re.escape(v) + r'(?![\w])', flat) and cmd0 not in ("echo", "printf", "grep", "[", "[[", "test"):
            return "--apply"
    # a glob in the kit's file name (`scripts/operator_deploy_*.sh`)
    for x in rest:
        base = os.path.basename(x.strip("\"'"))
        if any(c in base for c in "*?[") and len(re.sub(r"[*?\[\]]", "", base)) >= 4 and fnmatch.fnmatch(KIT_FILE, base):
            return "kit"
    # a shell that reads its program from STDIN (a pipe, a heredoc, a here-string, a process substitution)
    if cmd0 in SHELLS:
        opts = [a for a in args[1:] if a.startswith("-")]
        operands = [a for a in args[1:] if not a.startswith("-")]
        if cmd0 in ("bash", "sh", "dash", "zsh", "ksh", "ash") and "-c" not in opts and not any(o.startswith("-") and "c" in o[1:] and not o.startswith("--") for o in opts) and not operands:
            return "kit"
        if cmd0 in (".", "source") and not operands:
            return "kit"
    if cmd0 in HOST_VERBS or any(cmd0.startswith(v + ".") for v in ("mkfs",)):
        return "write"
    if cmd0 == "systemctl" and (not args[1:] or [a for a in args[1:] if not a.startswith("-")][:1] and [a for a in args[1:] if not a.startswith("-")][0] not in SYSTEMCTL_READONLY):
        return "write"
    if cmd0 == "dd" and any(a.startswith("of=") and not sanctioned(a[3:], ctx) for a in args[1:]):
        return "write"
    if cmd0 == "cd" and args[1:] and ctx.real_arg(args[1]):
        return "write"
    if cmd0 == "git" and "-C" in args and not sanctioned(args[args.index("-C") + 1] if args.index("-C") + 1 < len(args) else "", ctx):
        return "write"
    # an interpreter whose program text names a real destination or executes something: it is not data
    if cmd0 in INTERPRETERS and (EXEC_PRIMITIVES.search(flat) or re.search(r"(?<![\w/.$-])(?:" + "|".join(re.escape(r) for r in REAL) + r")(?![\w-])", flat)):
        return "write"          # a program text that executes something, or names a real destination (python -c, perl -e ...)
    # a shell given a program with -c: the program is judged command by command, as if it stood alone
    if cmd0 in ("bash", "sh", "dash", "zsh", "ksh", "ash") and "-c" in rest:
        i = rest.index("-c")
        prog = rest[i + 1] if i + 1 < len(rest) else ""
        for _, ln in logical_lines(prog):
            for c2 in split_simple(strip_comment(ln)):
                w2 = words(c2)
                if not w2:
                    continue
                k2, wrapped2 = command_class(w2, ctx)
                if (k2 and not wrapped2) or (not wrapped2 and redirect_writes(c2, ctx)):
                    return k2 or "write"
    # a state-changing command whose operand is not provably scratch
    if cmd0 in MUTATORS:
        ops = [x for x in args[1:] if not x.startswith("-")]
        if cmd0 in ("cp", "mv", "install", "ln"):
            ops = ops[-1:]
        elif cmd0 in ("chown", "chmod"):
            ops = ops[1:]
        for a in ops:
            if not sanctioned(a, ctx):
                return "write"
    return None


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
    if re.search(r'--apply', flat) and cmd0 not in ("echo", "printf", "grep", "[", "[[", "test"):
        kind = "--apply"        # deny by default (amendment 101): any command carrying --apply, whatever its first word
    elif kit_re.search(flat) and (cmd0 in RUNNERS or kit_re.fullmatch(cmd0 or "x")):
        # amendment 105: python reading the kit is no exception either (`python3 - "$KIT"` ran it in the round-11 probe)
        kind = "kit"
    elif kit_re.search(flat) and cmd0 not in BENIGN:
        kind = "kit"            # deny by MENTION: eval, ionice, stdbuf, an unlisted wrapper, bash -c, trap, export, a variable alias
    if kind is None and not wrapped:
        kind = extra_kind(rest, cmd0, flat, ctx)
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


HEREDOC = HEREDOC_ANY
DEST_LITERAL = re.compile(r"""\b(makedirs|mkdir|chown|chmod|rename|copy\w*|copytree|move|open|symlink|run|call|write_text|write_bytes|unlink|remove)\(\s*\[?\s*f?["'](/etc|/usr/local|/var/lib|/var/log|/var/spool|/run|/srv|/opt|/usr/lib|/usr/share|/var/cache|/boot|/root|/home|/lib)\b""")
REDIRECT = re.compile(r"""(?:^|[^<>&\d])(?:\d|&)?>>?\s*["']?([^\s"'|&;<>()]+)""")


def outer_redirect_targets(cmd):
    """The targets of the `>` / `>>` / `&>` redirections the OUTER shell opens: found outside quotes only (a `>` inside a
    quoted `bash -c '...'` is opened by the shell INSIDE the command, which an ns_run command runs in the namespace)."""
    out, i, q = [], 0, None
    while i < len(cmd):
        c = cmd[i]
        if q:
            if c == q:
                q = None
            elif c == "\\" and q == '"':
                i += 1
        elif c in "'\"":
            q = c
        elif c == "\\":
            i += 1
        elif c == ">" and not (i > 0 and cmd[i - 1] in "=-") and not cmd.startswith(">(", i):
            j = i + 1
            while j < len(cmd) and cmd[j] in ">|":
                j += 1
            if j < len(cmd) and cmd[j] == "&":
                j += 1
                if j < len(cmd) and (cmd[j].isdigit() or cmd[j] == "-"):
                    i = j
                    continue
            while j < len(cmd) and cmd[j] in " \t":
                j += 1
            k, tq = j, None
            while k < len(cmd):
                ch = cmd[k]
                if tq:
                    if ch == tq:
                        tq = None
                elif ch in "'\"":
                    tq = ch
                elif ch in " \t;&|<>(){}":
                    break
                k += 1
            tok = cmd[j:k].replace('"', "").replace("'", "")
            if tok:
                out.append(tok)
            i = k
            continue
        i += 1
    return out


def redirect_writes(cmd, ctx):
    """Destinations the OUTER shell writes through a redirection (it opens the file before any wrapper runs): a real
    destination, or (amendment 105, deny by default) anything that is not provably scratch."""
    out = []
    for t in outer_redirect_targets(cmd):
        if t.startswith(("/dev/", "/proc/", "&")):
            continue
        if ctx.real_arg(t):
            out.append(t)
        elif not sanctioned(t, ctx) and not re.fullmatch(r'\$?\{?\d+\}?', t) and not t.startswith(("$(", "`")):
            out.append(t)          # a target that is not provably scratch
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
        if x in STRING_CARRIERS or (x.startswith("-") and len(x) < 4):
            continue                       # `export X="bash $KIT --apply"`, `declare -x c=...`, `local c=...`
        m = re.fullmatch(r'([A-Za-z_]\w*)=(.*)', x, re.S)
        if not m:
            break
        v = m.group(2)
        if (re.search(r'\s', v) and (kit_re.search(v) or re.search(r'--apply', v) or BUILD_API.search(v)
                                      or re.search(r'guest_build_env\.py\s+(begin|cargo|host-build|kernel|rootfs|finish|dist|discard)\b', v)
                                      or "opkit_fixture.sh" in v)):
            out.append(m.group(1))
    return out


def in_namespace_body(body, ctx):
    """A heredoc body that is a script for ns_run: it sources the helper and calls opkit_ns_assert BEFORE its first
    line that names the kit, --apply or a build verb (amendment 105: the old test looked at the first line only,
    so a body that began with an assignment was data and was never judged)."""
    src = ass = None
    for k, ln in enumerate(body):
        t = ln.strip()
        if t.startswith("#"):
            continue
        if src is None and re.match(r'^\.\s+"?\$\{?OPKIT_LIB\}?"?', t):
            src = k
        if src is not None and ass is None and "opkit_ns_assert" in t:
            ass = k
        if (ctx.kit_re().search(t) or "--apply" in t or BUILD_API.search(t) or BUILD_VERB.search(t)) and not re.match(r'^kit\(\)', t):
            return src is not None and ass is not None and src < ass < k
    return src is not None and ass is not None


# ── amendment 109: the conservative layer ─────────────────────────────────────────────────────────────────────────
# This gate is a BEST-EFFORT SECOND LAYER: it flags the shapes listed here and in --selftest; it is not a guarantee
# that no text can run the kit. The boundary that matters is the namespace and the helper's proof. A test script that
# wants to evade a textual gate can; the gate catches mistakes, not intent. What amendment 109 adds, in files that mention
# the helper, the kit, `--apply` or a build verb, for a command that is not itself ns_run/kit/inns:
#   * quote tricks inside a flag-like word (--ap""ply, --a\pply, $'--apply', -"-"apply) are normalised away first;
#   * eval / source / . / bash -c / sh -c / exec / command / builtin / alias whose operand is not a plain literal;
#   * a command substitution, backtick or parameter transformation (${x@P}, ${x@Q}, ${!x}) in a command position or
#     in a word an interpreter receives;
#   * a decoding utility (base64 -d, xxd -r, openssl enc -d, uudecode, printf %b) on a line that also feeds a shell;
#   * a file this script wrote (redirection, tee, cp/mv/install, cat <<) that it later executes or sources.
DECODERS = re.compile(r'\bbase64\b[^|;&]*\s(?:-d\b|--decode\b|-D\b)|\bxxd\b[^|;&]*\s-r|\bopenssl\b[^|;&]*\b(?:enc|base64)\b|\buudecode\b|\bbasenc\b[^|;&]*--decode|\bprintf\b[^|;&]*%b|\becho\b\s+-\w*e')
SHELL_FEED = re.compile(r'(?:^|[\s|;&(`"\'])(?:bash|sh|dash|zsh|ksh|ash|eval|source|python3?|perl|ruby|node)(?=[\s"\'|;&)<]|$)|(?:^|[\s|;&(`])\.\s')
TRANSFORM = re.compile(r'\$\{[!]?[A-Za-z_][\w\[\]@*]*@[QEAaKk]\}|\$\{!')
SAFE_SOURCED_VARS = {"OPKIT_LIB", "LIB", "HERE", "REPO", "SELF", "BASH_SOURCE"}
PEEL = {"command", "builtin", "exec"}


def raw_tokens(cmd):
    """Whitespace-separated tokens of `cmd` with their quotes kept."""
    out, cur, q, i = [], "", None, 0
    while i < len(cmd):
        c = cmd[i]
        if q:
            cur += c
            if c == q:
                q = None
            elif c == "\\" and q == '"' and i + 1 < len(cmd):
                i += 1
                cur += cmd[i]
        elif c in "'\"":
            q = c
            cur += c
        elif c == "\\" and i + 1 < len(cmd):
            cur += c + cmd[i + 1]
            i += 1
        elif c in " \t\n":
            if cur:
                out.append(cur)
            cur = ""
        else:
            cur += c
        i += 1
    if cur:
        out.append(cur)
    return out


def is_dynamic(tok):
    """True when the shell EXPANDS something in `tok` (outside single quotes): $name, ${..}, $(..), `..`."""
    q, i = None, 0
    while i < len(tok):
        c = tok[i]
        if q == "'":
            if c == "'":
                q = None
        elif c == "\\":
            i += 1
        elif q == '"' and c == '"':
            q = None
        elif q is None and c in "'\"":
            q = c
        elif c == "`":
            return True
        elif c == "$" and i + 1 < len(tok) and (tok[i + 1].isalpha() or tok[i + 1] in "_{(@*#?!0123456789"):
            return True
        i += 1
    return False


SIMPLE_EXPANSION_WORD = re.compile(r'^"?(?:\$\{[^}\s]+\}|\$\w+|\$\([^()\n]*\)|`[^`\n]*`)"?[\w./-]*"?$')


def script_operand(c0, toks):
    """The file an interpreter or shell is told to run (its first operand that is not an option), '' when it runs a program
    text (-c, -e) or stdin (-)."""
    if c0 in (".", "source"):
        return norm_path(toks[1]) if len(toks) > 1 else ""
    args = toks[1:]
    if c0 in ("env", "sudo", "setsid", "nohup", "exec"):
        while args and (args[0].startswith("-") or re.fullmatch(r'[A-Za-z_]\w*=.*', args[0])):
            args = args[1:]
        return norm_path(args[0]) if args else ""
    for a in args:
        if a in ("-c", "-e", "-E", "-"):
            return ""
        if a.startswith("-"):
            continue
        return norm_path(a)
    return ""


def norm_path(tok):
    t = tok.strip().strip("\"'")
    return re.sub(r'\$\{(\w+)\}', r'$\1', t)


def normalize_text(text):
    """The text with quote/escape tricks inside a word removed, so `--ap""ply`, `-"-"apply`, `--a\\pply` and `$'--apply'`
    match what the shell will really pass. Used ALONGSIDE the original text, never instead of it."""
    def ansi(m):
        try:
            v = codecs.decode(m.group(1).replace("\\$", "$"), "unicode_escape")
        except Exception:
            return m.group(0)
        return v if re.fullmatch(r'[\w\-./=:@%+,]*', v) else '"' + v.replace('"', '\\"') + '"'
    t = re.sub(r"\$'((?:[^'\\]|\\.)*)'", ansi, text)
    for _ in range(4):
        t2 = re.sub(r'(?<=[\w\-.=/$])(?:""|\'\')', '', t)
        t2 = re.sub(r'(?:""|\'\')(?=[\w\-.=/$])', '', t2)
        t2 = re.sub(r'(?<=[\w\-])"([\w\-./=]+)"', r'\1', t2)
        t2 = re.sub(r'"([\w\-./=]+)"(?=[\w\-])', r'\1', t2)
        t2 = re.sub(r"(?<=[\w\-])'([\w\-./=]+)'", r'\1', t2)
        t2 = re.sub(r"'([\w\-./=]+)'(?=[\w\-])", r'\1', t2)
        t2 = re.sub(r'(?<=[\w\-])\\(?=[A-Za-z])', '', t2)
        if t2 == t:
            break
        t = t2
    return t


def relevant_file(text):
    return bool(KIT_TOKEN.search(text) or BUILD_VERB.search(text) or "--apply" in text or "opkit_ns" in text or re.search(r'\bns_run\b', text))


def peel(w):
    """`command bash ...`, `builtin eval ...`, `exec bash ...` judge as the command they run (`command -v x` is read-only)."""
    while w and w[0] in PEEL:
        if w[0] == "command" and len(w) > 1 and w[1] in ("-v", "-V"):
            break
        w = w[1:]
        while w and w[0] in ("-p", "--"):
            w = w[1:]
    return w


def literal_program_kind(prog, ctx):
    """A literal program text (`eval '...'`) is judged command by command, as if it stood alone."""
    for _, ln in logical_lines(prog):
        for c2 in split_simple(strip_comment(ln)):
            w2 = words(c2)
            if not w2:
                continue
            k2, wrapped2 = command_class(w2, ctx)
            if (k2 and not wrapped2) or (not wrapped2 and redirect_writes(c2, ctx)):
                return k2 or "write"
    return None


def conservative_kind(cmd, w, ctx):
    """Amendment 109: what an UNWRAPPED command in a kit-related file may not be, whether or not it names the kit."""
    w = peel(strip_helper(w))
    if not w or w[0] in WRAPPERS:
        return None
    toks = raw_tokens(cmd)
    while toks and (re.fullmatch(r'[A-Za-z_]\w*=.*', toks[0]) or toks[0] in KEYWORDS):
        toks = toks[1:]
    while toks and toks[0] in PEEL:
        toks = toks[1:]
        while toks and toks[0] in ("-p", "--"):
            toks = toks[1:]
    while toks and re.match(r'^(?:\d*[<>]|&>)', toks[0]):        # a leading redirection (`exec 8<f`, `>file` after a group) is not the command
        toks = toks[2:] if re.fullmatch(r'(?:\d*[<>]{1,3}|&>)', toks[0]) else toks[1:]
    if not toks:
        return None
    c0 = w[0]

    def ns_ok(t):
        if re.search(r'\$\((?:refused\s+\S+\s+\S+\s+)?(?:ns_run|kit|inns)\b', t):
            return True
        m = re.fullmatch(r'"?\$\{?(\w+)\}?"?', t)
        return bool(m and m.group(1) in ctx.nsvars)

    # the command word itself comes from an expansion (`"$I" "$K"`, `$(printf bash) ...`, a prompt expansion)
    if toks and is_dynamic(toks[0]) and SIMPLE_EXPANSION_WORD.match(toks[0]) and not ns_ok(toks[0]) and toks[0] not in ('"$@"', '$@', '"$*"') \
            and not re.fullmatch(r'"?\$\{?(?:' + "|".join(sorted(SANCTIONED_VARS | SAFE_SOURCED_VARS)) + r')\}?(?:/[^\s"]*)?"?', toks[0]):
        return "dynamic"
    if re.search(r'\$\{[!]?[A-Za-z_][\w\[\]@*]*@P\}', cmd):
        return "dynamic"                  # ${P@P} EXECUTES the command substitutions in its value, whatever command it sits in
    if TRANSFORM.search(cmd) and (c0 in SHELLS | INTERPRETERS | {"eval", ":", "env", "sudo"} or (toks and toks[0].startswith(("$", '"$')))):
        return "dynamic"                  # ${x@Q} ${x@E} ${!k} (a variable named by a variable) reaching an interpreter, or in the command word
    if c0 == "alias" and len(w) > 1:
        return "kit"
    if c0 == "shopt" and "expand_aliases" in w:
        return "kit"
    if c0 == "eval":
        operand = toks[1:] if toks else []
        if any(is_dynamic(t) and not ns_ok(t) for t in operand):
            return "dynamic"
        if operand:
            return literal_program_kind(" ".join(shlex_split(" ".join(operand))), ctx)
    if c0 in ("bash", "sh", "dash", "zsh", "ksh", "ash") and "-c" in toks:
        i = toks.index("-c")
        if i + 1 < len(toks) and is_dynamic(toks[i + 1]) and not ns_ok(toks[i + 1]):
            return "dynamic"
    if c0 in (".", "source") and len(toks) > 1:
        t = toks[1]
        if t.startswith("<(") or (is_dynamic(t) and not ns_ok(t)
                                  and not re.match(r'^"?\$\{?(?:' + "|".join(sorted(SAFE_SOURCED_VARS)) + r')\b', t)):
            return "dynamic"
    # a file this script wrote, then ran or sourced
    op = script_operand(c0, toks) if c0 in SHELLS | INTERPRETERS | {"env", "sudo", "setsid", "nohup", "exec"} else norm_path(toks[0])
    if op and op in ctx.written:
        return "written"
    return None


def note_writes(cmd, w, ctx):
    """Record the files a command writes (redirection targets, tee, cp/mv/install destinations)."""
    for t in outer_redirect_targets(cmd):
        if not t.startswith(("/dev/", "/proc/", "&")) and not re.fullmatch(r'\$?\{?\d+\}?', t):
            ctx.written.add(norm_path(t))
    w = strip_helper(w)
    if w and w[0] == "tee":
        for x in w[1:]:
            if not x.startswith("-"):
                ctx.written.add(norm_path(x))
    elif w and w[0] in ("cp", "mv", "install", "ln"):
        ops = [x for x in w[1:] if not x.startswith("-")]
        if len(ops) >= 2:
            ctx.written.add(norm_path(ops[-1]))


def line_kind(line):
    """A decoding utility on a line that also hands something to a shell, an interpreter or eval."""
    t = strip_comment(line)
    first = (t.split() or [""])[0]
    if first in WRAPPERS or first == "refused":
        return None
    if DECODERS.search(t) and SHELL_FEED.search(t):
        return "decode"
    if re.search(r'(?:^|[;&|({\s])(?:eval|source|\.|(?:ba|da|z|k|a)?sh\s+-c)\s+["\']?(?:\$\(|`)(?!\s*(?:refused\s+\S+\s+\S+\s+)?(?:ns_run|kit|inns)\b)', t):
        return "decode"            # eval / source / sh -c handed a substitution that is not an ns_run result
    return None


def check_text_core(path, text):
    bad = []
    kit_helper = re.compile(r'^\s*kit\(\)\s*\{\s*opkit_ns_assert\s*\|\|[^;{}]*;\s*bash "\$KIT" "\$@";\s*\}\s*$')
    lines = logical_lines(text)
    ctx = Ctx(text)
    relevant = relevant_file(text)
    # the self-test's own `--child` block runs INSIDE the unshare the test makes; nothing else is exempt
    skip_range = None
    if os.path.basename(path) == "test_opkit_ns.sh":
        a = next((n for n, l in lines if l.startswith('if [ "${1:-}" = --child ]')), None)
        b = next((n for n, l in lines if a and n > a and l == "fi"), None)
        skip_range = (a, b) if a and b else None
    i = 0
    while i < len(lines):
        n, line = lines[i]
        i += 1
        # a heredoc: gather its body; an IN-NAMESPACE body (it starts by sourcing the helper) is judged for
        # kit/--apply/build commands only, any other body is data -- but the command that feeds it to an
        # interpreter must itself be wrapped when the body names a real destination
        m = HEREDOC.search(strip_comment(line))
        if m and not any(lines[j][1].strip() == m.group(2) for j in range(i, len(lines))):
            m = None                          # `$(( 1 << 2 ))`, not a heredoc: no terminator follows
        body, ns_body, body_mentions = [], False, False
        if skip_range and skip_range[0] <= n <= skip_range[1]:
            continue
        if m:
            while i < len(lines) and lines[i][1].strip() != m.group(2):
                body.append(lines[i])
                i += 1
            i += 1
            ns_body = in_namespace_body([b[1] for b in body], ctx)
            names_dest = bool(DEST_LITERAL.search("\n".join(b[1] for b in body)))
            btxt = "\n".join(b[1] for b in body)
            body_mentions = bool(ctx.kit_re().search(btxt) or re.search(r'--apply', btxt) or BUILD_API.search(btxt)
                                 or BUILD_VERB.search(btxt) or KIT_FILE in btxt)
        if kit_helper.match(line):           # the helper itself: asserts before every call
            continue
        if relevant and line_kind(line):
            bad.append(f"{path}:{n}: [decode] a decoding utility feeds a shell, eval or an interpreter on this line: {strip_comment(line).strip()[:100]}")
        for cmd in split_simple(strip_comment(line)):
            for t in redirect_writes(cmd, ctx):
                bad.append(f"{path}:{n}: [write] a redirection into a real destination ({t}); the OUTER shell opens it even on an ns_run command: {cmd.strip()[:100]}")
            for v in assigned_command_strings(cmd, ctx):
                ctx.cmdvars.add(v)
                bad.append(f"{path}:{n}: [kit] {v} is assigned a command string naming the kit / --apply / a build verb; run it with ns_run, not through a variable: {cmd.strip()[:100]}")
            w = words(cmd)
            if not w:
                continue
            # `printf -v c "bash %s" "$KIT"` builds a command string in a variable: whoever expands it later runs the kit
            if w[0] == "printf" and "-v" in w[1:-1] and \
                    (ctx.kit_re().search(" ".join(w)) or re.search(r'--apply|' + BUILD_VERB.pattern, " ".join(w))):
                ctx.cmdvars.add(w[w.index("-v") + 1])
            kind, wrapped = command_class(w, ctx)
            if not wrapped and (pc := prim_called(w)):
                bad.append(f"{path}:{n}: [primitive] {pc} changes the mount table of whatever namespace it runs in; run it only inside ns_run: {cmd.strip()[:100]}")
            if not kind and not wrapped and relevant:
                kind = conservative_kind(cmd, w, ctx)
            note_writes(cmd, w, ctx)
            if m and not kind and not ns_body and names_dest and strip_helper(w)[0] in ("python3", "python", "bash", "sh") and not wrapped:
                kind = "write"
            if m and body_mentions and not ns_body and not wrapped and HEREDOC.search(cmd) and not kind:
                kind = "kit"                  # amendment 105: a heredoc that names the kit / --apply / the build API, fed to an unwrapped command
            if kind and not wrapped:
                bad.append(f"{path}:{n}: [{kind}] not itself an ns_run/kit command: {cmd.strip()[:100]}")
        if m and ns_body:
            bl = logical_lines("\n".join(b[1] for b in body))
            for bn, bline in [(body[k - 1][0], t) for k, t in bl]:
                if kit_helper.match(bline):
                    continue
                for cmd in split_simple(strip_comment(bline)):
                    w = words(cmd)
                    if not w:
                        continue
                    kind, wrapped = command_class(w, ctx)
                    if kind in ("kit", "--apply", "build") and not wrapped:
                        bad.append(f"{path}:{bn}: [{kind}] in-namespace body, not itself a kit/ns_run command: {cmd.strip()[:100]}")
    base = os.path.basename(path)
    for n, line in lines:
        t = strip_comment(line)
        if base != "test_opkit_ns.sh":
            if INTERNAL_KNOBS.search(t):
                bad.append(f"{path}:{n}: [knob] a helper-internal knob outside the helper's own self-test: {t.strip()[:100]}")
            if "OPKIT_CAPS_KEEP" in t and "opkit_fixture.sh" not in t:
                bad.append(f"{path}:{n}: [knob] OPKIT_CAPS_KEEP (CAP_SYS_ADMIN) is for the controlled-build fixture only: {t.strip()[:100]}")
        for m2 in ([] if base == "test_opkit_ns.sh" else re.finditer(r'OPKIT_RW=("[^"]*"|\'[^\']*\'|\S*)', t)):
            if any(ctx.real_arg(tok) or not sanctioned(tok, ctx) for tok in m2.group(1).strip("\"'").split()):
                bad.append(f"{path}:{n}: [knob] OPKIT_RW names something that is not provably scratch: {m2.group(0)[:100]}")
    if KIT_TOKEN.search(text) and "opkit_ns.sh" not in text:
        bad.append(f"{path}: runs the kit but never uses scripts/lib/opkit_ns.sh")
    return bad


def check_text(path, text):
    """check_text_core on the text as written AND on the text with quote tricks inside words normalised away."""
    bad = check_text_core(path, text)
    nt = normalize_text(text)
    if nt != text:
        have = set(bad)
        bad += [b for b in check_text_core(path, nt) if b not in have]
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


# ── amendment 118: diagnostics never go through a descriptor the helper has not vetted ──────────────────────────────────
# Incident 2026-10-09: ns_run classified fd 2 as a writable regular file (the real /etc/passwd, opened read-write by a caller) and
# then wrote its refusal into it. The rule, held over scripts/lib/opkit_ns.sh: (a) NO line writes to fd 1 or fd 2 (`>&2`, `1>&2`,
# /dev/stderr, /dev/stdout, /proc/self/fd/1|2, /dev/fd/1|2, sys.stderr, a bare echo/printf) except opkit_say, whose first act is
# the classifier on fd 2, and the VALUE functions below, whose stdout is a command substitution's input; (b) ns_run classifies
# fds 0-2 (opkit_ns_std_fds_ok) before it unshares, makes a directory or opens a handle, and runs its pre-checks with their
# stderr captured; (c) the in-namespace shell classifies them again before it mounts anything.
VALUE_FUNCTIONS = {"opkit_overrides", "opkit_ns_ids", "opkit_bounding_arg", "opkit_rw_extra_roots", "opkit_ns_prephase"}
_FD_WRITE = re.compile(r'(?<![\w<])[12]?>&2|>\s*/dev/(?:stderr|stdout)\b|>\s*/(?:proc/self|dev)/fd/[12]\b|sys\.(?:stderr|stdout)')
_PRINTS = re.compile(r'\b(?:echo|printf)\b')
_REDIRECTED = re.compile(r'(?<![<])>')   # any redirection: >&2 and 1>&2 belong to _FD_WRITE alone, not to a second rule


def diagnostic_problems(text, label="scripts/lib/opkit_ns.sh"):
    bad, fns = [], helper_functions(text)
    for name, body in fns.items():
        for n, l in body:
            code = re.sub(r"""\s#\s.*$""", "", l)
            if not code.strip():
                continue
            if _FD_WRITE.search(code) and name != "opkit_say":
                bad.append(f"{label}:{n}: {name} writes to fd 1 or fd 2 directly (use opkit_say): {code.strip()[:90]}")
            elif _PRINTS.search(code) and "opkit_say" not in code.split("echo")[0] and name not in VALUE_FUNCTIONS | {"opkit_say"} \
                    and not _REDIRECTED.search(code) and not re.search(r'\$\([^)]*\b(?:echo|printf)\b', code):
                bad.append(f"{label}:{n}: {name} prints to its stdout, which may be an unvetted descriptor: {code.strip()[:90]}")
    say = [l for _, l in fns.get("opkit_say", []) if l.strip()]
    if not say or not re.match(r'\s*if opkit_ns_fd_why 2\b', say[0]):
        bad.append(f"{label}: opkit_say must begin by classifying fd 2 (`if opkit_ns_fd_why 2 ...`)")
    run = "\n".join(l for _, l in fns.get("ns_run", []))
    std = run.find("opkit_ns_std_fds_ok")
    first = [run.find(w) for w in ("unshare ", "mktemp", "exec {hfd}", "opkit_say \"REFUSE(ns_run)")]
    if std < 0 or any(i >= 0 and i < std for i in first):
        bad.append(f"{label}: ns_run must classify fds 0-2 (opkit_ns_std_fds_ok) before it unshares, makes a directory or says anything")
    if not re.search(r'\$\(opkit_ns_prephase 2>&1\)', run):
        bad.append(f"{label}: ns_run must run its pre-checks with stderr captured (`$(opkit_ns_prephase 2>&1)`)")
    i_std = text.find("opkit_ns_std_fds_ok $OPKIT_DIAG_ROOTS || exit 97")
    i_iso = text.find("\n    opkit_ns_isolate || {")
    if i_std < 0 or i_iso < 0 or i_iso < i_std:
        bad.append(f"{label}: the in-namespace shell must classify fds 0-2 (exit 97) before opkit_ns_isolate")
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
    bad += primitive_problems(root)
    bad += diagnostic_problems(open(os.path.join(root, "scripts", "lib", "opkit_ns.sh")).read())
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
    ("a command string without --apply assigned then eval'd", '. scripts/lib/opkit_ns.sh\nc=\'bash "$KIT" --from c\'\neval "$c"\n'),
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
    ("a mkdir through WORK reassigned to a real path", '. scripts/lib/opkit_ns.sh\nWORK=/var/lib/axon-x\nmkdir -p "$WORK/sub"\n'),
    ("a mkdir through a variable holding a real path", '. scripts/lib/opkit_ns.sh\nD=/var/lib/axon-x\nmkdir -p "$D"\n'),
    ("a mkdir through a variable derived from a real path", '. scripts/lib/opkit_ns.sh\nD=/var/lib/axon-x\nE=$D/sub\nmkdir -p "$E"\n'),
    ("a redirect through WORK reassigned to a real path", '. scripts/lib/opkit_ns.sh\nWORK=/etc/axon\necho x > "$WORK/y"\n'),
    ("a redirect into /etc", '. scripts/lib/opkit_ns.sh\necho x > /etc/axon/y\n'),
    ("an append into /usr/local", '. scripts/lib/opkit_ns.sh\necho x >>/usr/local/bin/y\n'),
    ("a redirect into a real path on an ns_run command (the outer shell opens it)", '. scripts/lib/opkit_ns.sh\nns_run true > /etc/axon/y\n'),
    ("python -c writing a file under /etc", '. scripts/lib/opkit_ns.sh\npython3 -c "open(\'/etc/axon/x\',\'w\').write(\'x\')"\n'),
    ("an unlisted wrapper on a differently named kit copy", '. scripts/lib/opkit_ns.sh\nionice -c3 bash "$WORK/other-kit.sh" --from c --apply\n'),
    ("python -c writing under /opt", '. scripts/lib/opkit_ns.sh\npython3 -c "open(\'/opt/x\',\'w\')"\n'),
    # amendment 105: the shapes the round-11 FIELD-ORIGIN reviewer executed past the gate (each ran a STUB kit with --apply)
    ("python reading the kit and running it from a heredoc", '. scripts/lib/opkit_ns.sh\npython3 - "$KIT" <<PY\nimport subprocess, sys\nsubprocess.run(["bash", sys.argv[1], "--apply"])\nPY\n'),
    ("python -c with an argv-built --apply", '. scripts/lib/opkit_ns.sh\npython3 -c \'import subprocess,sys; subprocess.run(["bash", sys.argv[1], "--" + "apply"])\' "$KIT"\n'),
    ("a trap that runs the kit without --apply", '. scripts/lib/opkit_ns.sh\ntrap \'bash "$KIT" --from c\' EXIT\n'),
    ("a trap that runs the kit", '. scripts/lib/opkit_ns.sh\ntrap \'bash "$KIT" --apply\' EXIT\n'),
    ("printf -v builds a command string, then eval", '. scripts/lib/opkit_ns.sh\nprintf -v c "bash %s --from c" "$KIT"\neval "$c"\n'),
    ("export of a command string, then eval", '. scripts/lib/opkit_ns.sh\nexport X="bash $KIT --apply"\neval "$X"\n'),
    ("declare of a command string, then eval", '. scripts/lib/opkit_ns.sh\ndeclare c="bash $KIT --apply"\neval "$c"\n'),
    ("local of a command string in a function, then eval", '. scripts/lib/opkit_ns.sh\nf() { local c="bash $KIT --apply"; eval "$c"; }\n'),
    ("a command piped into bash", '. scripts/lib/opkit_ns.sh\necho "bash $KIT --apply" | bash\n'),
    ("a command piped into sh (the second command names nothing)", '. scripts/lib/opkit_ns.sh\nprintf \'bash "%s" --apply\\n\' "$KIT" | sh\n'),
    ("a command piped into bash -s", '. scripts/lib/opkit_ns.sh\ncat "$KIT" | bash -s -- --apply\n'),
    ("a heredoc fed to bash", '. scripts/lib/opkit_ns.sh\nbash <<EOF\nbash "$KIT" --apply\nEOF\n'),
    ("a heredoc written to a file, then run", '. scripts/lib/opkit_ns.sh\ncat >"$WORK/r.sh" <<EOF\nbash "$KIT" --apply\nEOF\nbash "$WORK/r.sh"\n'),
    ("a copy of the kit with --apply passed through a variable", '. scripts/lib/opkit_ns.sh\ncp "$KIT" "$WORK/k"\nA=--apply\nbash "$WORK/k" $A\n'),
    ("--apply carried in a variable, kit named by its file", '. scripts/lib/opkit_ns.sh\nA=--apply\nbash scripts/operator_deploy_protected_host.sh $A\n'),
    ("a glob in the kit's file name with --apply in a variable", '. scripts/lib/opkit_ns.sh\nA=--apply\nbash scripts/operator_deploy_*.sh $A\n'),
    ("a kit copied with cat and applied through a variable", '. scripts/lib/opkit_ns.sh\ncat "$KIT" >"$WORK/k"\nA=--apply\nbash "$WORK/k" $A\n'),
    ("a copy of the kit run without --apply", '. scripts/lib/opkit_ns.sh\ncp "$KIT" "$WORK/k"\nbash "$WORK/k" --from c\n'),
    ("a glob in the kit's file name", '. scripts/lib/opkit_ns.sh\nbash scripts/operator_deploy_p*.sh --from c\n'),
    ("source of a process substitution", '. scripts/lib/opkit_ns.sh\nsource <(cat "$KIT")\n'),
    ("a perl heredoc that runs the kit", '. scripts/lib/opkit_ns.sh\nperl - "$KIT" <<PL\nsystem("bash", $ARGV[0], "--apply");\nPL\n'),
    ("ruby -e with --apply", '. scripts/lib/opkit_ns.sh\nruby -e \'system("bash", ENV["KIT"], "--apply")\'\n'),
    ("node child_process", '. scripts/lib/opkit_ns.sh\nnode -e \'require("child_process").execSync("bash " + process.env.KIT)\'\n'),
    ("dd to a device", '. scripts/lib/opkit_ns.sh\ndd if=/dev/zero of=/dev/sdd count=1\n'),
    ("mount -o remount,rw /", '. scripts/lib/opkit_ns.sh\nmount -o remount,rw /\n'),
    ("a bare mount", '. scripts/lib/opkit_ns.sh\nmount --bind "$WORK/a" "$WORK/b"\n'),
    ("a mkdir through $WORK/../..", '. scripts/lib/opkit_ns.sh\nmkdir -p "$WORK/../../etc/x"\n'),
    ("bash -c writing under /etc", '. scripts/lib/opkit_ns.sh\nbash -c \'echo x > /etc/axon/y\'\n'),
    ("python os.makedirs under /opt", '. scripts/lib/opkit_ns.sh\npython3 -c "import os; os.makedirs(\'/opt/x\')"\n'),
    ("python pathlib under /opt", '. scripts/lib/opkit_ns.sh\npython3 -c "import pathlib; pathlib.Path(\'/opt/x\').mkdir()"\n'),
    ("perl writing under /opt", '. scripts/lib/opkit_ns.sh\nperl -e \'open(F, ">/opt/x")\'\n'),
    ("cd /opt && touch x", '. scripts/lib/opkit_ns.sh\ncd /opt && touch x\n'),
    ("touch on a relative path", '. scripts/lib/opkit_ns.sh\ntouch x\n'),
    ("a mkdir on an unknown variable", '. scripts/lib/opkit_ns.sh\nmkdir -p "$SOMEWHERE/x"\n'),
    ("bare systemctl enable", '. scripts/lib/opkit_ns.sh\nsystemctl enable axon-x.service\n'),
    ("bare useradd", '. scripts/lib/opkit_ns.sh\nuseradd axon-x\n'),
    ("git -C /opt", '. scripts/lib/opkit_ns.sh\ngit -C /opt init\n'),
    ("a redirect into an unknown variable", '. scripts/lib/opkit_ns.sh\necho x > "$SOMEWHERE/y"\n'),
    ("a tee on an unknown path", '. scripts/lib/opkit_ns.sh\necho x | tee /home/y\n'),
    ("OPKIT_CAPS_KEEP outside the fixture call", '. scripts/lib/opkit_ns.sh\nOPKIT_CAPS_KEEP=sys_admin ns_run bash "$KIT" --apply\n'),
    ("OPKIT_RW naming a system directory", '. scripts/lib/opkit_ns.sh\nOPKIT_RW=/opt ns_run bash "$KIT" --from c\n'),
    ("OPKIT_RW naming an unknown variable", '. scripts/lib/opkit_ns.sh\nOPKIT_RW="$ELSEWHERE" ns_run true\n'),
    ("a helper-internal knob set by a test", '. scripts/lib/opkit_ns.sh\nOPKIT_KEEP_FDS=7 ns_run true\n'),
    # amendment 109: the shapes the round-12 FIELD-ORIGIN (part 2) reviewer ran past amendment 105's gate (each ran a STUB kit
    # with --apply). The kit under another name made by a command substitution, the flag spelt with quote tricks:
    ("an alias made by $(echo $KIT) with --ap\"\"ply", '. scripts/lib/opkit_ns.sh\nK=$(echo $KIT)\nA=--ap""ply\nbash "$K" $A\n'),
    ("the same inside a function", '. scripts/lib/opkit_ns.sh\nK=$(echo $KIT)\nA=--ap""ply\nf() { bash "$K" $A; }\nf\n'),
    ("command -p bash on the alias", '. scripts/lib/opkit_ns.sh\nK=$(echo $KIT)\nA=--ap""ply\ncommand -p bash "$K" $A\n'),
    ("builtin eval of a string naming the alias", '. scripts/lib/opkit_ns.sh\nK=$(echo $KIT)\nbuiltin eval "bash $K --ap""ply"\n'),
    ("a shell alias that runs the kit", '. scripts/lib/opkit_ns.sh\nK=$(echo $KIT)\nshopt -s expand_aliases\nalias k=\'bash "$K" --ap""ply\'\nk\n'),
    ("prompt expansion ${P@P} of a command substitution", '. scripts/lib/opkit_ns.sh\nK=$(echo $KIT)\nP=\'$(bash $K --ap""ply)\'\n: "${P@P}"\n'),
    ("indirect expansion ${!k}", '. scripts/lib/opkit_ns.sh\nk=KIT\nbash "${!k}" --ap""ply\n'),
    ("a command string built by printf in a substitution, then eval", '. scripts/lib/opkit_ns.sh\nK=$(echo $KIT)\nc=$(printf \'%s %s --%s\' bash "$K" apply)\neval "$c"\n'),
    ("eval of a decoded string", '. scripts/lib/opkit_ns.sh\neval "$(echo YmFzaCAkS0lUIC0tYXBwbHk= | base64 -d)"\n'),
    ("bash -c of a decoded string", '. scripts/lib/opkit_ns.sh\nbash -c "$(echo YmFzaCAkS0lUIC0tYXBwbHk= | base64 -d)"\n'),
    ("a decoded string piped to a shell (xxd)", '. scripts/lib/opkit_ns.sh\necho 62617368 | xxd -r -p | sh\n'),
    ("a decoded string piped to a shell (openssl)", '. scripts/lib/opkit_ns.sh\nopenssl enc -d -base64 <<<YmFzaA== | bash\n'),
    ("eval of an unquoted substitution", '. scripts/lib/opkit_ns.sh\neval $(printf "bash %s --apply" "$KIT")\n'),
    ("sh -c of an unquoted substitution", '. scripts/lib/opkit_ns.sh\nsh -c $(echo true)\n'),
    ("sh -c of a variable holding a program", '. scripts/lib/opkit_ns.sh\nx=true\nsh -c "$x"\n'),
    ("a command word that is a variable", '. scripts/lib/opkit_ns.sh\nI=bash\nK=$(echo $KIT)\n"$I" "$K" --ap""ply\n'),
    ("a command word that is a variable (no kit in sight)", '. scripts/lib/opkit_ns.sh\nI=mount\n"$I" -o remount,rw /\n'),
    ("a flag spelt with a quoted dash", '. scripts/lib/opkit_ns.sh\nA=-"-"apply\nbash "$KIT" "$A"\n'),
    ("a flag spelt with a backslash", '. scripts/lib/opkit_ns.sh\nbash "$KIT" --a\\pply\n'),
    ("a flag spelt as an ANSI-C string", '. scripts/lib/opkit_ns.sh\nbash "$KIT" $\'\\x2d\\x2dapply\'\n'),
    ("a flag held in an ANSI-C variable", '. scripts/lib/opkit_ns.sh\nA=$\'\\x2d\\x2dapply\'\nK=$(echo $KIT)\nbash "$K" "$A"\n'),
    ("a script written by printf, then run", '. scripts/lib/opkit_ns.sh\nprintf \'bash %s --apply\\n\' "$KIT" >$W/s.sh\nbash $W/s.sh\n'),
    ("a script written with a hidden alias, then run", '. scripts/lib/opkit_ns.sh\nK=$(echo $KIT)\nprintf \'bash %s --ap""ply\\n\' "$K" >$W/s.sh\nbash $W/s.sh\n'),
    ("a script written by tee, then executed by path", '. scripts/lib/opkit_ns.sh\necho true | tee "$W/s.sh" >/dev/null\nchmod +x "$W/s.sh"\n"$W/s.sh"\n'),
    ("a script copied into place, then sourced", '. scripts/lib/opkit_ns.sh\ncp "$WORK/f" "$W/f.sh"\n. "$W/f.sh"\n'),
    ("a function file written, then sourced", '. scripts/lib/opkit_ns.sh\nK=$(echo $KIT)\nprintf \'k(){ bash %s --ap""ply; }\\n\' "$K" >$W/f.sh\n. $W/f.sh\nk\n'),
    ("a script written by cat <<, then run", '. scripts/lib/opkit_ns.sh\ncat >$W/s.sh <<EOF\ntrue\nEOF\nbash $W/s.sh\n'),
    ("a source of a variable path", '. scripts/lib/opkit_ns.sh\n. "$SOMEWHERE"\n'),
    ("a fake heredoc marker in an assignment", '. scripts/lib/opkit_ns.sh\nX="<<EOF"\nbash "$KIT" --apply\nEOF\n'),
    ("a fake heredoc marker in an echo", '. scripts/lib/opkit_ns.sh\necho "<<EOF"\nbash "$KIT" --apply\nEOF\n'),
    ("a fake heredoc marker in a single-quoted string", '. scripts/lib/opkit_ns.sh\nX=\'<<EOF\'\nbash "$KIT" --from c\nEOF\n'),
    ("sudo bash -c on the alias", '. scripts/lib/opkit_ns.sh\nK=$(echo $KIT)\nsudo bash -c "$K \\$A"\n'),
    ("a here-string fed to bash", '. scripts/lib/opkit_ns.sh\nbash <<<"bash $KIT --apply"\n'),
    ("command eval of a variable", '. scripts/lib/opkit_ns.sh\nx=true\ncommand eval "$x"\n'),
    ("builtin source of a variable path", '. scripts/lib/opkit_ns.sh\nbuiltin source "$SOMEWHERE"\n'),
    ("eval of a literal program that mounts", '. scripts/lib/opkit_ns.sh\neval "mount --bind a b"\n'),
    ("a variable named by a variable, handed to an interpreter", '. scripts/lib/opkit_ns.sh\nk=SCRIPT\npython3 "${!k}"\n'),
    ("a flag held in a variable (another program)", '. scripts/lib/opkit_ns.sh\nA=--apply\n./deploy.sh $A\n'),
    ("a command string naming the kit, assigned and not used here", '. scripts/lib/opkit_ns.sh\nc=\'bash "$KIT" --from c\'\n'),
    ("a heredoc written to a file, then run through a split spelling", '. scripts/lib/opkit_ns.sh\ncat >"$WORK/r.sh" <<EOF\nbash "$KIT" --apply\nEOF\nbash "$WORK"/r.sh\n'),
    ("a copy of the kit, run from a function defined before the copy", '. scripts/lib/opkit_ns.sh\nrunit() { bash "$WORK/k" --from c; }\ncp "$KIT" "$WORK/k"\nrunit\n'),
    ("a flag held in a variable, spelt with an empty quote pair (another program)", '. scripts/lib/opkit_ns.sh\nA=--ap""ply\n./deploy.sh $A\n'),
    ("a flag held in a variable, spelt with a quoted dash (another program)", '. scripts/lib/opkit_ns.sh\nA=-"-"apply\n./deploy.sh $A\n'),
    ("a flag held in a variable, spelt with a backslash (another program)", '. scripts/lib/opkit_ns.sh\nA=--a\\pply\n./deploy.sh $A\n'),
    ("a flag held in a variable, spelt as an ANSI-C string (another program)", '. scripts/lib/opkit_ns.sh\nA=$\'\\x2d\\x2dapply\'\n./deploy.sh $A\n'),
    ("a flag as an ANSI-C string (another program)", '. scripts/lib/opkit_ns.sh\n./deploy.sh $\'--apply\'\n'),
    ("a program text decoded by base64 handed to python -c", '. scripts/lib/opkit_ns.sh\npython3 -c "$(echo cHJpbnQoMSk= | base64 -d)"\n'),
    ("prompt expansion of a plain value", '. scripts/lib/opkit_ns.sh\nP=\'$(true)\'\n: "${P@P}"\n'),
    ("an alias of a host verb", '. scripts/lib/opkit_ns.sh\nshopt -s expand_aliases\nalias m=mount\nm --bind a b\n'),
    ("a multi-line quoted bash -c that runs the kit", '. scripts/lib/opkit_ns.sh\nbash -c \'\nbash "$KIT" --apply\n\'\n'),
    # amendment 111: one shape per guard of the peel / alias rules, so each is refused by its own guard alone
    ("command -p eval of a variable", '. scripts/lib/opkit_ns.sh\nx=true\ncommand -p eval "$x"\n'),
    ("exec eval of a variable", '. scripts/lib/opkit_ns.sh\nx=true\nexec eval "$x"\n'),
    ("an alias that shadows the helper", '. scripts/lib/opkit_ns.sh\nalias ns_run=env\n'),
    ("alias expansion switched on", '. scripts/lib/opkit_ns.sh\nshopt -s expand_aliases\n'),
]
CONTROLS = [
    ('. scripts/lib/opkit_ns.sh\nns_run bash "$KIT" --from c --apply\n'),
    ('. scripts/lib/opkit_ns.sh\nrefused "x" "y" ns_run bash "$KIT" --from c\n'),
    ('. scripts/lib/opkit_ns.sh\no=$(ns_run bash "$KIT" --from c 2>&1); rc=$?\n'),
    ('. scripts/lib/opkit_ns.sh\nRUSTC_WRAPPER=/usr/bin/true ns_run bash "$KIT" --from c\n'),
    ('. scripts/lib/opkit_ns.sh\nOPKIT_NET=host ns_run bash scripts/lib/opkit_fixture.sh a b\n'),
    ('. scripts/lib/opkit_ns.sh\nns_run mkdir -p /var/lib/axon-x\n'),
    ('. scripts/lib/opkit_ns.sh\nns_run python3 - "$KIT" <<PY\nPY\ncp -- "$KIT" "$WORK/k"\nKIT=$CLONE/k\n'),
    ('. scripts/lib/opkit_ns.sh\nK=$KIT\nns_run bash "$K" --from c\n'),
    ('. scripts/lib/opkit_ns.sh\necho "$KIT"\n[ -f "$KIT" ] && grep -q x "$KIT"\n'),
    ('. scripts/lib/opkit_ns.sh\nD=/var/lib/axon-x\nns_run mkdir -p "$D"\n'),
    ('. scripts/lib/opkit_ns.sh\necho x > "$WORK/out" 2>/dev/null\nns_run true >"$WORK/o" 2>&1\n'),
    ('. scripts/lib/opkit_ns.sh\nOPKIT_NET=host ns_run python3 -c "import guest_build_env as g; g.begin(1)"\n'),
    ('. scripts/lib/opkit_ns.sh\nc=$(ns_run true)\neval "$c"\n'),
    ('. scripts/lib/opkit_ns.sh\ncat <<EOF >"$WORK/x"\nhello\nEOF\nprintf x | sha256sum\nmkdir -p "$WORK/a"\n'),
    ('. scripts/lib/opkit_ns.sh\nOPKIT_CAPS_KEEP=sys_admin OPKIT_NET=host ns_run bash scripts/lib/opkit_fixture.sh a b\n'),
    ('. scripts/lib/opkit_ns.sh\ntrap \'rm -rf "$WORK"\' EXIT\nexport OPKIT_LIB=$HERE/lib/opkit_ns.sh\nf() { local o rc; o=$(ns_run true); }\n'),
    ('. scripts/lib/opkit_ns.sh\nA=--apply\nns_run bash "$KIT" $A\n'),
    ('. scripts/lib/opkit_ns.sh\nsystemctl status axon-x.service\nbash -c \'echo hi\'\neval "$(ns_run true)"\n'),
    ('. scripts/lib/opkit_ns.sh\nWORK=$(mktemp -d "${TMPDIR:-/var/tmp}/x.XXXXXX")\nD=$WORK/sub\nmkdir -p "$D"\ncp a "$D/b"\nrm -rf "${WORK:?}/x"\necho x >"$D/y"\n'),
    ('. scripts/lib/opkit_ns.sh\nx=$(( 1 << 2 ))\nOPKIT_RW="$WORK $STASHDIR" ns_run true\n'),
    ('. scripts/lib/opkit_ns.sh\nns_run bash -c \'\nmount -t tmpfs t /mnt\necho x > /etc/axon/y\n\'\n'),
    ('. scripts/lib/opkit_ns.sh\npython3 -c \'import json,sys; print(json.load(open(sys.argv[1])))\' "$WORK/x.json"\n'),
    # amendment 109: what the new rules must leave alone (real shapes of the shipped test scripts)
    ('. scripts/lib/opkit_ns.sh\n. "$OPKIT_LIB"\nsource "$HERE/lib/opkit_ns.sh"\n'),
    ('. scripts/lib/opkit_ns.sh\ncommand -v unshare >/dev/null || exit 77\nexec 8<"$W/f"\nexec </dev/null\n'),
    ('. scripts/lib/opkit_ns.sh\nbash -c \'echo $HOME "$1"\' x\nsh -c \'true "$2"\'\n'),
    ('. scripts/lib/opkit_ns.sh\nprintf x >"$W/x"\ncat "$W/x"\ncp "$W/x" "$W/y"\npython3 - "$W/y" <<PY\nPY\n'),
    ('. scripts/lib/opkit_ns.sh\nbase64 -d <<<aGVsbG8= >"$W/out"\necho "$(base64 -d <<<aGVsbG8=)"\n'),
    ('. scripts/lib/opkit_ns.sh\no=$(ns_run true)\neval "$o"\neval "$(ns_run true)"\nbash -c "$(ns_run true)"\n'),
    ('. scripts/lib/opkit_ns.sh\nX="a << b"\ny=$(( 1 << 3 ))\ncat <<EOF >"$W/x"\nhello <<EOF\nEOF\nns_run bash "$KIT" --from c --apply\n'),
    ('. scripts/lib/opkit_ns.sh\nfoo() { "$@"; }\nfoo true\n: "${X:=1}"\necho "${#X} ${X%%/*} ${X@Q}"\n'),
    ('. scripts/lib/opkit_ns.sh\nns_run bash -c "echo $KIT"\nbash "$HERE/scratch.sh"\n'),
    ('. scripts/lib/opkit_ns.sh\nHF=$WORK/hostfile bash -c \'. "$OPKIT_LIB"; exec 7>>"$HF"; ns_run true\'\n'),
]


# ── amendment 111: the counts the documents quote are DERIVED, and a stale quote is a failure ────────────────────────
# Amendment 105 and the matrix row A236 quoted a must-flag/control count long after the self-test had grown past it: a number
# typed into a document is not tied to anything. Every quote of the form "N must-flag shapes" (and the
# "M controls" that follows it) anywhere under governance/, scripts/ and crates/ must now equal the count this file derives
# from BYPASSES and CONTROLS, which is the count `--selftest` prints. A quote that cannot be read as one number ("48 + 40")
# is refused too: write the number `--selftest` prints, or none.
QUOTE_SKIP_DIRS = {"target", ".git", "node_modules", "__pycache__"}
QUOTE_SUFFIXES = (".md", ".py", ".sh", ".rs", ".txt")
MUST_QUOTE = re.compile(r"(\d+(?: \+ \d+)*) (?:must-flag shapes?|shapes its `--selftest`)")
CTL_QUOTE = re.compile(r"(?:(?!must-flag)[^.;|]){0,90}?\b(\d+(?: \+ \d+)*) controls?\b")


def quoted_count_problems(texts, n_must, n_ctl):
    """`texts`: {label: text}. Every quoted must-flag / control count that is not exactly (n_must, n_ctl) is a problem."""
    bad, seen = [], 0
    for label, raw in sorted(texts.items()):
        t = re.sub(r"\s+", " ", raw)
        for m in MUST_QUOTE.finditer(t):
            seen += 1
            for what, quote, want, start in [("must-flag shapes", m.group(1), n_must, m.start(1))] + (
                    [("controls", c.group(1), n_ctl, 0)] if (c := CTL_QUOTE.match(t[m.end():m.end() + 100])) else []):
                if quote.strip() != str(want):
                    bad.append(f"{label}: quotes {quote} {what}, the self-test carries {want}: {t[max(0, m.start() - 40):m.end() + 60]!r}")
    return bad, seen


def quote_texts(root):
    out = {}
    for top in ("governance", "scripts", "crates"):
        for d, dirs, fs in os.walk(os.path.join(root, top)):
            dirs[:] = [x for x in dirs if x not in QUOTE_SKIP_DIRS]
            for f in fs:
                if f.endswith(QUOTE_SUFFIXES):
                    p = os.path.join(d, f)
                    try:
                        out[os.path.relpath(p, root)] = open(p, errors="replace").read()
                    except OSError:
                        pass
    return out


def check_quoted_counts(root):
    n_must, n_ctl = len(BYPASSES) + 1, len(CONTROLS)
    bad, seen = quoted_count_problems(quote_texts(root), n_must, n_ctl)
    if bad:
        print("opkit_ns_drift: a quoted self-test count is stale (the self-test carries %d must-flag shapes and %d controls):\n%s" % (n_must, n_ctl, "\n".join(bad)))
        return 1
    print(f"opkit_ns_drift: quoted counts ok ({n_must} must-flag shapes, {n_ctl} controls; {seen} quotes checked)")
    return 0


# amendment 113: bare calls of a destructive helper primitive (each preceded by sourcing the helper)
PRIM_SHAPES = [
    ("a bare opkit_ns_isolate", 'OPKIT_SCRATCH=/var/tmp/x opkit_ns_isolate\n'),
    ("a bare opkit_ns_make_ro", 'opkit_ns_make_ro\n'),
    ("a bare opkit_ns_fresh_proc", 'opkit_ns_fresh_proc\n'),
    ("a bare opkit_ns_private_dev", 'opkit_ns_private_dev /var/tmp/x\n'),
    ("a bare opkit_ns_drop_host_fd", 'opkit_ns_drop_host_fd\n'),
    ("a primitive in an if", 'if opkit_ns_make_ro; then echo x; fi\n'),
    ("a primitive after &&", 'true && opkit_ns_fresh_proc\n'),
    ("a primitive in a bash -c string", "bash -c '. scripts/lib/opkit_ns.sh; opkit_ns_make_ro'\n"),
    ("a primitive behind env", 'env A=1 opkit_ns_private_dev /var/tmp/x\n'),
    ("a primitive behind sudo", 'sudo opkit_ns_isolate\n'),
    ("a primitive in a function body", 'f() { opkit_ns_make_ro; }\n'),
    ("a primitive in a command substitution", 'x=$(opkit_ns_fresh_proc)\n'),
]
PRIM_CONTROLS = [
    ("ns_run", 'ns_run true\n'),
    ("an echo naming a primitive", 'echo opkit_ns_isolate\n'),
    ("the non-destructive assertion", 'opkit_ns_assert\n'),
    ("a grep for a primitive", 'grep -q opkit_ns_make_ro scripts/lib/opkit_ns.sh\n'),
    ("a comment naming a primitive", '# opkit_ns_isolate is not called here\n'),
    ("an assertion inside ns_run", 'ns_run bash -c \'. "$OPKIT_LIB"; opkit_ns_assert\'\n'),
]

# (label, anchor in the real helper, replacement): each planted text must be flagged by diagnostic_problems
DIAG_SHAPES = [
    ("a bare >&2 in a function", '  [ -e "$p" ] || return 0\n', '  [ -e "$p" ] || return 0\n  echo "x" >&2\n'),
    ("1>&2", '  [ -e "$p" ] || return 0\n', '  [ -e "$p" ] || return 0\n  echo "x" 1>&2\n'),
    ("printf to /dev/stderr", '  [ -e "$p" ] || return 0\n', '  [ -e "$p" ] || return 0\n  printf x >/dev/stderr\n'),
    ("echo to /proc/self/fd/2", '  [ -e "$p" ] || return 0\n', '  [ -e "$p" ] || return 0\n  echo x >/proc/self/fd/2\n'),
    ("a bare echo on stdout in a non-value function", '  [ -e "$p" ] || return 0\n', '  [ -e "$p" ] || return 0\n  echo LEAK\n'),
    ("python writing sys.stderr", '    sys.exit(1)   # (amendment 118', '    sys.stderr.write("x"); sys.exit(1)   # (amendment 118'),
    ("opkit_say that writes without classifying fd 2", "  if opkit_ns_fd_why 2 $OPKIT_DIAG_ROOTS; then printf '%s\\n' \"$*\" >&2; return 0; fi\n", "  printf '%s\\n' \"$*\" >&2; return 0\n"),
    ("ns_run that unshares before it classifies fds 0-2", '  opkit_ns_std_fds_ok $OPKIT_DIAG_ROOTS || return 97\n', '  unshare --mount true\n  opkit_ns_std_fds_ok $OPKIT_DIAG_ROOTS || return 97\n'),
    ("ns_run that never classifies fds 0-2", '  opkit_ns_std_fds_ok $OPKIT_DIAG_ROOTS || return 97\n', ''),
    ("ns_run pre-checks with stderr not captured", 'pre=$(opkit_ns_prephase 2>&1)', 'pre=$(opkit_ns_prephase)'),
    ("the in-namespace shell mounts before it classifies fds", '    opkit_ns_std_fds_ok $OPKIT_DIAG_ROOTS || exit 97\n', ''),
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
    # EVERY accepted shape is reported (amendment 111), not only the first: a guard removed from the gate shows as the
    # shapes only IT refused, by name, so a mutation row is killed by its own shape and a coincidental failure is visible
    accepted = [label for label, text in BYPASSES if not check_text("bypass", text)]
    if accepted:
        for label in accepted:
            print(f"selftest: the bypass shape '{label}' was ACCEPTED")
        return 1
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
    # amendment 118: diagnostics never go through an unvetted descriptor. The real helper is clean; each planted shape is flagged by name.
    helper = open(os.path.join(root, "scripts", "lib", "opkit_ns.sh")).read()
    if diagnostic_problems(helper):
        print("selftest: the real helper writes to an unvetted descriptor:\n" + "\n".join(diagnostic_problems(helper))); return 1
    diag_accepted = []          # EVERY accepted shape is reported, so a guard removed from the gate shows as the shapes only IT refused
    for label, old, new in DIAG_SHAPES:
        planted_h = helper.replace(old, new, 1)
        if planted_h == helper:
            print(f"selftest: the diagnostic shape '{label}' did not apply (its anchor is gone from the helper)"); return 1
        if not diagnostic_problems(planted_h):
            diag_accepted.append(label)
    if diag_accepted:
        for label in diag_accepted:
            print(f"selftest: the diagnostic shape '{label}' was ACCEPTED")
        return 1
    # the quoted-count check refuses a stale quote, an unreadable one, and accepts the derived numbers and a quote with none
    nm, nc = len(BYPASSES) + 1, len(CONTROLS)
    for label, doc in [
        ("a stale must-flag count", f"the gate carries {nm - 1} must-flag shapes and {nc} controls"),
        ("a stale control count", f"the gate carries {nm} must-flag shapes and {nc + 1} controls"),
        ("a stale count with no controls", f"carries {nm + 3} must-flag shapes at this commit"),
        ("a sum instead of one number", f"{nm - 40} + {40} must-flag shapes"),
        ("a stale count across a line wrap", f"carries {nm - 1} must-flag\nshapes and {nc} controls"),
        ("a stale count in the shapes-its-selftest wording", f"flags the {nm - 1} shapes its `--selftest` lists"),
    ]:
        if not quoted_count_problems({"planted.md": doc}, nm, nc)[0]:
            print(f"selftest: a stale quoted count was ACCEPTED ({label})"); return 1
    for label, doc in [("the derived counts", f"**{nm} must-flag shapes and {nc} controls**"),
                       ("no number at all", "the must-flag shapes of the self-test, and its controls"),
                       ("an unrelated controls number", f"{nm} must-flag shapes. 57 controls tracked elsewhere")]:
        if quoted_count_problems({"planted.md": doc}, nm, nc)[0]:
            print(f"selftest: the derived quoted count was REFUSED ({label})"); return 1
    # amendment 113: the destructive primitives of the helper. NOT counted in the must-flag / control numbers above (those are
    # quoted in the amendments and derived from BYPASSES and CONTROLS); reported on their own.
    global _prims_override
    hp = open(os.path.join(root, "scripts", "lib", "opkit_ns.sh")).read()
    if primitive_problems(root, hp):
        print("selftest: the real helper's primitives do not all start with the precondition:\n" + "\n".join(primitive_problems(root, hp))); return 1
    _prims_override = derived_primitives(hp) | PRIM_BASE
    try:
        pre = ". scripts/lib/opkit_ns.sh\n"
        for label, text in PRIM_SHAPES:
            if not check_text("primitive", pre + text):
                print(f"selftest: the primitive shape '{label}' was ACCEPTED"); return 1
        for label, text in PRIM_CONTROLS:
            got = check_text("primitive-control", pre + text)
            if got:
                print(f"selftest: the primitive control '{label}' was REFUSED: {got}"); return 1
    finally:
        _prims_override = None
    new_fn = "\nopkit_ns_newmount() {\n  mount --bind /a /b\n}\n"
    new_ok = "\nopkit_ns_newmount() {\n  opkit_ns_precondition opkit_ns_newmount || return 97\n  mount --bind /a /b\n}\n"
    first = "  opkit_ns_precondition opkit_ns_make_ro || return 97\n"
    drop1 = "  opkit_ns_precondition opkit_ns_drop_host_fd || return 97\n"
    assert first in hp and drop1 in hp and "opkit_ns_make_ro() {" in hp, "selftest: the helper's make_ro / drop_host_fd lines moved"
    planted = [          # ORDER matters to the mutation rows: each guard of primitive_problems is first refused by its own shape
        ("a precondition that is not the first statement", hp.replace(first, "  true\n" + first, 1)),
        ("a precondition that does not return 97", hp.replace(first, "  opkit_ns_precondition opkit_ns_make_ro\n", 1)),
        ("a precondition naming another primitive", hp.replace(first, "  opkit_ns_precondition opkit_ns_isolate || return 97\n", 1)),
        ("no precondition at all in a base primitive", hp.replace(first, "", 1)),
        ("no precondition in opkit_ns_drop_host_fd (a primitive that mounts nothing)", hp.replace(drop1, "", 1)),
        ("a base primitive that is not defined", hp.replace("opkit_ns_drop_host_fd() {", "opkit_ns_drop_host_fd_renamed() {", 1)),
        ("a new function that mounts, with no precondition", hp + new_fn),
    ]
    for label, text in planted:
        if not primitive_problems(root, text):
            print(f"selftest: a helper with {label} was ACCEPTED"); return 1
    if primitive_problems(root, hp + new_ok):
        print("selftest: a new mounting function that starts with the precondition was REFUSED"); return 1
    # the primitive set is DERIVED: a new function that mounts is named, so its bare call in a test script is flagged
    _prims_override = derived_primitives(hp + new_ok) | PRIM_BASE
    try:
        if not check_text("primitive", ". scripts/lib/opkit_ns.sh\nopkit_ns_newmount\n"):
            print("selftest: a bare call of a primitive the helper gained (derived, not listed) was ACCEPTED"); return 1
    finally:
        _prims_override = None
    # check() itself is wired to the helper's primitives: a tree whose helper lacks the precondition in make_ro is refused as a whole
    import shutil, tempfile
    tmp = tempfile.mkdtemp(prefix="opkit-drift-selftest.")
    try:
        os.makedirs(os.path.join(tmp, "scripts", "lib"))
        for f in os.listdir(os.path.join(root, "scripts")):
            if f.startswith("test_") and f.endswith(".sh") or f == "operator_deploy_protected_host.sh":
                shutil.copy(os.path.join(root, "scripts", f), os.path.join(tmp, "scripts", f))
        for f in ("opkit_fixture.sh",):
            if os.path.exists(os.path.join(root, "scripts", "lib", f)):
                shutil.copy(os.path.join(root, "scripts", "lib", f), os.path.join(tmp, "scripts", "lib", f))
        with open(os.path.join(tmp, "scripts", "lib", "opkit_ns.sh"), "w") as fh:
            fh.write(hp)
        if check(tmp):
            print("selftest: check() refuses a copy of the real tree:\n" + "\n".join(check(tmp))); return 1
        with open(os.path.join(tmp, "scripts", "lib", "opkit_ns.sh"), "w") as fh:
            fh.write(hp.replace(first, "", 1))
        if not check(tmp):
            print("selftest: check() on a tree whose helper has no precondition in opkit_ns_make_ro was ACCEPTED"); return 1
        with open(os.path.join(tmp, "scripts", "lib", "opkit_ns.sh"), "w") as fh:
            fh.write(hp.replace('  [ -e "$p" ] || return 0\n', '  [ -e "$p" ] || return 0\n  echo x >&2\n', 1))
        if not check(tmp):
            print("selftest: check() on a tree whose helper writes to fd 2 directly was ACCEPTED"); return 1
    finally:
        shutil.rmtree(tmp)
    print(f"selftest: ok ({len(BYPASSES) + 1} must-flag shapes refused, {len(CONTROLS)} controls accepted, plus the kit-destination shapes and "
          f"{len(PRIM_SHAPES) + len(planted) + 1} primitive shapes and {len(DIAG_SHAPES)} diagnostic-channel shapes)"); return 0


if __name__ == "__main__":
    a = [x for x in sys.argv[1:] if x not in ("--selftest", "--check-quoted-counts")]
    root = a[0] if a else os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
    if "--check-quoted-counts" in sys.argv:
        sys.exit(check_quoted_counts(root))
    if "--selftest" in sys.argv:
        sys.exit(selftest(root))
    bad = check(root)
    if bad:
        print("opkit_ns_drift: a violation:\n" + "\n".join(bad)); sys.exit(1)
    print("opkit_ns_drift: ok (every kit run, --apply and controlled-build verb in a test script is itself an ns_run command; every kit write target is under a shadowed destination)")
