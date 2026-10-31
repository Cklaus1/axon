#!/bin/sh
# Dry-run generator (no model): answers every prompt with the program in
# $GENVIAB_ORACLE (genviab.py sets it per task: the task's reference.ax, or
# an empty main for the empty generator). Proves the harness tells a working
# generator from a broken one before any paid call.
cat > /dev/null
exec cat "${GENVIAB_ORACLE:?GENVIAB_ORACLE is not set}"
