# The Grafana dashboard of a BabelChat cluster, as Nix data (module.nix
# provisions it as JSON). Labels come from Prometheus' targets: `service`,
# `kind`, `role` (primary or standby) and `machine`.
let
  datasource = {
    type = "prometheus";
    uid = "prometheus";
  };

  # Long polls answer when something happens or the wait ends: their
  # durations say nothing about speed.
  quick = ''route!~".*/(poll.*|watch_.*)|/replication/updates|/gateway/ws"'';

  panel =
    {
      title,
      queries,
      x,
      y,
      w ? 8,
      h ? 8,
      unit ? "short",
      type ? "timeseries",
      description ? null,
    }:
    {
      inherit title type datasource;
      gridPos = {
        inherit
          x
          y
          w
          h
          ;
      };
      targets = map (
        query:
        {
          inherit datasource;
          expr = query.expr;
          legendFormat = query.legend or "__auto";
          refId = query.ref or "A";
        }
        // (
          if type == "table" then
            {
              instant = true;
              format = "table";
            }
          else
            { }
        )
      ) queries;
      fieldConfig = {
        defaults = { inherit unit; };
        overrides = [ ];
      };
      options = { };
    }
    // (if description == null then { } else { inherit description; });

  row = title: y: {
    inherit title;
    type = "row";
    collapsed = false;
    gridPos = {
      x = 0;
      inherit y;
      w = 24;
      h = 1;
    };
    panels = [ ];
  };

  stat =
    title: expr: x: y:
    panel {
      inherit title x y;
      type = "stat";
      w = 4;
      h = 4;
      queries = [ { inherit expr; } ];
    };

  one = expr: legend: [ { inherit expr legend; } ];
in
{
  uid = "babelchat";
  title = "BabelChat";
  tags = [ "babelchat" ];
  schemaVersion = 39;
  timezone = "browser";
  refresh = "30s";
  time = {
    from = "now-6h";
    to = "now";
  };
  panels = [
    (row "Now" 0)
    (stat "Targets down" "count(up == 0) or vector(0)" 0 1)
    (stat "Sessions" "sum(babel_edge_sessions) or vector(0)" 4 1)
    (stat "Open sockets" "sum(babel_edge_websockets) or vector(0)" 8 1)
    (stat "Translation jobs" "sum(babel_translation_jobs) or vector(0)" 12 1)
    (panel {
      title = "Firing alerts";
      type = "table";
      x = 16;
      y = 1;
      w = 8;
      h = 4;
      queries = one ''ALERTS{alertstate="firing"}'' "";
    })

    (row "Requests" 5)
    (panel {
      title = "Calls per second, by service";
      x = 0;
      y = 6;
      unit = "reqps";
      queries = one "sum by (service) (rate(babel_requests_total[5m]))" "{{service}}";
    })
    (panel {
      title = "Failed calls per second (5xx), by route";
      x = 8;
      y = 6;
      unit = "reqps";
      queries = one ''sum by (service, route) (rate(babel_requests_total{status=~"5.."}[5m]))'' "{{service}} {{route}}";
    })
    (panel {
      title = "Slowest routes, p95";
      description = "Long polls left out.";
      x = 16;
      y = 6;
      unit = "s";
      queries = one "topk(8, histogram_quantile(0.95, sum by (le, service, route) (rate(babel_request_duration_seconds_bucket{${quick}}[5m]))))" "{{service}} {{route}}";
    })

    (row "Translation" 14)
    (panel {
      title = "Messages per second, by what happened";
      description = "queued: sent to a model; over_budget: the space's daily budget was spent; no_targets: nobody reads another language.";
      x = 0;
      y = 15;
      unit = "reqps";
      queries = one "sum by (status) (rate(babel_translation_messages_total[5m]))" "{{status}}";
    })
    (panel {
      title = "Time to translate a message";
      description = "From the Translation Cell queueing it to the answer, retries included.";
      x = 8;
      y = 15;
      unit = "s";
      queries = [
        {
          expr = "histogram_quantile(0.5, sum by (le) (rate(babel_translation_job_seconds_bucket[5m])))";
          legend = "p50";
        }
        {
          expr = "histogram_quantile(0.95, sum by (le) (rate(babel_translation_job_seconds_bucket[5m])))";
          legend = "p95";
          ref = "B";
        }
      ];
    })
    (panel {
      title = "Jobs waiting or translating";
      x = 16;
      y = 15;
      queries = one "sum by (service) (babel_translation_jobs)" "{{service}}";
    })
    (panel {
      title = "Model time, p95";
      x = 0;
      y = 23;
      unit = "s";
      queries = one "histogram_quantile(0.95, sum by (le, model) (rate(babel_engine_translation_seconds_bucket[5m])))" "{{model}}";
    })
    (panel {
      title = "Failed translations per second";
      x = 8;
      y = 23;
      unit = "reqps";
      queries = one ''sum by (model, outcome) (rate(babel_engine_translation_seconds_count{outcome!="ok"}[5m]))'' "{{model}} {{outcome}}";
    })
    (panel {
      title = "Tokens per second";
      x = 16;
      y = 23;
      queries = one "sum by (model, type) (rate(babel_engine_tokens_total[5m]))" "{{model}} {{type}}";
    })

    (row "Edges" 31)
    (panel {
      title = "Sessions and open sockets";
      x = 0;
      y = 32;
      queries = [
        {
          expr = "sum by (service) (babel_edge_sessions)";
          legend = "{{service}} sessions";
        }
        {
          expr = "sum by (service) (babel_edge_websockets)";
          legend = "{{service}} sockets";
          ref = "B";
        }
      ];
    })
    (panel {
      title = "Writes refused by the rate limit";
      x = 8;
      y = 32;
      unit = "reqps";
      queries = one "sum by (service) (rate(babel_edge_rate_limited_total[5m]))" "{{service}}";
    })
    (panel {
      title = "Sessions dropped for falling behind";
      x = 16;
      y = 32;
      queries = one "sum by (service) (increase(babel_edge_sessions_dropped_total[5m]))" "{{service}}";
    })

    (row "Stores" 40)
    (panel {
      title = "Standbys: writes behind the primary";
      x = 0;
      y = 41;
      queries = one "babel_standby_behind" "{{service}} on {{machine}}";
    })
    (panel {
      title = "Standbys: since the primary last answered";
      x = 8;
      y = 41;
      unit = "s";
      queries = one "time() - babel_standby_last_contact_timestamp_seconds" "{{service}} on {{machine}}";
    })
    (panel {
      title = "Since the last backup, and its copy";
      x = 16;
      y = 41;
      unit = "s";
      queries = [
        {
          expr = "time() - babel_backup_last_success_timestamp_seconds";
          legend = "{{service}}";
        }
        {
          expr = "time() - babel_backup_copy_last_success_timestamp_seconds";
          legend = "{{service}} copied";
          ref = "B";
        }
      ];
    })
    (panel {
      title = "Outbox: deliveries and retries per second";
      description = "Retries mean a Translation Cell or the Directory is not taking a guild's events.";
      x = 0;
      y = 49;
      unit = "reqps";
      queries = [
        {
          expr = "sum by (service) (rate(babel_outbox_delivered_total[5m]))";
          legend = "{{service}} delivered";
        }
        {
          expr = "sum by (service) (rate(babel_outbox_retries_total[5m]))";
          legend = "{{service}} retries";
          ref = "B";
        }
      ];
    })

    (row "Machines" 57)
    (panel {
      title = "CPU busy";
      x = 0;
      y = 58;
      unit = "percentunit";
      queries = one ''1 - avg by (machine) (rate(node_cpu_seconds_total{mode="idle"}[5m]))'' "{{machine}}";
    })
    (panel {
      title = "Memory used";
      x = 8;
      y = 58;
      unit = "percentunit";
      queries = one "1 - node_memory_MemAvailable_bytes / node_memory_MemTotal_bytes" "{{machine}}";
    })
    (panel {
      title = "Disk used";
      x = 16;
      y = 58;
      unit = "percentunit";
      queries = one ''1 - node_filesystem_avail_bytes{mountpoint="/"} / node_filesystem_size_bytes{mountpoint="/"}'' "{{machine}}";
    })
    (panel {
      title = "Network received";
      x = 0;
      y = 66;
      unit = "Bps";
      queries = one ''sum by (machine) (rate(node_network_receive_bytes_total{device!="lo"}[5m]))'' "{{machine}}";
    })
    (panel {
      title = "Network sent";
      x = 8;
      y = 66;
      unit = "Bps";
      queries = one ''sum by (machine) (rate(node_network_transmit_bytes_total{device!="lo"}[5m]))'' "{{machine}}";
    })
    (panel {
      title = "Until certificates expire";
      description = "`deploy.sh renew` issues those expiring within 90 days again.";
      x = 16;
      y = 66;
      unit = "s";
      queries = one "min by (service) (babel_certificate_expiry_timestamp_seconds) - time()" "{{service}}";
    })
  ];
}
