#!/usr/bin/env python3
"""Host mirror of axon-guest-init's child path, for quick filter checks.

usage: hostgate.py B64FILTER [--source PATH] -- argv...
"""
import base64
import ctypes
import os
import sys

b64 = sys.argv[1]
rest = sys.argv[2:]
source = None
if rest[0] == "--source":
    source = rest[1]
    rest = rest[2:]
assert rest[0] == "--"
argv = rest[1:]
if source:
    fd = os.open(source, os.O_RDONLY)
    os.set_inheritable(fd, True)
    argv = argv + ["--source-fd", str(fd), source]

prog = base64.b64decode(b64)


class SockFprog(ctypes.Structure):
    _fields_ = [("len", ctypes.c_ushort), ("filter", ctypes.c_void_p)]


buf = ctypes.create_string_buffer(prog, len(prog))
fprog = SockFprog(len(prog) // 8, ctypes.cast(buf, ctypes.c_void_p))
libc = ctypes.CDLL(None, use_errno=True)
PR_SET_NO_NEW_PRIVS, PR_SET_SECCOMP, SECCOMP_MODE_FILTER = 38, 22, 2
assert libc.prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == 0
r = libc.prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, ctypes.byref(fprog), 0, 0)
assert r == 0, ctypes.get_errno()
os.execv(argv[0], argv)
