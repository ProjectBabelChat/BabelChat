//! How much translation one model server takes: N guilds chatting at once,
//! each sending the sample chat message by message (its window growing, its
//! own context), at several values of N. Prints, per N, how long a message
//! waits for its translations and how many the server finishes per second.
//!
//! ```sh
//! cargo run --release -p babel-engine --example load_test -- \
//!   --models models.toml --model hy-mt2-7b --guilds 1,2,4,8,16
//! ```
//!
//! The model server must take requests in parallel to gain from N > 1
//! (llama.cpp: `-np <slots>`, with `-c` slots times the context a request
//! needs).

mod shared;

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use babel_api_engine::types::{Engine as _, Usage};
use babel_engine::{Engine, config};
use clap::Parser;
use shared::{CHAT, request};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    models: PathBuf,
    /// The model to load (by default the models file's first).
    #[arg(long)]
    model: Option<String>,
    /// How many guilds chat at once, per run.
    #[arg(long, value_delimiter = ',', default_value = "1,2,4,8")]
    guilds: Vec<usize>,
    /// Times each guild goes through the chat per run.
    #[arg(long, default_value_t = 1)]
    rounds: usize,
}

struct Sample {
    latency: Duration,
    targets: usize,
    usage: Option<Usage>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let configs = config::load(&args.models)?;
    let config = match &args.model {
        Some(id) => configs
            .iter()
            .find(|config| &config.id == id)
            .ok_or_else(|| anyhow::anyhow!("{} has no model {id}", args.models.display()))?,
        None => configs
            .first()
            .ok_or_else(|| anyhow::anyhow!("{} has no models", args.models.display()))?,
    };
    let engine = Arc::new(Engine::new(vec![config.build(None)?]));

    println!(
        "{}: guilds chatting at once, each through a chat of {} messages",
        config.id,
        CHAT.len()
    );
    println!();
    println!(
        "| guilds | messages | failed | wall s | messages/s | translations/s | p50 s | p95 s | max s | output tok/s | input tok/msg (cached) |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    // Guild numbers keep growing, so no run reuses another's cached context.
    let mut next_guild = 0;
    for &guilds in &args.guilds {
        let started = Instant::now();
        let tasks = (0..guilds)
            .map(|_| {
                next_guild += 1;
                let (engine, guild, rounds) = (engine.clone(), next_guild, args.rounds);
                tokio::spawn(async move {
                    let mut samples = Vec::new();
                    for _ in 0..rounds {
                        for index in 0..CHAT.len() {
                            let request = request(guild, index);
                            let targets = request.targets.len();
                            let sent = Instant::now();
                            let usage = engine.translate(request).await.ok().map(|r| r.usage);
                            samples.push(Sample {
                                latency: sent.elapsed(),
                                targets,
                                usage,
                            });
                        }
                    }
                    samples
                })
            })
            .collect::<Vec<_>>();
        let mut samples = Vec::new();
        for task in tasks {
            samples.extend(task.await?);
        }
        report(guilds, started.elapsed(), &samples);
    }
    Ok(())
}

fn report(guilds: usize, wall: Duration, samples: &[Sample]) {
    let done = samples
        .iter()
        .filter(|s| s.usage.is_some())
        .collect::<Vec<_>>();
    let failed = samples.len() - done.len();
    let mut latencies = done
        .iter()
        .map(|s| s.latency.as_secs_f64())
        .collect::<Vec<_>>();
    latencies.sort_by(f64::total_cmp);
    let percentile = |p: f64| {
        if latencies.is_empty() {
            return f64::NAN;
        }
        latencies[((latencies.len() - 1) as f64 * p).round() as usize]
    };
    let wall_s = wall.as_secs_f64();
    let translations: usize = done.iter().map(|s| s.targets).sum();
    let usage = done.iter().filter_map(|s| s.usage.as_ref());
    let (input, cached, output) = usage.fold((0, 0, 0), |(i, c, o), u| {
        (
            i + u.input_tokens + u.cache_read_tokens,
            c + u.cache_read_tokens,
            o + u.output_tokens,
        )
    });
    let per_message = |tokens: i64| tokens as f64 / done.len().max(1) as f64;
    println!(
        "| {guilds} | {} | {failed} | {wall_s:.1} | {:.2} | {:.2} | {:.1} | {:.1} | {:.1} | {:.0} | {:.0} ({:.0}) |",
        samples.len(),
        done.len() as f64 / wall_s,
        translations as f64 / wall_s,
        percentile(0.5),
        percentile(0.95),
        percentile(1.0),
        output as f64 / wall_s,
        per_message(input),
        per_message(cached),
    );
}
