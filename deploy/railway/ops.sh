#!/bin/sh
# Operator steps inside the Zecraft container on Railway, run as root
# through `railway ssh` (plain words only, so nothing depends on remote
# quoting):
#
#   ops.sh held          exit 0 when no Zecraft process is running
#                        (the service variable ZECRAFT_HOLD=1 keeps it stopped)
#   ops.sh pack          /data/export/zecraft.tar (+ .sha256): secrets/,
#                        data/, config/, vault.env and MANIFEST.sha256
#                        (to move the stack back to a Mac)
#   ops.sh backup STAMP  consistent copies of every SQLite database in
#                        /data/backups/STAMP with SHA256SUMS
#   ops.sh clean NAME    removes /data/export (NAME=export) or /data/backups/NAME
#   ops.sh usage         disk use of /data
#
# Prints hashes, counts and sizes only, never file contents.
set -eu
umask 077
# /proc/*/comm keeps 15 characters: every zecraft-raffle-* binary reads "zecraft-raffle-".
running() { grep -qsxF "zecraft-raffle-" /proc/[0-9]*/comm; }
cmd=${1:?usage: ops.sh held|pack|backup|clean|usage ...}
case $cmd in
  held)
    if running; then exit 1; fi
    ;;
  pack)
    if running; then
      echo "the stack is running; hold it first (ZECRAFT_HOLD=1)" >&2
      exit 1
    fi
    work=$(mktemp -d)
    mkdir -p /data/export
    cd /data
    find secrets data config vault.env -type f | LC_ALL=C sort | xargs sha256sum >"$work/MANIFEST.sha256"
    tar -cf /data/export/zecraft.tar -C "$work" MANIFEST.sha256 -C /data secrets data config vault.env
    sha256sum /data/export/zecraft.tar | cut -d' ' -f1 >/data/export/zecraft.tar.sha256
    echo "$(wc -l <"$work/MANIFEST.sha256") files packed, sha256 $(cat /data/export/zecraft.tar.sha256)"
    rm -rf "$work"
    ;;
  backup)
    stamp=${2:?usage: ops.sh backup STAMP}
    case $stamp in *[!0-9TZ]* | "") exit 2 ;; esac
    d="/data/backups/$stamp"
    mkdir -p "$d"
    find /data/data -name '*.sqlite' | while read -r f; do
      out="$d/$(echo "${f#/data/data/}" | tr / _)"
      sqlite3 "$f" ".backup '$out'"
      [ "$(sqlite3 "$out" 'PRAGMA integrity_check;')" = ok ]
    done
    cd "$d"
    sha256sum -- *.sqlite >SHA256SUMS
    echo "$(wc -l <SHA256SUMS) databases copied and checked"
    ;;
  clean)
    name=${2:?usage: ops.sh clean export|STAMP}
    case $name in
      export) rm -rf /data/export ;;
      *[!0-9TZ]* | "") exit 2 ;;
      *) rm -rf "/data/backups/$name" ;;
    esac
    ;;
  usage)
    du -sh /data/secrets /data/data /data/config 2>/dev/null
    df -h /data | tail -1
    ;;
  *)
    echo "unknown step $cmd" >&2
    exit 2
    ;;
esac
