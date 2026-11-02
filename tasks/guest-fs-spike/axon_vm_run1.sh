#!/bin/bash
# usage: run1.sh <label> <effects-or-UNSET> <program> [kernel] [initrd]
S=/var/tmp/r51-tmp/CE9Spike
VM=/home/cklaus/projects/axon-lanes/.doc-build/CE9Spike/debug/axon-vm
K=${4:-$S/img/vmlinuz}; I=${5:-$S/img/initramfs.cpio.gz}
D=$(sha256sum $K|cut -d' ' -f1)
export HOME=$S/home
if [ "$2" = UNSET ]; then unset AXON_VM_ALLOWED_EFFECTS; else export AXON_VM_ALLOWED_EFFECTS=$2; fi
timeout 60 $VM run "$3" --kernel $K --initrd $I --expect-digest $D --json > out-$1.json 2> err-$1.txt
echo $? > rc-$1.txt
