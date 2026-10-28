#!/usr/bin/env bash
# Deploys a BabelChat cluster described by <deployment>/cluster.nix to
# NixOS machines, from this machine. See docs/deployment.md.
#
#   deploy/nixos/deploy.sh init                 # keys and certificates, once
#   deploy/nixos/deploy.sh install <machine>    # replace its OS with NixOS (erases it)
#   deploy/nixos/deploy.sh switch [<machine>…]  # build and activate (all by default)
#   deploy/nixos/deploy.sh admin <args…>        # babel-admin on the operator's machine
#   deploy/nixos/deploy.sh dashboard            # Grafana at http://localhost:3000
#   deploy/nixos/deploy.sh failover <service|machine>… [--primary-lost]
#                                               # standbys take over
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

# "<name> <address> <secret files…>" per machine. Evaluating it takes a
# while: `load_inventory` keeps it for the commands that follow (and the
# subshells they start).
inventory_cache=
load_inventory() {
  inventory_cache=$(evaluate inventory)
}
inventory() {
  if [ -n "$inventory_cache" ]; then
    printf '%s\n' "$inventory_cache"
  else
    evaluate inventory
  fi
}

# Builds every machine given, at once (one evaluation); prints
# "<name> <toplevel>" for each.
build_all() {
  build "${@/#/toplevel.}" >/dev/null
  evaluate toplevel --apply 'machines: builtins.concatStringsSep "" (
    builtins.attrValues (builtins.mapAttrs (name: machine: "${name} ${machine.outPath}\n") machines))'
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
  if [ ! -f "$secrets/grafana.key" ]; then
    (umask 077 && head -c 32 /dev/urandom | base64 >"$secrets/grafana.key")
  fi
  say "Secrets are in $secrets: back them up."
}

install_machine() {
  local name=$1 address toplevel disko staging file=$deployment/placement.json
  # A machine cut off by a failover comes back erased: nothing old on it
  # can act as a primary. The others talk to it again after `switch`.
  if [ -f "$file" ] && tool jq jq -e --arg name "$name" '(.cutOff // []) | index($name)' "$file" >/dev/null; then
    say "$name was cut off; it is back once installed and every machine switched"
    tool jq jq --arg name "$name" '.cutOff -= [$name]' "$file" >"$file.new"
    mv "$file.new" "$file"
  fi
  address=$(address_of "$name")
  say "Building $name"
  toplevel=$(build "toplevel.$name")
  disko=$(build "disko.$name")
  staging=$(mktemp -d)
  trap 'rm -rf "$staging"; trap - RETURN' RETURN
  stage_secrets "$name" "$staging/var/lib/babelchat-secrets"
  say "Installing NixOS on $name ($address): its disk is erased"
  # Whatever answers there is about to be erased; the old system's key (or
  # an interrupted install's) would only stop the installer.
  ssh-keygen -R "$address" -f "$secrets/known_hosts" >/dev/null 2>&1 || true
  tool nixos-anywhere nixos-anywhere \
    --store-paths "$disko" "$toplevel" \
    --extra-files "$staging" \
    --ssh-option StrictHostKeyChecking=accept-new \
    --ssh-option UserKnownHostsFile="$secrets/known_hosts" \
    "root@$address"
  # The new system has a new host key.
  ssh-keygen -R "$address" -f "$secrets/known_hosts" >/dev/null 2>&1 || true
}

# Activates <name>'s system: $2 if built already (build_all).
switch_machine() {
  local name=$1 toplevel=${2:-} address staging digest
  address=$(address_of "$name")
  if [ -z "$toplevel" ]; then
    say "Building $name"
    toplevel=$(build "toplevel.$name")
  fi
  say "Copying to $name ($address)"
  NIX_SSHOPTS="${ssh_options[*]}" nix copy --substitute-on-destination \
    --to "ssh://root@$address" "$toplevel"

  staging=$(mktemp -d)
  trap 'rm -rf "$staging"; trap - RETURN' RETURN
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

# Moves services that keep data to their standbys' machines, recording the
# moves in placement.json (over cluster.nix). Given a machine, moves every
# service it is the primary of.
#
# 1. every machine is built for the new placement, while nothing changed;
# 2. each old primary is stopped, or, if lost, its machine is cut off from
#    the others at once (and for good), so that should it still run, or
#    come back, it cannot act as a second primary;
# 3. the standbys stop copying, and the services start on their data;
# 4. every machine gets the new cluster file and restarts its services;
# 5. an old primary's data is put aside and its machine becomes the new
#    standby, starting over from a checkpoint (unless it was lost).
#
# Writes a primary made in its last moment, not yet copied, are lost.
failover() {
  local arg lost_all='' stores matched name primary standby machine
  local names=() 
  local -A primary_of standby_of lost toplevel
  stores=$(evaluate stores)
  for arg in "$@"; do
    case $arg in
      --primary-lost) lost_all=1 ;;
      -*)
        echo "usage: deploy.sh failover <service|machine>… [--primary-lost]" >&2
        exit 1
        ;;
      *)
        matched=$(awk -v arg="$arg" '$1 == arg || $2 == arg { print $1 }' <<<"$stores")
        if [ -z "$matched" ]; then
          echo "$arg is neither a service that keeps data nor the machine of one" >&2
          exit 1
        fi
        names+=($matched)
        ;;
    esac
  done
  [ ${#names[@]} -gt 0 ] || { echo "usage: deploy.sh failover <service|machine>… [--primary-lost]" >&2; exit 1; }
  mapfile -t names < <(printf '%s\n' "${names[@]}" | sort -u)

  load_inventory
  for name in "${names[@]}"; do
    read -r _ primary standby < <(awk -v name="$name" '$1 == name' <<<"$stores")
    if [ "$standby" = - ]; then
      echo "$name has no standby to take over" >&2
      exit 1
    fi
    primary_of[$name]=$primary
    standby_of[$name]=$standby
    # Machines cut off already are lost, whatever the command line says.
    if [ -n "$lost_all" ] || ! inventory | awk '{ print $1 }' | grep -qx "$primary"; then
      lost[$primary]=1
    fi
  done
  for name in "${names[@]}"; do
    if [ -n "${lost[${standby_of[$name]}]:-}" ]; then
      echo "$name's standby is on ${standby_of[$name]}, lost too" >&2
      exit 1
    fi
    primary=${primary_of[$name]}
    if [ -z "${lost[$primary]:-}" ] && ! ssh "${ssh_options[@]}" -o ConnectTimeout=10 "root@$(address_of "$primary")" true; then
      echo "$primary does not answer. If it is down for good, power it off and run:" >&2
      echo "  deploy.sh failover $* --primary-lost" >&2
      exit 1
    fi
  done

  # The new placement, built for every machine while the primaries still
  # serve.
  local file=$deployment/placement.json before after
  before=$(cat "$file" 2>/dev/null || echo '{}')
  after=$before
  for name in "${names[@]}"; do
    primary=${primary_of[$name]}
    after=$(tool jq jq --arg name "$name" --arg machine "${standby_of[$name]}" \
      --arg old "$primary" --argjson lost "$([ -n "${lost[$primary]:-}" ] && echo true || echo false)" '
        .services[$name] = { machine: $machine, standby: (if $lost then null else $old end) }
        | .cutOff = ((.cutOff // []) + (if $lost then [$old] else [] end) | unique)
      ' <<<"$after")
  done
  printf '%s\n' "$after" >"$file"
  local addresses
  addresses=$(inventory) # before cutting off: where every machine is
  load_inventory
  say "Building every machine"
  local built
  if ! built=$(build_all $(inventory | awk '{ print $1 }')); then
    printf '%s\n' "$before" >"$file"
    echo "building failed; nothing changed" >&2
    exit 1
  fi
  while read -r machine primary; do
    toplevel[$machine]=$primary
  done <<<"$built"

  # From here until the machines are switched, these services are
  # unavailable.
  local key pids=()
  for primary in "${!lost[@]}"; do
    say "Cutting $primary off: power it off for good (your provider's console), and reinstall it before using it again"
    # Now, before the standbys take over: should it still run, calls to it
    # fail instead of writing what the new primaries never see. The switch
    # below makes it last.
    key=$(cat "$secrets/wireguard/$primary.pub")
    for machine in $(inventory | awk '{ print $1 }'); do
      ssh "${ssh_options[@]}" "root@$(address_of "$machine")" \
        "for interface in \$(wg show interfaces); do wg set \$interface peer $key remove; done" &
      pids+=($!)
    done
  done
  for key in "${pids[@]}"; do
    wait "$key" || say "A machine could not cut a lost one off now; it will when switched"
  done
  for name in "${names[@]}"; do
    primary=${primary_of[$name]}
    if [ -z "${lost[$primary]:-}" ]; then
      say "Stopping $name on $primary"
      # Masked until its machine is switched, in case it restarts.
      ssh "${ssh_options[@]}" "root@$(awk -v m="$primary" '$1 == m { print $2 }' <<<"$addresses")" \
        "systemctl mask --runtime --now babelchat-$name"
    fi
    say "Promoting the standby of $name on ${standby_of[$name]}"
    ssh "${ssh_options[@]}" "root@$(address_of "${standby_of[$name]}")" \
      "systemctl stop babelchat-standby-$name"
  done

  # The new primaries' machines first, then every other at once.
  local first=() rest=()
  for machine in $(inventory | awk '{ print $1 }'); do
    if printf '%s\n' "${standby_of[@]}" | grep -qx "$machine"; then
      first+=("$machine")
    else
      rest+=("$machine")
    fi
  done
  if ! switch_each "${first[@]}" || ! switch_each "${rest[@]}"; then
    echo "some machines failed to switch (above); run \`deploy.sh switch\` for them" >&2
    exit 1
  fi
  for name in "${names[@]}"; do
    primary=${primary_of[$name]}
    say "$name now runs on ${standby_of[$name]}$([ -z "${lost[$primary]:-}" ] && echo ", with its standby on $primary")"
  done
}

# For failover: switches the machines given at once, with what it built.
# An old primary's data is put aside first: it starts over as a standby.
switch_each() {
  local machine pids=() failed=0
  for machine in "$@"; do
    (
      for name in "${names[@]}"; do
        if [ "${primary_of[$name]}" = "$machine" ]; then
          say "Putting $name's old data aside"
          ssh "${ssh_options[@]}" "root@$(address_of "$machine")" "
            set -e
            mkdir -p /var/lib/babelchat-retired
            mv /var/lib/private/babelchat/$name /var/lib/babelchat-retired/$name-\$(date +%Y%m%d-%H%M%S)
          "
        fi
      done
      switch_machine "$machine" "${toplevel[$machine]}"
      for name in "${names[@]}"; do
        if [ "${primary_of[$name]}" = "$machine" ]; then
          ssh "${ssh_options[@]}" "root@$(address_of "$machine")" "systemctl unmask --runtime babelchat-$name"
        fi
      done
    ) 2>&1 | sed -u "s/^/[$machine] /" &
    pids+=($!)
  done
  for machine in "${pids[@]}"; do
    wait "$machine" || failed=1
  done
  return $failed
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
    load_inventory
    if [ $# -eq 0 ]; then
      set -- $(inventory | awk '{ print $1 }')
    fi
    for name in "$@"; do
      switch_machine "$name"
    done
    ;;
  failover)
    failover "$@"
    ;;
  admin)
    operator=$(evaluate operator)
    ssh "${ssh_options[@]}" "root@$(address_of "$operator")" babel-admin "$@"
    ;;
  dashboard)
    monitoring=$(evaluate monitoring)
    [ -n "$monitoring" ] || { echo "no monitoring in $deployment/cluster.nix" >&2; exit 1; }
    say "Grafana: http://localhost:3000  Prometheus: http://localhost:9090  (Ctrl-C to stop)"
    ssh "${ssh_options[@]}" -N -L 3000:127.0.0.1:3000 -L 9090:127.0.0.1:9090 "root@$(address_of "$monitoring")"
    ;;
  ssh)
    [ $# -ge 1 ] || { echo "usage: deploy.sh ssh <machine> [<command>…]" >&2; exit 1; }
    name=$1
    shift
    ssh "${ssh_options[@]}" "root@$(address_of "$name")" "$@"
    ;;
  *)
    sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
