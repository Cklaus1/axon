#!/usr/bin/env python3
"""CE-9 spike: seccomp-bpf for an IO-only interpreter run (scratch, not product).

ALLOW   the interpreter substrate (memory, threads, signals, fd-relative I/O,
        getrandom) plus execve (the filter is installed BEFORE init execs axon).
EACCES  every path-taking FS syscall (open/openat/stat/readlink/mkdir/...).
KILL    everything else, and any non-x86_64 syscall ABI.

usage: mkfilter.py {errno|kill}   (FS action)  -> base64 on stdout
"""
import base64
import struct
import sys

ALLOW = [0, 1, 3, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 19, 20, 24, 25, 28, 35,
         39, 56, 59, 60, 72, 131, 158, 186, 202, 204, 218, 228, 230, 231, 234, 273,
         302, 318, 324, 334]
FS = [2, 4, 6, 21, 79, 80, 82, 83, 84, 85, 86, 87, 88, 89, 90, 92, 217, 257, 258,
      262, 263, 264, 267, 269, 316, 332, 437, 439]

AUDIT_ARCH_X86_64 = 0xC000003E
RET_ALLOW = 0x7FFF0000
RET_KILL_PROCESS = 0x80000000
RET_ERRNO_EACCES = 0x00050000 | 13
LD_W_ABS, JEQ_K, RET_K = 0x20, 0x15, 0x06


def ins(code, jt, jf, k):
    return struct.pack("<HBBI", code, jt, jf, k)


def build(fs_action):
    p = []
    p.append(ins(LD_W_ABS, 0, 0, 4))                    # seccomp_data.arch
    p.append(ins(JEQ_K, 1, 0, AUDIT_ARCH_X86_64))
    p.append(ins(RET_K, 0, 0, RET_KILL_PROCESS))
    p.append(ins(LD_W_ABS, 0, 0, 0))                    # seccomp_data.nr
    # x32 ABI (bit 30) never matches the lists below -> default KILL.
    n_a, n_f = len(ALLOW), len(FS)
    # layout after the nr load: ALLOW jeqs, FS jeqs, RET KILL, RET FS, RET ALLOW
    for i, nr in enumerate(ALLOW):
        to_allow = (n_a - 1 - i) + n_f + 2
        p.append(ins(JEQ_K, to_allow, 0, nr))
    for i, nr in enumerate(FS):
        to_fs = (n_f - 1 - i) + 1
        p.append(ins(JEQ_K, to_fs, 0, nr))
    p.append(ins(RET_K, 0, 0, RET_KILL_PROCESS))
    p.append(ins(RET_K, 0, 0, fs_action))
    p.append(ins(RET_K, 0, 0, RET_ALLOW))
    return b"".join(p)


if __name__ == "__main__":
    act = {"errno": RET_ERRNO_EACCES, "kill": RET_KILL_PROCESS}[sys.argv[1]]
    sys.stdout.write(base64.b64encode(build(act)).decode())
