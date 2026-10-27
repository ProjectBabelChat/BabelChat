#!/usr/bin/env bash
# Serves Hy-MT2 1.8B with llama.cpp on 127.0.0.1:8001, for models.toml.
# Downloads the model (1.1 GB) on first run.
#
#   deploy/local/llm.sh              # Vulkan: integrated GPUs too
#   LLM_DEVICE=cpu deploy/local/llm.sh
#
# On an Intel Iris Xe, Vulkan reads prompts about 3.5 times faster than the
# CPU and writes a little slower; prompts are most of the work here.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
dir=${BABEL_MODEL_DIR:-$here/models}
file=Hy-MT2-1.8B-Q4_K_M.gguf

mkdir -p "$dir"
if [ ! -f "$dir/$file" ]; then
  curl -L --fail -o "$dir/$file.part" \
    "https://huggingface.co/tencent/Hy-MT2-1.8B-GGUF/resolve/main/$file"
  mv "$dir/$file.part" "$dir/$file"
fi

case ${LLM_DEVICE:-vulkan} in
  vulkan) package=llama-cpp-vulkan layers=99 ;;
  cpu) package=llama-cpp layers=0 ;;
  *) echo "LLM_DEVICE is vulkan or cpu" >&2; exit 1 ;;
esac

# One slot: requests queue, and consecutive ones of a conversation reuse
# the prompt they share from the cache.
exec nix shell --inputs-from "$root" "nixpkgs#$package" -c \
  llama-server -m "$dir/$file" --host 127.0.0.1 --port 8001 \
  -ngl "$layers" -c 8192 -np 1 --jinja
