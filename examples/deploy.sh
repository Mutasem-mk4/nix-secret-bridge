#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf 'usage: %s <flake-ref> <root@target>\n' "$0" >&2
}

if [ "$#" -ne 2 ]; then
  usage
  exit 64
fi

flake_ref="$1"
target="$2"
remote_dir="/run/nix-secret-bridge"
remote_key="${remote_dir}/age-identity.txt"

: "${NIX_SECRET_BRIDGE_AGE_KEY:?set NIX_SECRET_BRIDGE_AGE_KEY to the age identity contents}"

cleanup_remote_key() {
  ssh "$target" "if [ -f '$remote_key' ]; then shred -u '$remote_key' 2>/dev/null || rm -f '$remote_key'; fi"
}

ssh "$target" "install -m 0700 -d '$remote_dir'"
printf '%s\n' "$NIX_SECRET_BRIDGE_AGE_KEY" \
  | ssh "$target" "umask 077; cat > '$remote_key'"

trap cleanup_remote_key EXIT

nixos-anywhere --flake "$flake_ref" "$target"
