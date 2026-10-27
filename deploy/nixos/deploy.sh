#!/usr/bin/env bash
# Deploys a BabelChat cluster described by <deployment>/cluster.nix to
# NixOS machines, from this machine. See docs/deployment.md.
#
#   deploy/nixos/deploy.sh init                 # keys and certificates, once
#   deploy/nixos/deploy.sh install <machine>    # replace its OS with NixOS (erases it)
#   deploy/nixos/deploy.sh switch [<machine>…]  # build and activate (all by default)
#   deploy/nixos/deploy.sh admin <args…>        # babel-admin on the operator's machine
#   deploy/nixos/deploy.sh ssh <machine> [<command>…]
#
# BABEL_DEPLOYMENT picks the deployment directory (deploy/vultr by default).
# Its secrets/ (ignored by git) holds the CA's key and every service's:
# keep a copy somewhere safe. BABEL_BUILD_CORES: jobs building the binaries
# (4; each takes a few GB of memory).
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
deployment=$(cd "${BABEL_DEPLOYMENT:-$root/deploy/vultr}" && pwd)
secrets=$deployment/secrets
eval_nix=$root/deploy/nixos/eval.nix

ssh_options=(-o StrictHostKeyChecking=accept-new -o UserKnownHostsFile="$secrets/known_hosts")

say() { printf '\033[1m%s\033[0m\n' "$*" >&2; }

# nix with the pinned nixpkgs' tools, without needing the flake committed.
tool() {
  local package=$1
  shift
  nix shell --inputs-from "$root" "nixpkgs#$package" -c "$@"
}

build() {
  # The server binaries take a few GB of memory per job to build.
  nix build --impure --no-link --print-out-paths --cores "${BABEL_BUILD_CORES:-4}" \
    --file "$eval_nix" --argstr deployment "$deployment" "$@"
}

evaluate() {
  nix eval --impure --raw --file "$eval_nix" --argstr deployment "$deployment" "$@"
}

# "<name> <address> <secret files…>" per machine.
inventory() {
  evaluate inventory
}

address_of() {
  local name=$1 line
  line=$(inventory | awk -v name="$name" '$1 == name')
  if [ -z "$line" ]; then
    echo "no machine $name in $deployment/cluster.nix" >&2
    exit 1
  fi
  awk '{ print $2 }' <<<"$line"
}

files_of() {
  inventory | awk -v name="$1" '$1 == name { for (i = 3; i <= NF; i++) print $i }'
}

# Copies the machine's secrets into $1 (laid out as on the machine).
stage_secrets() {
  local name=$1 dir=$2 file
  mkdir -p "$dir/pki"
  chmod 700 "$dir" "$dir/pki"
  while read -r file; do
    local from=$secrets/$file
    if [ "$file" = wireguard.key ]; then
      from=$secrets/wireguard/$name.key
    fi
    if [ ! -f "$from" ]; then
      echo "missing $from: run \`deploy.sh init\` (or make it, for an engine's .env)" >&2
      exit 1
    fi
    install -m 600 "$from" "$dir/$file"
  done < <(files_of "$name")
}

init() {
  mkdir -p "$secrets/wireguard" "$secrets/pki"
  chmod 700 "$secrets" "$secrets/wireguard" "$secrets/pki"
  local name
  for name in $(inventory | awk '{ print $1 }'); do
    if [ ! -f "$secrets/wireguard/$name.key" ]; then
      say "WireGuard key for $name"
      (umask 077 && tool wireguard-tools wg genkey >"$secrets/wireguard/$name.key")
      tool wireguard-tools wg pubkey <"$secrets/wireguard/$name.key" >"$secrets/wireguard/$name.pub"
    fi
  done

  say "Building the server binaries"
  local package
  package=$(build package)
  # The machines' cluster file, with the certificates here.
  sed 's|^pki = .*|pki = "pki"|' "$(build clusterFile)" >"$secrets/cluster.toml"
  "$package/bin/babel-pki" cluster --cluster "$secrets/cluster.toml"
  if [ ! -f "$secrets/pki/ops.crt" ]; then
    "$package/bin/babel-pki" issue --dir "$secrets/pki" --role admin --name ops
  fi
  say "Secrets are in $secrets: back them up."
}

install_machine() {
  local name=$1 address toplevel disko staging
  address=$(address_of "$name")
  say "Building $name"
  toplevel=$(build "toplevel.$name")
  disko=$(build "disko.$name")
  staging=$(mktemp -d)
  trap 'rm -rf "$staging"' RETURN
  stage_secrets "$name" "$staging/var/lib/babelchat-secrets"
  say "Installing NixOS on $name ($address): its disk is erased"
  tool nixos-anywhere nixos-anywhere \
    --store-paths "$disko" "$toplevel" \
    --extra-files "$staging" \
    --ssh-option StrictHostKeyChecking=accept-new \
    --ssh-option UserKnownHostsFile="$secrets/known_hosts" \
    "root@$address"
  # The new system has a new host key.
  ssh-keygen -R "$address" -f "$secrets/known_hosts" >/dev/null 2>&1 || true
}

switch_machine() {
  local name=$1 address toplevel staging digest
  address=$(address_of "$name")
  say "Building $name"
  toplevel=$(build "toplevel.$name")
  say "Copying to $name ($address)"
  NIX_SSHOPTS="${ssh_options[*]}" nix copy --substitute-on-destination \
    --to "ssh://root@$address" "$toplevel"

  staging=$(mktemp -d)
  trap 'rm -rf "$staging"' RETURN
  stage_secrets "$name" "$staging/secrets"
  digest=$(cd "$staging/secrets" && find . -type f | sort | xargs sha256sum | sha256sum | cut -d' ' -f1)
  say "Activating $name"
  tar -C "$staging" -cf - secrets | ssh "${ssh_options[@]}" "root@$address" "
    set -e
    rm -rf /var/lib/babelchat-secrets.new
    mkdir -p /var/lib/babelchat-secrets.new
    tar -xf - -C /var/lib/babelchat-secrets.new --strip-components=1
    chmod 700 /var/lib/babelchat-secrets.new
    old=\$(cat /var/lib/babelchat-secrets/.digest 2>/dev/null || true)
    echo $digest >/var/lib/babelchat-secrets.new/.digest
    rm -rf /var/lib/babelchat-secrets.old
    if [ -d /var/lib/babelchat-secrets ]; then mv /var/lib/babelchat-secrets /var/lib/babelchat-secrets.old; fi
    mv /var/lib/babelchat-secrets.new /var/lib/babelchat-secrets
    rm -rf /var/lib/babelchat-secrets.old
    nix-env -p /nix/var/nix/profiles/system --set $toplevel
    $toplevel/bin/switch-to-configuration switch
    # Services read their certificates when they start.
    if [ \"\$old\" != $digest ]; then
      systemctl restart 'wireguard-*.service' 'babelchat-*.service' || true
    fi
  "
}

command=${1:-}
shift || true
case $command in
  init) init ;;
  install)
    [ $# -eq 1 ] || { echo "usage: deploy.sh install <machine>" >&2; exit 1; }
    install_machine "$1"
    ;;
  switch)
    if [ $# -eq 0 ]; then
      set -- $(inventory | awk '{ print $1 }')
    fi
    for name in "$@"; do
      switch_machine "$name"
    done
    ;;
  admin)
    operator=$(evaluate operator)
    ssh "${ssh_options[@]}" "root@$(address_of "$operator")" babel-admin "$@"
    ;;
  ssh)
    [ $# -ge 1 ] || { echo "usage: deploy.sh ssh <machine> [<command>…]" >&2; exit 1; }
    name=$1
    shift
    ssh "${ssh_options[@]}" "root@$(address_of "$name")" "$@"
    ;;
  *)
    sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
