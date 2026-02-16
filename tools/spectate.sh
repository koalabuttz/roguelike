#!/usr/bin/env bash
# Watch an LLM play the roguelike in real time.
# The MCP server writes a frame to the spectate file after every action.
# This script refreshes the terminal display every 100ms.
#
# Usage: ./tools/spectate.sh           # default path
#        ./tools/spectate.sh 12345     # watch game with seed 12345
#        ROGUELIKE_SPECTATE_PATH=/tmp/my-game.txt ./tools/spectate.sh

if [ -n "$1" ]; then
    file="/tmp/roguelike-spectate-$1.txt"
else
    file="${ROGUELIKE_SPECTATE_PATH:-/tmp/roguelike-spectate.txt}"
fi

exec watch -t -n0.1 cat "$file"
