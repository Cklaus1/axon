#!/usr/bin/env python3
"""b263_host.py — the host a B263 qualification run measured (amendment 65).

The B263 record's `host` and `caveat` were constants ("WSL2-nested" and a
Hyper-V caveat), so a record made on any host claimed to come from that one,
and every protected receipt carries `qualification-host:<host>` verbatim.
`host` is now the MEASURED identity (hostname, /etc/machine-id,
systemd-detect-virt, kernel), prefixed by the operator's own name for the
host (B263_HOST_LABEL, `b263_qualify.sh --host-label`) when given. The caveat
is the operator's statement (B263_CAVEAT, `--caveat`), else one derived from
the measured virtualization — never a fixed text.

`python3 scripts/b263_host.py` prints identity() as JSON.
`python3 scripts/b263_host.py --x3-reason` prints x3_reason(measure()): the
reason the qualification records (and the operator SIGNS) for its BLOCKED
`x3_l0_hypervisor_boundary` row. It was a constant naming WSL2 and Hyper-V, so
a record made on bare metal or under another hypervisor asserted, under the
operator's signature, a false fact about the host (amendment 70). Every claim
it makes about the host is now one of the measured facts.
"""
import json
import os
import platform
import subprocess


def _first_line(p):
    try:
        with open(p) as f:
            return f.readline().strip() or None
    except OSError:
        return None


def _detect_virt():
    try:
        r = subprocess.run(["systemd-detect-virt"], capture_output=True, text=True)
        return r.stdout.strip() or "none"
    except OSError:
        return "unknown"


def measure():
    """The host facts every derived statement is made from (and nothing else)."""
    kernel = platform.release()
    virt = _detect_virt()
    return {"hostname": platform.node(), "machine_id": _first_line("/etc/machine-id"),
            "virt": virt, "kernel": kernel,
            "wsl": "microsoft" in kernel.lower() or virt == "wsl"}


def x3_reason(facts):
    """The BLOCKED reason for x3_l0_hypervisor_boundary, from `facts` alone.

    The row is BLOCKED on every host (nothing here asserts that a guest cannot
    escape through the layer below the host kernel); what differs per host is
    WHAT that layer is, and the reason may only say what was measured."""
    virt, kernel = facts.get("virt") or "unknown", facts.get("kernel") or "unknown"
    measured = f"systemd-detect-virt: {virt}; kernel {kernel}"
    tail = "no assertion here covers a guest escape through it."
    if facts.get("wsl"):
        return (f"Host is WSL2 ({measured}) with nested KVM under Hyper-V (operator decision D2). "
                "The L0 hypervisor and the WSL2 utility VM are outside the qualified boundary; "
                "no assertion here covers a guest escape through L0/L1.")
    if virt == "none":
        return (f"Host measured as bare metal ({measured}): the host kernel's KVM is the "
                f"hypervisor this profile runs on, and it is outside what this run can test; {tail}")
    if virt == "unknown":
        return (f"Host virtualization could not be measured ({measured}): what lies below the host "
                f"kernel is not known, and it is outside the qualified boundary; {tail}")
    return (f"Host is a virtual machine ({measured}): KVM here is nested under that hypervisor "
            f"(L0), which is outside the qualified boundary; {tail}")


def identity(env=None, facts=None):
    env = os.environ if env is None else env
    facts = measure() if facts is None else facts
    machine_id, virt, kernel = facts["machine_id"], facts["virt"], facts["kernel"]
    wsl, hostname = facts["wsl"], facts["hostname"]
    label = (env.get("B263_HOST_LABEL") or "").strip()
    measured = (f"hostname {hostname}; machine-id {machine_id or 'unreadable'}; virt {virt}"
                f"{' (WSL2)' if wsl else ''}; kernel {kernel}")
    host = f"{label} ({measured})" if label else measured
    caveat = (env.get("B263_CAVEAT") or "").strip()
    if not caveat:
        if virt not in ("none",) or wsl:
            caveat = (f"Measured virtualization: {virt}{' under WSL2 (Hyper-V)' if wsl else ''}; "
                      "the hypervisor below this host is outside the qualified boundary.")
        else:
            caveat = ("Measured virtualization: none (bare metal per systemd-detect-virt); the "
                      "host kernel and firmware are the boundary.")
        caveat += " (derived by b263_host.py; the operator states it with --caveat)"
    facts = {"hostname": hostname, "machine_id": machine_id, "virt": virt, "wsl": wsl,
             "host_label": label or None}
    return {"host": host, "caveat": caveat, "facts": facts}


if __name__ == "__main__":
    import sys
    if sys.argv[1:] == ["--x3-reason"]:
        print(x3_reason(measure()))
    elif sys.argv[1:]:
        sys.exit(f"usage: {sys.argv[0]} [--x3-reason]")
    else:
        print(json.dumps(identity()))
