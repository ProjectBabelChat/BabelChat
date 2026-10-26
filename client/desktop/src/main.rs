//! `babelchat`: BabelChat on the desktop.
//!
//! ```sh
//! babelchat                       # the "default" profile
//! babelchat --profile bob         # a second user on the same machine
//! ```
//!
//! Sign-ins are shared with the terminal client (`babel`): a profile signed
//! in from either opens in both.

mod app;
mod chat;
mod i18n;
mod runtime;
mod sign_in;
mod widgets;

use std::{path::PathBuf, sync::Arc};

use babel_bergamot::{BergamotConfig, BergamotTranslator};
use babel_sdk::{DeviceTranslator, MockDeviceTranslator, Transport, profile};
use clap::Parser;
use gpui::prelude::*;
use gpui::{
    App, Bounds, KeyBinding, WindowAppearance, WindowBounds, WindowOptions, actions, px, size,
};
use gpui_md3e::{Md3eAssets, theme::SchemeConfig};
use gpui_platform::application;

use crate::{
    app::{BabelApp, FocusNext, FocusPrev},
    runtime::{Config, Tokio},
};

#[derive(Parser)]
#[command(
    about = "BabelChat on the desktop: chat in your language, read in yours",
    version
)]
struct Args {
    /// Server for new sign-ins.
    #[arg(long, env = "BABEL_SERVER", default_value = "http://127.0.0.1:7000")]
    server: String,
    /// Which saved sign-in to use, so several users can share a machine.
    #[arg(long, env = "BABEL_PROFILE", default_value = "default")]
    profile: String,
    /// Where sign-ins and caches are saved.
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
    /// Light or dark, or follow the system.
    #[arg(long, env = "BABEL_THEME", default_value = "system", value_parser = ["system", "light", "dark"])]
    theme: String,
    /// Window size in logical pixels, e.g. 1280x800.
    #[arg(long, default_value = "1280x800")]
    size: String,
    /// Do not show desktop notifications while the window is in the
    /// background.
    #[arg(long)]
    no_system_notifications: bool,
}

actions!(babelchat, [Quit]);

/// BabelChat's seed color for the Material color scheme.
const SEED: u32 = 0x4F5BD5;

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("babel-sdk")
        .enable_all()
        .build()?;
    let translator: Arc<dyn DeviceTranslator> = match args.device.as_str() {
        "mock" => Arc::new(MockDeviceTranslator::default()),
        _ => Arc::new(BergamotTranslator::new(BergamotConfig {
            models_dir: args.models_dir.clone().unwrap_or_else(profile::models_dir),
            download: true,
        })),
    };
    let config = Config {
        server: args.server.clone(),
        profile: args.profile.clone(),
        dir: args.config_dir.clone().unwrap_or_else(profile::default_dir),
        cache: !args.no_cache,
        transport: match args.transport.as_str() {
            "websocket" => Transport::WebSocket,
            "poll" => Transport::LongPoll,
            _ => Transport::Auto,
        },
        translator,
        system_notifications: !args.no_system_notifications,
    };
    let (width, height) = args
        .size
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?)))
        .unwrap_or((1280.0, 800.0));
    let handle = runtime.handle().clone();
    let theme = args.theme.clone();
    let profile_name = args.profile;

    application()
        .with_assets(Md3eAssets)
        .run(move |cx: &mut App| {
            let dark = match theme.as_str() {
                "dark" => true,
                "light" => false,
                _ => matches!(
                    cx.window_appearance(),
                    WindowAppearance::Dark | WindowAppearance::VibrantDark
                ),
            };
            gpui_md3e::init(
                SchemeConfig {
                    seed: SEED,
                    dark,
                    ..SchemeConfig::default()
                },
                cx,
            );
            cx.set_global(Tokio(handle));
            cx.set_global(config);
            cx.bind_keys([
                KeyBinding::new("tab", FocusNext, None),
                KeyBinding::new("shift-tab", FocusPrev, None),
                KeyBinding::new("escape", chat::Cancel, None),
                KeyBinding::new("ctrl-q", Quit, None),
            ]);
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
            let title = if profile_name == "default" {
                "BabelChat".to_string()
            } else {
                format!("BabelChat — {profile_name}")
            };
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        titlebar: Some(gpui::TitlebarOptions {
                            title: Some(title.into()),
                            ..Default::default()
                        }),
                        app_id: Some("babelchat".into()),
                        ..Default::default()
                    },
                    |window, cx| cx.new(|cx| BabelApp::new(window, cx)),
                )
                .expect("could not open a window");
            gpui_md3e::script::run_from_env("BABEL_SCRIPT", window.into(), cx);
            cx.activate(true);
        });
    drop(runtime);
    Ok(())
}
