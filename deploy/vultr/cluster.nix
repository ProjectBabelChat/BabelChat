# A production-like BabelChat cluster on eight virtual machines: every
# service apart, with a standby for every store (docs/deployment.md).
#
# 1. Create the machines (Debian, x86-64, your SSH key added for root) and
#    put their public IPv4 addresses below, and your SSH public key.
# 2. deploy/nixos/deploy.sh init; then install and switch each machine.
#
# Placement is only the `machine` (and `standby`) of each service: put
# several on one machine by naming the same one, and drop the machines no
# longer used. Ports never clash. A service that keeps data and moves to
# another machine needs its data moved too (docs/deployment.md).
{
  sshKeys = [
    # "ssh-ed25519 AAAA... you@laptop"
  ];

  cluster = {
    machines = {
      # 1 vCPU, 2 GB unless noted.
      edge-a = {
        address = "203.0.113.11";
        wireguardAddress = "10.8.0.11";
      };
      edge-b = {
        address = "203.0.113.12";
        wireguardAddress = "10.8.0.12";
      };
      # Account service, Directory, egress proxy; the operator's tools.
      core = {
        address = "203.0.113.2";
        wireguardAddress = "10.8.0.2";
      };
      cell-a = {
        address = "203.0.113.3";
        wireguardAddress = "10.8.0.3";
      };
      cell-b = {
        address = "203.0.113.4";
        wireguardAddress = "10.8.0.4";
      };
      translation = {
        address = "203.0.113.5";
        wireguardAddress = "10.8.0.5";
      };
      # 4 vCPU, 8 GB: Hy-MT2 on the CPU.
      llm = {
        address = "203.0.113.6";
        wireguardAddress = "10.8.0.6";
      };
      # Standbys of the Account service, the Directory and the Translation
      # Cells.
      spare = {
        address = "203.0.113.7";
        wireguardAddress = "10.8.0.7";
      };
    };

    account = {
      machine = "core";
      standby = "spare";
      # Accounts by invitation: `deploy.sh admin signup-code --uses 5`.
      signups = "invite";
    };
    directory = {
      machine = "core";
      standby = "spare";
    };

    messageCells = {
      # Each other's standby.
      message-cell-0 = {
        machine = "cell-a";
        standby = "cell-b";
      };
      message-cell-1 = {
        machine = "cell-b";
        standby = "cell-a";
      };
    };

    translationCells = {
      translation-cell-0 = {
        machine = "translation";
        standby = "spare";
        # Every new guild may use the LLM while trying things out;
        # `free` is the real default (`deploy.sh admin set-plan` per guild).
        plan = "paid3";
      };
      translation-cell-1 = {
        machine = "translation";
        standby = "spare";
        plan = "paid3";
      };
    };

    engines.engine-0 = {
      machine = "llm";
      llama = {
        url = "https://huggingface.co/tencent/Hy-MT2-1.8B-GGUF/resolve/main/Hy-MT2-1.8B-Q4_K_M.gguf";
        sha256 = "dc5f44fcf1fa496ee7ad725982c0c8c553a4de00259b53af84c4b89fb0c06699";
        threads = 4;
      };
      models = [
        {
          id = "hy-mt2-1.8b";
          name = "Hy-MT2 1.8B";
          backend = "openai";
          base_url = "http://127.0.0.1:8001/v1";
          style = "plain";
          # Tencent's recommended sampling for the 1.8B and 7B models.
          extra = {
            temperature = 0.7;
            top_p = 0.6;
            top_k = 20;
            repeat_penalty = 1.05;
          };
        }
        {
          id = "mock";
          name = "Mock";
          backend = "mock";
        }
      ];
      # A hosted model needs its key: put ANTHROPIC_API_KEY=... in
      # secrets/engine-0.env, set environmentFile = true and add, e.g.,
      # { id = "claude-haiku-5-5"; name = "Claude Haiku 5.5";
      #   backend = "anthropic"; api_key_env = "ANTHROPIC_API_KEY"; }
    };

    edges = {
      # Clients connect to https://edge-0.203-0-113-11.sslip.io (no DNS
      # needed); set `host` to use a domain of your own.
      edge-0.machine = "edge-a";
      edge-1.machine = "edge-b";
    };

    # Engines reach hosted models only through it.
    egress = {
      machine = "core";
      allowHosts = [
        "api.anthropic.com"
        "api.openai.com"
      ];
    };

    operator.machine = "core";
  };
}
