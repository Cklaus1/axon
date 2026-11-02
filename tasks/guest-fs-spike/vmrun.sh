#!/bin/bash
# CE-9 spike: boot the Linux-profile guest once (Firecracker, no jailer).
# usage: vmrun.sh LABEL PROGRAM.ax MODE(path|fd) FILTER(none|errno|kill)
# prints: LABEL rc=<guest workload exit> and the workload's stdout/stderr.
set -u
L=/var/tmp/r51-tmp/CE9Spike/linux
LABEL=$1 PROG=$2 MODE=$3 FILTER=$4
O=$L/runs/$LABEL
rm -rf "$O"; mkdir -p "$O/stage/job"
cp "$PROG" "$O/stage/job/program.ax"
echo "$MODE" > "$O/stage/job/mode"
mkfs.ext4 -q -F -O ^has_journal -E root_owner=0:0 -d "$O/stage" "$O/work.img" 64M
if [ "$FILTER" = none ]; then
    POL='{"schema":"axon-vm-mmds/1","run_id":"ce9","principal":"ce9","allowed_effects":["IO"],"budget_tokens":0}'
else
    POL="{\"schema\":\"axon-vm-mmds/1\",\"run_id\":\"ce9\",\"principal\":\"ce9\",\"allowed_effects\":[\"IO\"],\"budget_tokens\":0,\"seccomp_bpf_b64\":\"$("$L/mkfilter.py" "$FILTER")\"}"
fi
printf '%s' "$POL" > "$O/policy.json"
BOOT="console=ttyS0 reboot=k panic=1 pci=off acpi=off root=/dev/vda rootfstype=squashfs ro init=/init loglevel=4 axon.policy=$(base64 -w0 < "$O/policy.json")"
echo "cmdline_bytes=${#BOOT}" > "$O/meta"
cat > "$O/vm.json" <<EOF
{
  "boot-source": {"kernel_image_path": "$L/vmlinux", "boot_args": "$BOOT"},
  "drives": [
    {"drive_id": "rootfs", "path_on_host": "$L/rootfs.sqfs", "is_root_device": true, "is_read_only": true},
    {"drive_id": "work", "path_on_host": "$O/work.img", "is_root_device": false, "is_read_only": false}
  ],
  "machine-config": {"vcpu_count": 1, "mem_size_mib": 256}
}
EOF
timeout 60 /usr/local/bin/firecracker --no-api --config-file "$O/vm.json" --api-sock "$O/fc.sock" \
    > "$O/serial.log" 2>&1 < /dev/null
echo "fc_rc=$?" >> "$O/meta"
mkdir -p "$O/out"
for f in stdout stderr exit; do
    debugfs -R "dump /out/$f $O/out/$f" "$O/work.img" >/dev/null 2>&1
done
echo "== $LABEL ($MODE, filter=$FILTER, $(basename "$PROG")) workload_rc=$(cat "$O/out/exit" 2>/dev/null || echo NONE) $(tr '\n' ' ' < "$O/meta")"
grep -a -E 'B263-(POLICY|START|OUT|FAIL)' "$O/serial.log" | tr -d '\r'
echo "-- stdout"; cat "$O/out/stdout" 2>/dev/null
echo "-- stderr"; grep -v W2001 "$O/out/stderr" 2>/dev/null
