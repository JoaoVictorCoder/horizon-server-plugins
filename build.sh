#!/bin/sh
# Builds the Horizon server with plugins: downloads Horizon at the commit in engine/HORIZON_COMMIT,
# adds the plugin crates, applies engine/horizon.patch and compiles only the server (no sound, no
# controllers). The server lands in dist/horizon-server-engine, the example plugins in dist/plugins/.
#
# Needs: Rust (rustup.rs), curl, tar, git (for `git apply`), a C compiler.
set -eu
cd "$(dirname "$0")"
ROOT=$(pwd)
COMMIT=$(cat engine/HORIZON_COMMIT)
WORK="$ROOT/.build/horizon-$COMMIT"
EXT=so
case "$(uname -s)" in
  Darwin) EXT=dylib ;;
  MINGW*|MSYS*|CYGWIN*) EXT=dll ;;
esac

if [ ! -f "$WORK/.patched" ]; then
  echo "== Downloading Horizon $COMMIT"
  rm -rf "$WORK"
  mkdir -p "$WORK"
  curl -fsSL "https://github.com/horizon-rust/horizon/archive/$COMMIT.tar.gz" | tar -xz -C "$WORK" --strip-components=1
  echo "== Adding the plugin crates and the engine patch"
  cp -r "$ROOT/api" "$WORK/crates/horizon-plugin-api"
  cp -r "$ROOT/host" "$WORK/crates/horizon-plugin-host"
  sed -i.bak 's|path = "../api"|path = "../horizon-plugin-api"|' "$WORK/crates/horizon-plugin-host/Cargo.toml"
  rm -f "$WORK/crates/horizon-plugin-host/Cargo.toml.bak"
  # .build/ is inside this repository: without the ceiling, git apply would take this repository as its
  # root and silently skip every path of the patch.
  (cd "$WORK" && GIT_CEILING_DIRECTORIES="$ROOT/.build" git apply "$ROOT/engine/horizon.patch")
  touch "$WORK/.patched"
else
  # The plugin crates may have changed since: copy them again.
  rm -rf "$WORK/crates/horizon-plugin-api" "$WORK/crates/horizon-plugin-host"
  cp -r "$ROOT/api" "$WORK/crates/horizon-plugin-api"
  cp -r "$ROOT/host" "$WORK/crates/horizon-plugin-host"
  sed -i.bak 's|path = "../api"|path = "../horizon-plugin-api"|' "$WORK/crates/horizon-plugin-host/Cargo.toml"
  rm -f "$WORK/crates/horizon-plugin-host/Cargo.toml.bak"
fi

echo "== Compiling the server (the first time takes a while)"
(cd "$WORK" && cargo build --locked --release -p horizon-client --no-default-features)
(cd "$WORK" && cargo build --locked --release -p horizon-plugin-api --examples)

mkdir -p dist/plugins
BIN=horizon-client
[ "$EXT" = dll ] && BIN=horizon-client.exe
cp "$WORK/target/release/$BIN" "dist/horizon-server-engine$( [ "$EXT" = dll ] && echo .exe || true)"
for p in homes dragao; do
  for f in "$WORK/target/release/examples/lib$p.$EXT" "$WORK/target/release/examples/$p.$EXT"; do
    [ -f "$f" ] && cp "$f" dist/plugins/
  done
done
# Forja (forja/, docs/forja.md) builds in this repository's workspace: it only needs the API.
echo "== Compiling Forja"
cargo build --locked --release -p forja
for f in "target/release/libforja.$EXT" "target/release/forja.$EXT"; do
  [ -f "$f" ] && cp "$f" dist/plugins/
done
echo "== Done: dist/horizon-server-engine and dist/plugins/. Next: see server/README.md"
