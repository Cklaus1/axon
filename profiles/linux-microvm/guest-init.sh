#!/bin/sh
# /init for the protected Linux microVM profile (B263).
#
# Runs as PID 1 on a READ-ONLY squashfs root (/dev/vda). The only writable
# persistent storage is the workspace drive (/dev/vdb, ext4), which carries the
# job in /job and receives the result in /out. There is no network device and
# no MMDS: the job's inputs arrive ONLY on the workspace drive.
#
# Protocol (host <-> guest), all on the workspace drive:
#   in : /job/program.ax           the Axon program to run (required)
#        /job/args                 optional, one argv word per line
#   out: /out/stdout /out/stderr   workload output
#        /out/exit                 workload exit status (decimal)
#        /out/guest.json           guest-observed facts (net devices, mounts,
#                                  block devices, loaded-artifact digests)
# and on the serial console (ttyS0), which the host captures independently:
#   B263-BOOT <kernel release>
#   B263-LOADED axon=<sha256> program=<sha256> init=<sha256>
#   B263-POLICY sha=<sha256>   (or `absent` / `undecodable` / `ambiguous words=N`)
#   B263-START
#   B263-OUT stdout=<sha256> exit=<n>
#   B263-DONE
# The host compares the serial digests against the bytes it extracts from the
# drive, so neither channel alone can vouch for a result.
#
# Boot policy (ACF-G25, closes x1): the host passes the capability policy as ONE
# kernel-cmdline word `axon.policy=<base64 of the axon-vm-mmds/1 JSON>`. This
# script only REPORTS it (serial `B263-POLICY sha=<sha256 of the decoded JSON>`,
# or `B263-POLICY absent|undecodable`); the DECISION is axon-guest-init's, which
# the workload is exec'd under. It refuses (exit 1, reason on /out/stderr, the
# program never runs) when the policy is absent, empty, labels-only, malformed,
# duplicated or possibly truncated, and exports the effect ceiling / token cap
# and installs seccomp otherwise. The no-policy escape hatch is compiled out of
# the image's axon-guest-init (non-default cargo feature).
#
# Every path ends in `reboot -f` (reboot=k -> Firecracker exits). If this
# script itself dies, the kernel panics (PID 1 exit) and panic=1 reboots.

PATH=/bin
export PATH

mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev 2>/dev/null
mount -t tmpfs -o nosuid,nodev,size=16m tmpfs /tmp

echo "B263-BOOT $(uname -r)"
echo "B263-VERSION $(cat /proc/version)"

# axon reserves a 1 GiB stack for its interpreter thread (main.rs, spawn with
# stack_size 1 GiB). Under the default heuristic overcommit a 256 MiB guest
# refuses that reservation (measured: "spawn worker thread: EAGAIN", exit 101).
# Always-overcommit lets the untouched reservation exist; the real bound is
# unchanged, since pages actually touched still come out of the guest's fixed
# RAM, and the host cgroup still bounds the VMM that backs it.
echo 1 > /proc/sys/vm/overcommit_memory

fail() {
    echo "B263-FAIL $1"
    sync
    reboot -f
}

mount -t ext4 -o nosuid,nodev /dev/vdb /work || fail "workspace-mount"

# PSV mode (v022-psv-protocol.md §4): the cmdline names a launch manifest. The
# inputs are three READ-ONLY drives, mounted nodev,nosuid,noexec,noacl — noacl
# so the test uid's access is the mode bits alone (PSV-2: an ACL the digest
# cannot see must not change what the child can read):
#   /dev/vdc -> /in/candidate   /dev/vdd -> /in/suite   /dev/vde -> /in/job
# and the trusted runner (axon-psv-runner) checks them before anything runs.
set -f
PSV_MSHA=""; PSV_WORDS=0
for w in $(cat /proc/cmdline); do
    case "$w" in axon.psv.manifest=*) PSV_MSHA="${w#axon.psv.manifest=}"; PSV_WORDS=$((PSV_WORDS + 1)) ;; esac
done
set +f
[ "$PSV_WORDS" -le 1 ] || fail "psv-ambiguous"
if [ -n "$PSV_MSHA" ]; then
    mount -t ext4 -o ro,nodev,nosuid,noexec,noacl /dev/vdc /in/candidate || fail "psv-candidate-mount"
    mount -t ext4 -o ro,nodev,nosuid,noexec,noacl /dev/vdd /in/suite || fail "psv-suite-mount"
    mount -t ext4 -o ro,nodev,nosuid,noexec,noacl /dev/vde /in/job || fail "psv-job-mount"
    echo "PSV-MANIFEST sha256=$PSV_MSHA"
else
    [ -f /work/job/program.ax ] || fail "no-program"
fi
rm -rf /work/out
mkdir -p /work/out
[ -n "$PSV_MSHA" ] && { mount --bind /work/out /out || fail "psv-out-bind"; }

AXON_SHA=$(sha256sum /usr/bin/axon | cut -d' ' -f1)
PROG_SHA=""
[ -n "$PSV_MSHA" ] || PROG_SHA=$(sha256sum /work/job/program.ax | cut -d' ' -f1)
INIT_SHA=$(sha256sum /usr/bin/axon-guest-init | cut -d' ' -f1)
RUNNER_SHA=$(sha256sum /usr/bin/axon-psv-runner 2>/dev/null | cut -d' ' -f1)
echo "B263-LOADED axon=$AXON_SHA program=$PROG_SHA init=$INIT_SHA runner=$RUNNER_SHA"

# Report the policy the host put on the cmdline. `set -f`: a cmdline word must
# not be glob-expanded. The digest is over the DECODED JSON bytes, i.e. the
# bytes the host serialised, so the host can compare it to what it sent.
# (Extracted between the markers and run by axon-guest-init's
# tests/b263_profile_wiring.rs — keep it self-contained.)
# >>> policy-report
set -f
# Words, not lines, on purpose: the policy is one cmdline WORD.
# shellcheck disable=SC2013
POLICY_WORDS=0
POLICY_B64=""
for w in $(cat /proc/cmdline); do
    case "$w" in
        axon.policy=*) POLICY_B64="${w#axon.policy=}"; POLICY_WORDS=$((POLICY_WORDS + 1)) ;;
    esac
done
set +f
POLICY_SHA=""
if [ "$POLICY_WORDS" -eq 0 ]; then
    echo "B263-POLICY absent"
elif [ "$POLICY_WORDS" -gt 1 ]; then
    echo "B263-POLICY ambiguous words=$POLICY_WORDS"
elif printf '%s' "$POLICY_B64" | base64 -d > /tmp/policy.json 2>/dev/null; then
    POLICY_SHA=$(sha256sum /tmp/policy.json | cut -d' ' -f1)
    echo "B263-POLICY sha=$POLICY_SHA"
else
    echo "B263-POLICY undecodable"
fi
rm -f /tmp/policy.json
# <<< policy-report

# Guest-side process bound: the workload runs in its own cgroup with a pids
# ceiling. This bounds fork bombs INSIDE the guest; the host bound on the VMM's
# threads is the jailer cgroup and is asserted separately.
mount -t cgroup2 cgroup2 /sys/fs/cgroup 2>/dev/null
mkdir -p /sys/fs/cgroup/job
echo "+pids +memory" > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null
echo 32 > /sys/fs/cgroup/job/pids.max 2>/dev/null
# Memory ceiling for the workload = guest RAM minus 48 MiB kept for PID 1 and
# the page cache backing the squashfs executables. Without it an over-asking
# workload does not get OOM-killed: with no swap the guest thrashes executable
# pages and livelocks until the host wall clock fires (measured). With it, the
# cgroup OOM killer ends the workload promptly and PID 1 still reports.
MEMKB=$(sed -n 's/^MemTotal: *\([0-9]*\) kB/\1/p' /proc/meminfo)
echo $(( (MEMKB - 49152) * 1024 )) > /sys/fs/cgroup/job/memory.max 2>/dev/null
echo 1 > /sys/fs/cgroup/job/memory.oom.group 2>/dev/null

ARGS=""
[ -f /work/job/args ] && ARGS=$(cat /work/job/args)

echo "B263-START"
(
    # "0" = the writing process; `$$` would name PID 1 inside a subshell.
    echo 0 > /sys/fs/cgroup/job/cgroup.procs
    cd /work
    # Empty environment by construction: nothing from the kernel cmdline or the
    # host reaches the workload except these three fixed values and what
    # axon-guest-init derives from the cmdline POLICY (AXON_ALLOWED_EFFECTS,
    # AXON_BUDGET_TOKENS, and the labels). `env -i` also means a
    # `NAME=value` cmdline word the kernel copied into PID 1's environment
    # cannot reach axon-guest-init either.
    if [ -n "$PSV_MSHA" ]; then
        # The runner takes no arguments: its paths are fixed and the manifest
        # digest is the cmdline word it reads itself.
        exec env -i PATH=/bin:/usr/bin HOME=/tmp XDG_CACHE_HOME=/tmp/cache \
            /usr/bin/axon-guest-init /usr/bin/axon-psv-runner
    fi
    # shellcheck disable=SC2086
    exec env -i PATH=/bin:/usr/bin HOME=/work XDG_CACHE_HOME=/tmp/cache \
        /usr/bin/axon-guest-init /usr/bin/axon run /work/job/program.ax $ARGS
) > /work/out/stdout 2> /work/out/stderr < /dev/null
RC=$?
echo "$RC" > /work/out/exit

NETDEVS=$(ls /sys/class/net | tr '\n' ' ')
BLKDEVS=$(ls /sys/block | tr '\n' ' ')
ROOTMNT=$(grep ' / ' /proc/mounts | head -1 | cut -d' ' -f4 | cut -d, -f1)
printf '{"netdevs":"%s","blockdevs":"%s","root_mount_mode":"%s","axon_sha256":"%s","program_sha256":"%s","guest_init_sha256":"%s","policy_sha256":"%s","pids_max":"%s","pids_events":"%s","memory_max":"%s","oom_kills":"%s"}\n' \
    "$NETDEVS" "$BLKDEVS" "$ROOTMNT" "$AXON_SHA" "$PROG_SHA" "$INIT_SHA" "$POLICY_SHA" \
    "$(cat /sys/fs/cgroup/job/pids.max 2>/dev/null)" \
    "$(grep max /sys/fs/cgroup/job/pids.events 2>/dev/null | cut -d' ' -f2)" \
    "$(cat /sys/fs/cgroup/job/memory.max 2>/dev/null)" \
    "$(grep '^oom_kill ' /sys/fs/cgroup/job/memory.events 2>/dev/null | cut -d' ' -f2)" \
    > /work/out/guest.json

OUT_SHA=$(sha256sum /work/out/stdout | cut -d' ' -f1)
if [ -n "$PSV_MSHA" ]; then
    # /init's OWN digest of the verdict file (the runner's line is only on
    # /out/stdout); the host compares it with the returned drive.
    if [ -f /work/out/verdict.json ]; then
        echo "PSV-VERDICT-INIT sha256=$(sha256sum /work/out/verdict.json | cut -d' ' -f1)"
    else
        echo "PSV-VERDICT-INIT none"
    fi
    umount /out 2>/dev/null
fi
sync
umount /work
echo "B263-OUT stdout=$OUT_SHA exit=$RC"
echo "B263-DONE"
reboot -f
