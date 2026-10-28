//! Runs a short multilingual chat through the models of a models file, the
//! way a Translation Cell would (the window grows message by message), and
//! prints every translation with its time and token usage.
//!
//! ```sh
//! cargo run -p babel-engine --example sample_chat -- --models models.toml
//! cargo run -p babel-engine --example sample_chat -- --models models.toml --model qwen3.5-4b
//! ```

mod shared;

use std::{path::PathBuf, time::Instant};

use babel_api_engine::types::Engine as _;
use babel_engine::{Engine, config};
use clap::Parser;
use shared::{CHAT, request};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    models: PathBuf,
    /// Only this model (repeatable). All of them by default.
    #[arg(long)]
    model: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let configs = config::load(&args.models)?;
    for config in configs
        .iter()
        .filter(|config| args.model.is_empty() || args.model.contains(&config.id))
    {
        let engine = Engine::new(vec![config.build(None)?]);
        println!("\n===== {} =====", config.id);
        let (mut total_ms, mut total_input, mut total_cached, mut failures) = (0, 0, 0, 0);

        for (index, (author, lang, text)) in CHAT.iter().enumerate() {
            let started = Instant::now();
            let result = engine.translate(request(0, index)).await;
            let ms = started.elapsed().as_millis();
            total_ms += ms;
            println!("\n{author} ({lang}): {text}");
            match result {
                Ok(response) => {
                    for translation in &response.translations {
                        println!("  {} → {}", translation.lang, translation.text);
                    }
                    let usage = response.usage;
                    total_input += usage.input_tokens + usage.cache_read_tokens;
                    total_cached += usage.cache_read_tokens;
                    println!(
                        "  [{ms} ms; input {} (cached {}), output {}]",
                        usage.input_tokens + usage.cache_read_tokens,
                        usage.cache_read_tokens,
                        usage.output_tokens
                    );
                }
                Err(error) => {
                    failures += 1;
                    println!("  FAILED: {error:?} [{ms} ms]");
                }
            }
        }

        println!(
            "\n{}: {} messages in {:.1} s ({:.1} s each), {} failed; input tokens {} of which cached {}",
            config.id,
            CHAT.len(),
            total_ms as f64 / 1000.0,
            total_ms as f64 / 1000.0 / CHAT.len() as f64,
            failures,
            total_input,
            total_cached
        );
    }
    Ok(())
}
