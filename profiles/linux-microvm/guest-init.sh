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
#   B263-LOADED axon=<sha256> program=<sha256>
#   B263-START
#   B263-OUT stdout=<sha256> exit=<n>
#   B263-DONE
# The host compares the serial digests against the bytes it extracts from the
# drive, so neither channel alone can vouch for a result.
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
[ -f /work/job/program.ax ] || fail "no-program"
rm -rf /work/out
mkdir -p /work/out

AXON_SHA=$(sha256sum /usr/bin/axon | cut -d' ' -f1)
PROG_SHA=$(sha256sum /work/job/program.ax | cut -d' ' -f1)
echo "B263-LOADED axon=$AXON_SHA program=$PROG_SHA"

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
    # host reaches the workload except these three fixed values.
    # shellcheck disable=SC2086
    exec env -i PATH=/bin:/usr/bin HOME=/work XDG_CACHE_HOME=/tmp/cache \
        /usr/bin/axon run /work/job/program.ax $ARGS
) > /work/out/stdout 2> /work/out/stderr < /dev/null
RC=$?
echo "$RC" > /work/out/exit

NETDEVS=$(ls /sys/class/net | tr '\n' ' ')
BLKDEVS=$(ls /sys/block | tr '\n' ' ')
ROOTMNT=$(grep ' / ' /proc/mounts | head -1 | cut -d' ' -f4 | cut -d, -f1)
printf '{"netdevs":"%s","blockdevs":"%s","root_mount_mode":"%s","axon_sha256":"%s","program_sha256":"%s","pids_max":"%s","pids_events":"%s","memory_max":"%s","oom_kills":"%s"}\n' \
    "$NETDEVS" "$BLKDEVS" "$ROOTMNT" "$AXON_SHA" "$PROG_SHA" \
    "$(cat /sys/fs/cgroup/job/pids.max 2>/dev/null)" \
    "$(grep max /sys/fs/cgroup/job/pids.events 2>/dev/null | cut -d' ' -f2)" \
    "$(cat /sys/fs/cgroup/job/memory.max 2>/dev/null)" \
    "$(grep '^oom_kill ' /sys/fs/cgroup/job/memory.events 2>/dev/null | cut -d' ' -f2)" \
    > /work/out/guest.json

OUT_SHA=$(sha256sum /work/out/stdout | cut -d' ' -f1)
sync
umount /work
echo "B263-OUT stdout=$OUT_SHA exit=$RC"
echo "B263-DONE"
reboot -f
