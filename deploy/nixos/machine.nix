# What every machine of a cluster deployed with deploy.sh is, besides its
# BabelChat services: a KVM virtual machine (Vultr, Hetzner, DigitalOcean and
# the like) with one disk, reached over SSH as root.
{
  config,
  lib,
  modulesPath,
  ...
}:

{
  imports = [ (modulesPath + "/profiles/qemu-guest.nix") ];

  options.babelchat.deploy = {
    sshKeys = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      description = "Public keys allowed to log in as root.";
    };
    disk = lib.mkOption {
      type = lib.types.str;
      default = "/dev/vda";
      description = "The disk NixOS is installed on (erased by `deploy.sh install`).";
    };
  };

  config = {
    # GPT with both a BIOS boot partition and an EFI system partition, so
    # the machine boots whichever firmware the provider uses.
    disko.devices.disk.main = {
      device = config.babelchat.deploy.disk;
      type = "disk";
      content = {
        type = "gpt";
        partitions = {
          boot = {
            size = "1M";
            type = "EF02";
          };
          ESP = {
            size = "512M";
            type = "EF00";
            content = {
              type = "filesystem";
              format = "vfat";
              mountpoint = "/boot";
              mountOptions = [ "umask=0077" ];
            };
          };
          root = {
            size = "100%";
            content = {
              type = "filesystem";
              format = "ext4";
              mountpoint = "/";
            };
          };
        };
      };
    };
    boot.loader.grub = {
      enable = true;
      efiSupport = true;
      efiInstallAsRemovable = true;
    };

    networking.hostName = config.services.babelchat.machine;
    networking.useDHCP = true;
    services.openssh = {
      enable = true;
      settings.PasswordAuthentication = false;
      settings.KbdInteractiveAuthentication = false;
    };
    users.users.root.openssh.authorizedKeys.keys = config.babelchat.deploy.sshKeys;

    # Small machines (1-2 GB) build nothing, but RocksDB and llama.cpp like
    # some room.
    zramSwap.enable = true;
    services.journald.settings.Journal.SystemMaxUse = "1G";
    nix.settings = {
      experimental-features = [
        "nix-command"
        "flakes"
      ];
      # deploy.sh copies closures signed by nobody.
      trusted-users = [ "root" ];
    };
    nix.gc = {
      automatic = true;
      options = "--delete-older-than 14d";
    };

    system.stateVersion = "26.11";
  };
}
