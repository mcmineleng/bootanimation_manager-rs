#!/system/bin/sh

MODDIR=$(cd "$(dirname "$0")" && pwd)
LOG="$MODDIR/logs.txt"

MODDIR="$MODDIR" "$MODDIR/utils" apply 2>>"$LOG"
