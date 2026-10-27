//! Two people chatting through the terminal client, as typed.

use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::Duration,
};

use babel_api_translation_cell::types::Plan;
use babel_cli::repl::{Flow, Repl};
use babel_sdk::{Client, MockDeviceTranslator, Update};
use babel_server::{EngineChoice, ServerConfig, start};
use tokio::sync::broadcast;

#[derive(Clone, Default)]
struct Screen(Arc<Mutex<Vec<u8>>>);

impl Write for Screen {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Screen {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

struct Terminal {
    repl: Repl,
    updates: broadcast::Receiver<Update>,
    screen: Screen,
    /// Screen length when the last command was typed.
    mark: usize,
}

impl Terminal {
    async fn open(url: &str, name: &str, locale: &str) -> Self {
        let client = Client::register(url, name, "correct horse battery", locale, None)
            .await
            .unwrap();
        let session = client
            .connect(Arc::new(MockDeviceTranslator::default()))
            .await
            .unwrap();
        let updates = session.updates();
        let screen = Screen::default();
        let mut repl = Repl::new(session, Box::new(screen.clone()));
        repl.start().await;
        Self {
            repl,
            updates,
            screen,
            mark: 0,
        }
    }

    async fn type_line(&mut self, line: &str) {
        self.mark = self.screen.text().len();
        assert_eq!(self.repl.line(line).await, Flow::Continue);
    }

    /// Output since the last typed line.
    fn recent(&self) -> String {
        self.screen.text()[self.mark..].to_string()
    }

    /// Runs the client until its screen shows `text`.
    async fn wait_for(&mut self, text: &str) -> String {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let screen = self.screen.text();
            if screen.contains(text) {
                return screen;
            }
            tokio::select! {
                update = self.updates.recv() => {
                    self.repl.update(update.unwrap());
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => self.repl.tick(),
                _ = tokio::time::sleep_until(deadline) => {
                    panic!("never showed {text:?}; screen:\n{screen}")
                }
            }
        }
    }
}

#[tokio::test]
async fn two_people_chat_in_their_own_languages() {
    let dir = tempfile::tempdir().unwrap();
    let config = ServerConfig {
        data_dir: dir.path().to_path_buf(),
        message_cells: 1,
        translation_cells: 1,
        engine: EngineChoice::Mock {
            latency: Duration::from_millis(20),
        },
        default_plan: Plan::Paid3,
        daily_budget: None,
    };
    let (address, _server) = start(&config, "127.0.0.1:0").await.unwrap();
    let url = format!("http://{address}");

    let mut alice = Terminal::open(&url, "alice", "ja-JP").await;
    let mut bob = Terminal::open(&url, "bob", "en-US").await;
    assert!(alice.screen.text().contains("You are in no guild yet"));

    alice.type_line("/create Rhythm Club").await;
    alice.type_line("/languages ja en llm ja en").await;
    alice
        .wait_for("Languages: ja-JP en-US (LLM: ja-JP en-US")
        .await;
    alice.type_line("/channel general").await;
    assert!(alice.recent().contains("-- #general --"));
    alice.type_line("/invite").await;
    let code = alice
        .recent()
        .lines()
        .find_map(|line| line.strip_prefix("Invite code: "))
        .unwrap()
        .to_string();

    bob.type_line(&format!("/join {code}")).await;
    assert!(bob.recent().contains("== Rhythm Club =="));
    assert!(
        bob.recent()
            .contains("You read en-US, translated by the server.")
    );
    alice.wait_for("→ bob joined Rhythm Club").await;

    alice.type_line("明日の夜、一緒に遊びませんか").await;
    assert!(
        alice
            .recent()
            .contains("#1 alice: 明日の夜、一緒に遊びませんか")
    );
    bob.wait_for("#1 alice: [en-US] 明日の夜、一緒に遊びませんか  ‹ja-JP→en-US›")
        .await;

    bob.type_line("Sure, see you there!").await;
    alice
        .wait_for("bob: [ja-JP] Sure, see you there!  ‹en-US→ja-JP›")
        .await;

    // Edits are retranslated and shown again.
    alice.type_line("/edit #1 やっぱり明後日にしませんか").await;
    bob.wait_for("↳ #general #1 alice: [en-US] やっぱり明後日にしませんか")
        .await;

    // Originals on request; another language through the device.
    bob.type_line("/original on").await;
    bob.type_line("/locale fr-FR").await;
    assert!(bob.recent().contains("Reading fr-FR"));
    alice.type_line("またね").await;
    let screen = bob
        .wait_for("alice: [device fr-FR] [en-US] またね  ‹ja-JP→en→fr-FR on device›")
        .await;
    assert!(screen.contains("┆ またね"));
}

#[tokio::test]
async fn other_channels_speak_up_through_notifications() {
    let dir = tempfile::tempdir().unwrap();
    let config = ServerConfig {
        data_dir: dir.path().to_path_buf(),
        message_cells: 1,
        translation_cells: 1,
        engine: EngineChoice::Mock {
            latency: Duration::from_millis(20),
        },
        default_plan: Plan::Paid3,
        daily_budget: None,
    };
    let (address, _server) = start(&config, "127.0.0.1:0").await.unwrap();
    let url = format!("http://{address}");

    let mut alice = Terminal::open(&url, "alice", "ja-JP").await;
    let mut bob = Terminal::open(&url, "bob", "en-US").await;
    alice.type_line("/create Rhythm Club").await;
    alice.type_line("/languages ja en llm ja en").await;
    alice.type_line("/channel general").await;
    alice.type_line("/invite").await;
    let code = alice
        .recent()
        .lines()
        .find_map(|line| line.strip_prefix("Invite code: "))
        .unwrap()
        .to_string();
    bob.type_line(&format!("/join {code}")).await;
    assert!(bob.recent().contains("-- #general --"));

    // Alice moves on to #random; Bob stays in #general and hears of it
    // through notifications (of every message, by default).
    alice.type_line("/channel random").await;
    alice.type_line("誰かいる？").await;
    alice.type_line("@bob 見て").await;
    bob.wait_for("🔔 new #random #1 alice: [en-US] 誰かいる？  ‹ja-JP→en-US›")
        .await;
    bob.wait_for("🔔 mention #random #2 alice: [en-US] @bob 見て  ‹ja-JP→en-US›")
        .await;
    bob.type_line("/unread").await;
    assert!(bob.recent().contains("Rhythm Club#random  •  @1"));

    bob.type_line("/reply #2 On my way").await;
    alice.wait_for("bob ↪#2: [ja-JP] On my way").await;

    // Opening the channel reads it.
    bob.type_line("/open random").await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        bob.type_line("/unread").await;
        if bob.recent().contains("All read.") {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "never read");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Mentions only: plain messages elsewhere stay quiet.
    bob.type_line("/open general").await;
    bob.type_line("/notify mentions").await;
    assert!(
        bob.recent()
            .contains("Notifications in Rhythm Club: mentions and replies.")
    );
    alice.type_line("静かに").await;
    alice.type_line("@bob 来て").await;
    let screen = bob.wait_for("🔔 mention #random").await;
    assert!(!screen.contains("静かに"));

    alice.type_line("/kick bob").await;
    bob.wait_for("! you were removed from Rhythm Club").await;
    alice.wait_for("← bob was removed from Rhythm Club").await;
}
