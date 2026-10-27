#!/bin/sh
# Unpacks the operator's export into the volume, inside the container (run
# as root through `railway ssh` by scripts/railway/import.sh push):
#
#   /app/unpack.sh
#
# The export is /data/import/zecraft.tar, its sha256 in
# /data/import/zecraft.tar.sha256; inside it, MANIFEST.sha256 lists every
# file's sha256. Prints only hash checks and counts, never contents.
# Refuses to overwrite an imported volume. Writes /data/.imported last,
# which lets the waiting entrypoint start the stack.
set -eu
umask 077
in=/data/import
if [ -e /data/.imported ]; then
  echo "this volume was already imported; refusing to overwrite it" >&2
  exit 1
fi
for d in secrets data config; do
  if [ -e "/data/$d" ]; then
    echo "/data/$d exists; refusing to overwrite it" >&2
    exit 1
  fi
done
echo "$(cat "$in/zecraft.tar.sha256")  $in/zecraft.tar" | sha256sum -c --strict -
tar -x -f "$in/zecraft.tar" -C /data --no-same-owner --no-same-permissions
cd /data
sha256sum -c --strict --quiet MANIFEST.sha256
echo "$(wc -l <MANIFEST.sha256) files match the manifest"
rm -f "$in/zecraft.tar" "$in/zecraft.tar.sha256"
rmdir "$in" 2>/dev/null || true
touch /data/.imported
echo "imported; the stack starts now"
