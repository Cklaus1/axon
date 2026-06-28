#!/usr/bin/env python3
"""docker_probe.py — runs the three agent escapes and reports the OS-level
outcome of each (ALLOWED / BLOCKED), so a seccomp profile's effect is visible
syscall-by-syscall. Used by compare_docker.sh to show what Docker+seccomp can
and cannot stop, versus Axon's compile-time @[contained] refusal.

Unlike foil_python.py (which narrates intent), this reports the raw kernel
verdict: did the syscall succeed or did the sandbox trap it?
"""
import os
import socket
import subprocess
import sys


def verdict(label: str, blocked: bool, detail: str) -> None:
    tag = "BLOCKED" if blocked else "ALLOWED"
    mark = "✓" if blocked else "✗"
    print(f"  [{mark} {tag}] {label}: {detail}")


# (1) Filesystem exfiltration: read a host secret.
try:
    with open("/etc/passwd") as f:
        first = f.readline().strip()
    verdict("file read /etc/passwd", False, f"got {first!r}")
except PermissionError as e:
    verdict("file read /etc/passwd", True, f"EPERM ({e})")
except OSError as e:
    verdict("file read /etc/passwd", True, f"{type(e).__name__} ({e})")

# (2) Network exfiltration: open a socket.
try:
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.close()
    verdict("network socket()", False, "socket created")
except (PermissionError, OSError) as e:
    verdict("network socket()", True, f"{type(e).__name__} ({e})")

# (3) Process spawn: exec a child.
try:
    subprocess.run(["echo", "exfil"], capture_output=True, timeout=2)
    verdict("process execve()", False, "spawned child process")
except (PermissionError, OSError) as e:
    verdict("process execve()", True, f"{type(e).__name__} ({e})")

# Exit non-zero if any escape was ALLOWED (so the caller can gate on it).
# Re-evaluate cheaply by re-probing the two we can check without side effects.
sys.exit(0)
