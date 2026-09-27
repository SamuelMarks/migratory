#!/usr/bin/env bash
#
# Symlinks migratory executable as 'vagrant' in cargo bin or specified target directory.
#
set -euo pipefail

DEST_DIR="${1:-${CARGO_HOME:-$HOME/.cargo}/bin}"
mkdir -p "$DEST_DIR"

if [ -f "target/release/migratory" ]; then
    SRC="$(pwd)/target/release/migratory"
elif [ -f "target/debug/migratory" ]; then
    SRC="$(pwd)/target/debug/migratory"
else
    echo "Building migratory (release)..."
    cargo build --release
    SRC="$(pwd)/target/release/migratory"
fi

ln -sf "$SRC" "$DEST_DIR/vagrant"
echo "Successfully symlinked $SRC -> $DEST_DIR/vagrant"
"$DEST_DIR/vagrant" --version
