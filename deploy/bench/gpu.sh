#!/usr/bin/env bash
# Runs on a rented GPU machine (Ubuntu with NVIDIA's driver, as GPU clouds
# provide it) to measure what one GPU translates: llama.cpp built for CUDA,
# serving one model at a time on 127.0.0.1:8001. The load test runs on your
# own machine through an SSH tunnel (docs/development.md).
#
#   scp deploy/bench/gpu.sh root@<gpu>:
#   ssh root@<gpu> ./gpu.sh setup                 # build llama.cpp, fetch the models
#   ssh root@<gpu> ./gpu.sh serve hy-mt2-7b 8     # one model, 8 requests at a time
#   ssh root@<gpu> ./gpu.sh stop
set -euo pipefail

dir=/root/bench
models=$dir/models
mkdir -p "$models"

# id -> Hugging Face file
declare -A files=(
  [hy-mt2-1.8b]=tencent/Hy-MT2-1.8B-GGUF/Hy-MT2-1.8B-Q4_K_M.gguf
  [hy-mt2-7b]=tencent/Hy-MT2-7B-GGUF/Hy-MT2-7B-Q4_K_M.gguf
  [hy-mt2-7b-q8]=tencent/Hy-MT2-7B-GGUF/HY-MT2-7B-Q8_0.gguf
  [qwen3.5-4b]=unsloth/Qwen3.5-4B-GGUF/Qwen3.5-4B-Q4_K_M.gguf
  [qwen3.5-9b]=unsloth/Qwen3.5-9B-GGUF/Qwen3.5-9B-Q4_K_M.gguf
)

setup() {
  nvidia-smi
  apt-get update -qq
  apt-get install -y -qq build-essential cmake git curl libcurl4-openssl-dev >/dev/null
  if ! command -v nvcc >/dev/null && [ ! -x /usr/local/cuda/bin/nvcc ]; then
    apt-get install -y -qq nvidia-cuda-toolkit >/dev/null
  fi
  export PATH=/usr/local/cuda/bin:$PATH
  if [ ! -x "$dir/llama.cpp/build/bin/llama-server" ]; then
    git clone --depth 1 https://github.com/ggml-org/llama.cpp "$dir/llama.cpp"
    cmake -S "$dir/llama.cpp" -B "$dir/llama.cpp/build" -DGGML_CUDA=ON -DCMAKE_CUDA_ARCHITECTURES=native -DCMAKE_BUILD_TYPE=Release
    cmake --build "$dir/llama.cpp/build" --target llama-server -j "$(nproc)"
  fi
  local id
  for id in "${!files[@]}"; do
    local path=${files[$id]}
    local repo=${path%/*} file=${path##*/}
    if [ ! -f "$models/$id.gguf" ]; then
      echo "fetching $id"
      curl -L --fail -o "$models/$id.gguf.part" "https://huggingface.co/$repo/resolve/main/$file"
      mv "$models/$id.gguf.part" "$models/$id.gguf"
    fi
  done
  ls -la "$models"
}

stop() {
  pkill -x llama-server || true
  while pgrep -x llama-server >/dev/null; do sleep 0.5; done
}

serve() {
  local id=$1 slots=${2:-1}
  [ -f "$models/$id.gguf" ] || { echo "no model $id; run setup" >&2; exit 1; }
  stop
  # Each slot gets the context one request needs at most (CTX_PER_SLOT,
  # 8k by default; 4k fits this chat and more slots in the VRAM).
  nohup "$dir/llama.cpp/build/bin/llama-server" -m "$models/$id.gguf" \
    --host 127.0.0.1 --port 8001 -ngl 99 -np "$slots" -c $((slots * ${CTX_PER_SLOT:-8192})) \
    --jinja >"$dir/server.log" 2>&1 &
  until curl -sf http://127.0.0.1:8001/health >/dev/null; do
    if ! pgrep -x llama-server >/dev/null; then
      tail -20 "$dir/server.log" >&2
      exit 1
    fi
    sleep 1
  done
  echo "serving $id with $slots slots"
  nvidia-smi --query-gpu=name,memory.used,memory.total --format=csv,noheader
}

case ${1:-} in
  setup) setup ;;
  serve) serve "${2:?model}" "${3:-1}" ;;
  stop) stop ;;
  *) sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 1 ;;
esac
