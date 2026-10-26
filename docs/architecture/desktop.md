# Desktop app

Code: `client/desktop` (`babel-desktop`, binary `babelchat`)

The desktop app is a Discord-like window over the [client SDK](client.md),
drawn with GPUI (`gpui-reloaded`) and the Material 3 Expressive components
of `gpui-md3e`. It holds no chat state of its own: the SDK session is the
source of truth, and the app remembers only what is on screen.

```
 ┌──────┬────────────────┬──────────────────────────────┬────────────┐
 │guilds│ guild name   ▾ │ # channel      🔔  👤+  👥    │ members    │
 │ (A)  │ channels       │                              │  owner     │
 │ (B)• │  # general  ①  │  messages, in your language  │  member    │
 │  +   │  # random      │  "Translated from Japanese · │            │
 │      │                │   Show original"             │            │
 │      │ you · language │ [ message #channel      ] ➤  │            │
 └──────┴────────────────┴──────────────────────────────┴────────────┘
```

## Screens

| Module | What |
|---|---|
| `app.rs` | The root view: sign-in, connecting, then the chat. Saves and forgets sign-ins. |
| `sign_in.rs` | Sign in or create an account (server, name, password, the language you read). |
| `chat/mod.rs` | The chat screen's state: selection, the message list, composer, toasts; SDK updates and notifications. |
| `chat/sidebar.rs` | Guild rail with unread dots and mention badges, channel list, user panel, guild menu, member list. |
| `chat/messages.rs` | Channel header, messages (grouping, replies, mentions, translation line), composer. |
| `chat/dialogs.rs` | Add a guild (create or join), invite, create a channel, languages, notifications, settings, confirmations. |
| `i18n.rs` | The UI's own words in English and Japanese, and language names. |
| `runtime.rs` | The Tokio runtime beside GPUI, the app's configuration, SDK errors in words. |
| `widgets.rs` | Avatars, badges, the dialog frame. |

What a user can do: sign up or in; create a guild (with its languages and a
`#general` channel) or join one with an invite code; create channels
(owners); write, reply, edit and delete messages; `@name` mentions; see
each message in their language, with where the translation came from and
the original on request; unread markers and mention counts per channel and
guild; notification levels per guild and channel; invite people; leave;
kick and ban (owners); change the language they read and their display
settings; sign out.

## Two runtimes

The SDK runs on Tokio and GPUI on its own executor. `main` starts a small
multi-threaded Tokio runtime; `runtime::spawn` runs an SDK future on it and
returns a future any executor can await, so views write

```rust
let task = runtime::spawn(cx, async move { session.send(&channel, &text).await });
cx.spawn(async move |this, cx| {
    if let Err(error) = task.await { /* show it */ }
}).detach();
```

The session's update channel (a Tokio broadcast receiver) is read directly
from a GPUI task: Tokio's channels need no Tokio runtime to be awaited.
Synchronous `Session` reads (`guilds`, `messages`, `unread`) are called
from render.

## Messages on screen

The open channel is a GPUI `list` aligned to the bottom and following the
tail. `rows` mirrors the session's message IDs; on every update the common
prefix and suffix are kept and only the middle is spliced, so loading older
history (when the list is scrolled to the top) or a new message keeps the
scroll position. Translated or edited messages are remeasured, since their
height changes.

Consecutive messages of one author within five minutes share a header.
Messages that mention the reader, or reply to them, are highlighted. The
line under a translated message names the source language and whether the
server or this device translated it; "Show original" swaps the text in
place. With the "also show the original" setting, the original is printed
in small type under every translation.

## Reading and notifications

The open channel is the session's focus only while the window is active:
it is marked read as it grows and never notifies. In the background
nothing is focused, so new messages stay unread and notify. Notifications
from the SDK (already in the reader's language) become a snackbar with an
"Open" action while the window is active, and a desktop notification
otherwise (`--no-system-notifications` turns those off). Activating a
desktop notification opens its channel.

## The UI's language

The app's own words follow the language the user reads (`UserSettings::
locale`): Japanese for `ja`, English otherwise. Before signing in, the
system locale decides. Adding a language is adding a `Strings` value.

## Profiles

Sign-ins are the SDK's profiles (`babel_sdk::profile`), shared with the
terminal client: `babelchat --profile alice` and `babel --profile alice`
are the same user with the same cache file. Several profiles run side by
side, one window each.

## Checking it without a person

`BABEL_SCRIPT` drives the window with `gpui_md3e::script` (the gallery's
driver, moved into the library): clicks, keys, text and `grim`
screenshots, dispatched into this window only.

```sh
BABEL_SCRIPT="wait 1500; click 34 50; wait 600; type 音ゲー部; key enter; wait 2500; shot /tmp/a.png; quit" \
  cargo run -p babel-desktop -- --profile alice
```

Coordinates are the window's logical pixels. Screenshots are of the whole
screen in its own pixels, so convert with the window's origin and scale
(`MD3E_SCRIPT_DEBUG=1` prints the viewport and scale factor).

## Not yet

- Multi-line messages, attachments, reactions, typing indicators.
- Editing guild and channel names, categories and threads from the UI
  (threads and categories made elsewhere are listed).
- The ban list and unbanning, and listing or revoking invites, from the
  UI (the SDK and `babel` have them).
- Keyboard navigation between channels and guilds.
- A tray icon and an unread count on the app icon.
