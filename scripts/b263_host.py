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


def identity(env=None):
    env = os.environ if env is None else env
    machine_id = _first_line("/etc/machine-id")
    virt = _detect_virt()
    kernel = platform.release()
    wsl = "microsoft" in kernel.lower()
    hostname = platform.node()
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
    print(json.dumps(identity()))
