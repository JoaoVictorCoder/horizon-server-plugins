#!/bin/sh
# Starts the server in this folder; type its commands here, `stop` to stop it.
# RESOURCES: the game files Horizon prepares from your own Minecraft 26.3 (see README.md).
cd "$(dirname "$0")"
ENGINE=${ENGINE:-../dist/horizon-server-engine}
RESOURCES=${RESOURCES:-$HOME/.local/share/horizon-launcher/server-runtime/game-files/horizon-client/26.3}
if [ ! -d "$RESOURCES" ]; then
  echo "No game files at $RESOURCES: set RESOURCES (see README.md)." >&2
  exit 1
fi
mkdir -p plugins
exec "$ENGINE" --resources "$RESOURCES" --dedicated --nogui "$@"
