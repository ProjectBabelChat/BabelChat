//! `babel`: chat on a BabelChat server from a terminal.
//!
//! ```sh
//! babel register alice --locale ja-JP     # once; the sign-in is saved
//! babel                                   # chat
//! babel --profile bob login bob           # a second user on the same machine
//! ```

use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, bail};
use babel_bergamot::{BergamotConfig, BergamotTranslator};
use babel_cli::{
    profile::{self, Profile},
    repl::{Flow, Repl},
};
use babel_sdk::{Client, DeviceTranslator, MockDeviceTranslator, SessionConfig, Transport};
use clap::{Parser, Subcommand};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::broadcast::error::RecvError,
};

#[derive(Parser)]
#[command(about = "Chat on BabelChat from a terminal, translated", version)]
struct Args {
    /// Server to register or sign in on.
    #[arg(long, env = "BABEL_SERVER", default_value = "http://127.0.0.1:7000")]
    server: String,
    /// Which saved sign-in to use, so several users can share a machine.
    #[arg(long, env = "BABEL_PROFILE", default_value = "default")]
    profile: String,
    /// Where sign-ins are saved.
    #[arg(long, env = "BABEL_CONFIG_DIR")]
    config_dir: Option<PathBuf>,
    /// Start without the saved messages, and save none.
    #[arg(long)]
    no_cache: bool,
    /// How events arrive: WebSocket, falling back to long polling (auto), or
    /// only one of them.
    #[arg(long, env = "BABEL_TRANSPORT", default_value = "auto", value_parser = ["auto", "websocket", "poll"])]
    transport: String,
    /// Who translates on this device: the Firefox Translations (Bergamot)
    /// models, fetched on first use, or a mock that only tags the text.
    #[arg(long, env = "BABEL_DEVICE", default_value = "bergamot", value_parser = ["bergamot", "mock"])]
    device: String,
    /// Where on-device models are kept.
    #[arg(long, env = "BABEL_MODELS_DIR")]
    models_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Create an account and save the sign-in. The password is read from
    /// BABEL_PASSWORD or prompted for.
    Register {
        name: String,
        /// The language you read, e.g. ja-JP. Defaults to the system's.
        #[arg(long)]
        locale: Option<String>,
    },
    /// Sign in to an existing account and save the sign-in.
    Login { name: String },
    /// Forget the saved sign-in.
    Logout,
    /// Chat (the default).
    Chat,
}

fn password(confirm: bool) -> anyhow::Result<String> {
    if let Ok(password) = std::env::var("BABEL_PASSWORD") {
        return Ok(password);
    }
    let password = rpassword::prompt_password("password: ")?;
    if confirm && rpassword::prompt_password("again: ")? != password {
        bail!("the passwords differ");
    }
    Ok(password)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let dir = args.config_dir.clone().unwrap_or_else(profile::default_dir);

    let (client, name) = match args.command.unwrap_or(Command::Chat) {
        Command::Register { name, locale } => {
            let locale = locale.unwrap_or_else(profile::system_locale);
            let client = Client::register(&args.server, &name, &password(true)?, &locale)
                .await
                .context("could not register")?;
            (client, name)
        }
        Command::Login { name } => {
            let client = Client::login(&args.server, &name, &password(false)?)
                .await
                .context("could not sign in")?;
            (client, name)
        }
        Command::Logout => {
            if profile::remove(&dir, &args.profile)? {
                println!("Signed out ({}).", args.profile);
            }
            return Ok(());
        }
        Command::Chat => {
            let Some(saved) = profile::load(&dir, &args.profile)? else {
                bail!("not signed in: run `babel register <name>` or `babel login <name>` first");
            };
            let config = SessionConfig {
                cache: (!args.no_cache).then(|| profile::cache_path(&dir, &args.profile)),
                transport: match args.transport.as_str() {
                    "websocket" => Transport::WebSocket,
                    "poll" => Transport::LongPoll,
                    _ => Transport::Auto,
                },
                ..SessionConfig::default()
            };
            let translator: Arc<dyn DeviceTranslator> = match args.device.as_str() {
                "mock" => Arc::new(MockDeviceTranslator::default()),
                _ => Arc::new(BergamotTranslator::new(BergamotConfig {
                    models_dir: args.models_dir.clone().unwrap_or_else(profile::models_dir),
                    download: true,
                })),
            };
            return chat(
                Client::with_token(&saved.server, saved.token)?,
                translator,
                config,
            )
            .await;
        }
    };

    let path = profile::save(
        &dir,
        &args.profile,
        &Profile {
            server: client.server().to_string(),
            name: name.clone(),
            token: client.token().clone(),
        },
    )?;
    println!(
        "Signed in as {name} on {}; saved to {}. Run `babel` to chat.",
        client.server(),
        path.display()
    );
    Ok(())
}

async fn chat(
    client: Client,
    translator: Arc<dyn DeviceTranslator>,
    config: SessionConfig,
) -> anyhow::Result<()> {
    let session = client
        .connect_with(translator, config)
        .await
        .context("could not connect")?;
    let mut updates = session.updates();
    let mut repl = Repl::new(session, Box::new(std::io::stdout()));
    repl.start().await;

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {
            line = lines.next_line() => match line? {
                Some(line) => {
                    if repl.line(&line).await == Flow::Quit {
                        break;
                    }
                }
                None => break,
            },
            update = updates.recv() => match update {
                Ok(update) => {
                    if repl.update(update) == Flow::Quit {
                        break;
                    }
                }
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            },
            _ = tick.tick() => repl.tick(),
        }
    }
    Ok(())
}
