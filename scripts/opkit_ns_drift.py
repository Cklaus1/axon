#!/usr/bin/env python3
"""opkit_ns_drift.py -- no test may run the operator deployment kit, or any other
state-changing verb, outside the namespace helper (amendment 92).

Incident 2026-10-06: a guard-removal experiment made a kit refusal test a REAL
root `--apply` on the dev host. The structural cure is scripts/lib/opkit_ns.sh
(`ns_run`: a private mount namespace, tmpfs over every destination, isolation
PROVED before the kit starts). This gate keeps it the only door:

  * every non-comment line, in any test script, that RUNS the kit
    (`$KIT`, `operator_deploy_protected_host.sh` as a command) or carries
    `--apply` must go through `ns_run` (the outer shell) or `kit` (the helper
    inside an already-proved namespace, which re-asserts before each call);
  * the kit-running scripts must source/use opkit_ns.sh at all;
  * `--selftest` plants an unwrapped `--apply` line and a wrapped one in
    scratch copies and requires this check to refuse the first and accept the second.

Exit 0 clean, 1 a violation.  Usage: opkit_ns_drift.py [--selftest] [ROOT]
"""
import os, re, sys, tempfile

KIT_RUN = re.compile(r'\$\{?KIT\}?\b|operator_deploy_protected_host\.sh')
APPLY = re.compile(r'(?<![\w-])--apply(?![\w-])')
# lines that merely NAME the kit (assignment, copy, hash, parse, mention in a string) run nothing
NOT_A_RUN = re.compile(
    r'^\s*(KIT=|cp\b.*\$KIT|cmp\b|rm\b|python3\b.*\$KIT|for f in|ARGS=|.*\bsed\b.*\$KIT)|'
    r'^\s*\w+\(\)\s*\{.*bash "\$KIT"')            # kit() { ...; bash "$KIT" "$@"; } is the helper itself
WRAPPED = re.compile(r'(?<![\w-])(ns_run|kit)(?![\w-])')

def scripts(root):
    for sub in ("scripts",):
        for d, _, fs in os.walk(os.path.join(root, sub)):
            for f in fs:
                if f.startswith("test_") and f.endswith(".sh"):
                    yield os.path.join(d, f)

def check_text(path, text):
    bad, in_heredoc, uses = [], None, False
    lines = text.splitlines()
    for n, line in enumerate(lines, 1):
        s = line.strip()
        if s.startswith("#"): continue
        if in_heredoc:
            if s == in_heredoc: in_heredoc = None
            # the heredoc BODY is still shell we judge (ns.sh is one); python bodies hold no kit run
        else:
            m = re.search(r"<<-?\s*'?(\w+)'?\s*$", line)
            if m: in_heredoc = m.group(1)
        runs = KIT_RUN.search(line) and not NOT_A_RUN.match(line)
        # a quoted prose mention (inside an echo / label string) is not a run: require a command-ish context
        if runs and not re.search(r'\bbash\b|\bsh\b|"\$KIT"|\$KIT\b', line): runs = False
        applies = APPLY.search(line) and (KIT_RUN.search(line) or 'bash' in line or 'ARGS' in line)
        if (runs or applies) and not WRAPPED.search(line):
            # continuation lines: the wrapper may be on the logical line's first physical line
            j = n - 2
            joined = line
            while j >= 0 and lines[j].rstrip().endswith("\\"):
                joined = lines[j] + joined; j -= 1
            if not WRAPPED.search(joined):
                bad.append(f"{path}:{n}: {s[:110]}")
    if re.search(r'ns_run|\bkit\b', text): uses = True
    if (KIT_RUN.search(text)) and "opkit_ns.sh" not in text:
        bad.append(f"{path}: runs the kit but never uses scripts/lib/opkit_ns.sh")
    return bad

def check(root):
    bad = []
    for p in scripts(root):
        bad += check_text(p, open(p).read())
    return bad

def selftest(root):
    src = open(os.path.join(root, "scripts", "test_operator_deploy.sh")).read()
    base = check_text("real", src)
    if base:
        print("selftest: the real test is not clean:\n" + "\n".join(base)); return 1
    attack = src.replace('ns_run bash "$KIT" "${ARGS[@]}" --no-systemctl --apply',
                         'bash "$KIT" "${ARGS[@]}" --no-systemctl --apply', 1)
    assert attack != src, "selftest: attack line not found"
    if not check_text("attack", attack):
        print("selftest: an unwrapped kit --apply was ACCEPTED"); return 1
    # a bare --apply on a non-kit variable line, in a fresh script
    if not check_text("fresh", 'sudo bash scripts/x.sh --apply\nbash "$KIT" --from c --apply\n'):
        print("selftest: a fresh unwrapped --apply was ACCEPTED"); return 1
    if check_text("ok", '. scripts/lib/opkit_ns.sh\nns_run bash "$KIT" --from c --apply\n'):
        print("selftest: a wrapped call was REFUSED"); return 1
    print("selftest: ok"); return 0

if __name__ == "__main__":
    a = [x for x in sys.argv[1:] if x != "--selftest"]
    root = a[0] if a else os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
    if "--selftest" in sys.argv: sys.exit(selftest(root))
    bad = check(root)
    if bad:
        print("opkit_ns_drift: kit run outside ns_run/kit:\n" + "\n".join(bad)); sys.exit(1)
    print("opkit_ns_drift: ok (every kit run and --apply in a test script goes through ns_run)")
