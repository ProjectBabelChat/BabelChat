//! Scripted input, for checking an app without a person at the keyboard:
//! `"move 400 300; down; wait 120; shot /tmp/a.png; up"`.
//!
//! Commands: `move X Y`, `down`, `up`, `click X Y`, `key <keystroke>` (one
//! key), `type <text>` (one character at a time), `keydown`/`keyup
//! <keystroke>`, `scroll <lines>`, `wait <ms>`, `shot <png>` and `quit`.
//!
//! Events are dispatched straight into the window, so nothing reaches other
//! applications. `shot` runs `grim` to capture the screen, limited to
//! `MD3E_SHOT_GEOMETRY` (`"x,y wxh"`) when set.

use std::time::Duration;

/// The pause between the events of a `click`.
const CLICK_STEP: Duration = Duration::from_millis(50);

use gpui::{
    AnyWindowHandle, App, AsyncApp, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PlatformInput, Point, ScrollDelta, ScrollEvent, point, px,
};

enum Command {
    Move(Point<gpui::Pixels>),
    Down,
    Up,
    Click(Point<gpui::Pixels>),
    Key(Keystroke),
    Type(String),
    KeyDown(Keystroke),
    KeyUp(Keystroke),
    Scroll(f32),
    Wait(Duration),
    Shot(String),
    Quit,
}

fn parse(script: &str) -> Vec<Command> {
    script
        .split([';', '\n'])
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let command = parts.next()?;
            let mut number = || parts.next().and_then(|n| n.parse::<f32>().ok());
            let keystroke = |line: &str| {
                let key = line.split_whitespace().nth(1)?;
                Keystroke::parse(Box::leak(key.to_owned().into_boxed_str())).ok()
            };
            Some(match command {
                "move" => Command::Move(point(px(number()?), px(number()?))),
                "down" => Command::Down,
                "up" => Command::Up,
                "click" => Command::Click(point(px(number()?), px(number()?))),
                "key" => Command::Key(keystroke(line)?),
                "type" => Command::Type(line.split_once(' ')?.1.to_owned()),
                "keydown" => Command::KeyDown(keystroke(line)?),
                "keyup" => Command::KeyUp(keystroke(line)?),
                "scroll" => Command::Scroll(number()?),
                "wait" => Command::Wait(Duration::from_millis(number()? as u64)),
                "shot" => Command::Shot(line.split_whitespace().nth(1)?.to_owned()),
                "quit" => Command::Quit,
                other => {
                    eprintln!("gpui-md3e: unknown script command {other:?}");
                    return None;
                }
            })
        })
        .collect()
}

/// Runs the script in the environment variable `var`, if set, against
/// `window`.
pub fn run_from_env(var: &str, window: AnyWindowHandle, cx: &mut App) {
    if let Ok(script) = std::env::var(var) {
        run(&script, window, cx);
    }
}

/// Runs `script` against `window`.
pub fn run(script: &str, window: AnyWindowHandle, cx: &mut App) {
    let commands = parse(script);
    cx.spawn(async move |cx: &mut AsyncApp| {
        // Let the first frames render before poking at the UI.
        cx.background_executor()
            .timer(Duration::from_millis(800))
            .await;
        let mut position = point(px(0.0), px(0.0));
        let mut button_down = false;
        for command in commands {
            let event = match command {
                Command::Move(to) => {
                    position = to;
                    Some(PlatformInput::MouseMove(MouseMoveEvent {
                        position,
                        pressed_button: button_down.then_some(MouseButton::Left),
                        modifiers: Modifiers::default(),
                    }))
                }
                Command::Down => {
                    button_down = true;
                    Some(PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                        first_mouse: false,
                    }))
                }
                Command::Up => {
                    button_down = false;
                    Some(PlatformInput::MouseUp(MouseUpEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                    }))
                }
                Command::Click(to) => {
                    position = to;
                    let modifiers = Modifiers::default();
                    for event in [
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position,
                            pressed_button: None,
                            modifiers,
                        }),
                        PlatformInput::MouseDown(MouseDownEvent {
                            button: MouseButton::Left,
                            position,
                            modifiers,
                            click_count: 1,
                            first_mouse: false,
                        }),
                    ] {
                        dispatch(cx, window, event);
                        // Let a frame render, as a person would, so hover and
                        // press state is in place.
                        cx.background_executor().timer(CLICK_STEP).await;
                    }
                    Some(PlatformInput::MouseUp(MouseUpEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers,
                        click_count: 1,
                    }))
                }
                Command::Key(keystroke) => {
                    press(cx, window, keystroke);
                    None
                }
                Command::Type(text) => {
                    for ch in text.chars() {
                        press(
                            cx,
                            window,
                            Keystroke {
                                modifiers: Modifiers::default(),
                                key: ch.to_string(),
                                key_char: Some(ch.to_string()),
                            },
                        );
                    }
                    None
                }
                Command::KeyDown(keystroke) => Some(key_down(keystroke)),
                Command::KeyUp(keystroke) => {
                    Some(PlatformInput::KeyUp(gpui::KeyUpEvent { keystroke }))
                }
                Command::Scroll(lines) => Some(PlatformInput::Scroll(ScrollEvent {
                    position,
                    delta: ScrollDelta::Lines(point(0.0, lines)),
                    ..Default::default()
                })),
                Command::Wait(duration) => {
                    cx.background_executor().timer(duration).await;
                    None
                }
                Command::Shot(path) => {
                    let mut grim = smol::process::Command::new("grim");
                    if let Ok(geometry) = std::env::var("MD3E_SHOT_GEOMETRY") {
                        grim.arg("-g").arg(geometry);
                    }
                    let status = grim.arg(&path).status().await;
                    if !matches!(status, Ok(status) if status.success()) {
                        eprintln!("gpui-md3e: grim failed for {path}");
                    }
                    None
                }
                Command::Quit => {
                    cx.update(|cx| cx.quit());
                    None
                }
            };
            if let Some(event) = event {
                dispatch(cx, window, event);
            }
        }
    })
    .detach();
}

/// Presses and releases a key, inserting its character into a focused text
/// input when no binding handles it.
fn press(cx: &mut AsyncApp, window: AnyWindowHandle, keystroke: Keystroke) {
    let _ = window.update(cx, |_, window, cx| {
        window.dispatch_keystroke(keystroke.clone(), cx);
        window.dispatch_event(PlatformInput::KeyUp(gpui::KeyUpEvent { keystroke }), cx);
        window.refresh();
    });
}

fn key_down(keystroke: Keystroke) -> PlatformInput {
    PlatformInput::KeyDown(gpui::KeyDownEvent {
        keystroke,
        is_held: false,
        prefer_character_input: false,
    })
}

fn dispatch(cx: &mut AsyncApp, window: AnyWindowHandle, event: PlatformInput) {
    let _ = window.update(cx, |_, window, cx| {
        let result = window.dispatch_event(event, cx);
        if std::env::var_os("MD3E_SCRIPT_DEBUG").is_some() {
            eprintln!(
                "dispatched: propagate={} default_prevented={} viewport={:?} scale={}",
                result.propagate,
                result.default_prevented,
                window.viewport_size(),
                window.scale_factor()
            );
        }
        window.refresh();
    });
}
