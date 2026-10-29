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
| `sign_in.rs` | Sign in or create an account (server, name, password, the language you read, a sign-up code on invitation-only servers). |
| `chat/mod.rs` | The chat screen's state: selection, the message list, composer, toasts; SDK updates and notifications. |
| `chat/sidebar.rs` | Guild rail with unread dots and mention badges, channel list, user panel, guild menu, member list. |
| `chat/messages.rs` | Channel header, messages (grouping, replies, mentions, translation line, reactions, threads started from them), composer and who is writing. |
| `chat/emoji.rs` | The emoji picker: common emoji, then all of them, searchable by name or shortcode. |
| `chat/dialogs.rs` | Add a guild (create or join), invite, create, edit or delete a channel, start a thread, the guild's settings, languages, notifications, settings, the password, confirmations. |
| `i18n.rs` | The UI's own words in English and Japanese, and language names. |
| `runtime.rs` | The Tokio runtime beside GPUI, the app's configuration, SDK errors in words. |
| `widgets.rs` | Avatars, badges, the dialog frame. |

What a user can do: sign up or in; create a guild (with its languages and a
`#general` channel) or join one with an invite code; create, rename and
delete channels (owners); rename and delete the guild (owners); start a
thread from a message, and rename or delete it (its creator, or owners);
write messages of several lines (Shift+Enter), reply, edit and delete
them; react with any emoji; see who is writing; `@name` mentions; see
each message in their language, with where the translation came from and
the original on request; unread markers and mention counts per channel and
guild; notification levels per guild and channel; invite people; leave;
kick and ban (owners); choose the guild's languages and, in paid guilds,
the model the server translates with (owners); change the language they
read and their display settings; change their password; sign their other
devices out; sign out.

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

## Reactions, threads and writing

Hovering a message shows its tools: react, reply, start a thread (in text
channels), edit and delete. Reactions sit under the message as chips with
their counts, the user's own highlighted; clicking one adds or takes back
the user's reaction, and the smiley after them opens the picker. The
picker opens where it was asked for, kept inside the window, with eight
common emoji above every Unicode emoji (a virtualized grid); typing
searches names and shortcodes, Enter takes the first match, Esc or a click
outside closes it. Messages a thread was started from link to it.

The composer is a several-line `TextField`: Enter sends, Shift+Enter
starts a new line, and it grows to eight lines before it scrolls. Typing in
it tells the channel the user is writing (not while editing a message);
the line under it says who else is ("alice is typing…", two names, or
"several people").

The channel header has a settings button for those who may manage the
channel (its owner; a thread's creator), with renaming, the topic and
deleting it. The guild's settings (its name and description, and deleting
it) are in the guild menu, for its owner. Deleting always asks first.

## Selecting, copying and dragging text

Message text is `gpui_md3e::SelectableText`: drag to select (across wrapped
lines), double-click for a word, triple-click for the whole message,
Ctrl+C to copy. A selection can be dragged into the composer or any other
text field, which inserts it where it is dropped; text dragged inside a
field moves. One selection exists at a time, as on a web page; pressing
anywhere else drops it. Copy works with the composer focused too: a field
with nothing selected copies the message selection instead.

Text travels between applications too. A text drag that leaves the window
becomes a native drag (on Wayland, `wl_data_device` offering
`text/plain;charset=utf-8`), and text dragged in from another program is
accepted by the text fields. Text always leaves as a copy. Dragging back
into the window it started from continues the original in-app drag, so a
move inside one field stays a move. Native text drags are implemented for
Wayland; on X11, Windows and macOS text drags stay inside the app.

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

## Files

The paperclip beside the composer opens the system's file dialog
(several at once), and files dropped anywhere on the channel are attached
too. Each is uploaded at once and waits above the composer as a chip
(name, then its size once uploaded; ✕ takes it back) until the next
message or reply in that channel takes it; Enter waits for uploads still
going. Files larger than 10 MB are refused before they are read.

Under a message, pictures (PNG, JPEG, GIF, WebP) show at their own size up
to 360 × 280, fitted to the column, laid out from the size the server
measured before they arrive; clicking one opens it large over the window
(Esc or a click closes it). Other files are cards with their name, size and
a save button, which asks where (the Downloads folder first). Files come
through the SDK's media cache (`<profile>.media` beside the sign-in), which
signing out deletes.

## Checking it without a person

`BABEL_SCRIPT` drives the window with `gpui_md3e::script` (the gallery's
driver, moved into the library): clicks, keys, text and `grim`
screenshots, dispatched into this window only.

```sh
BABEL_SCRIPT="wait 1500; click 34 50; wait 600; type 音ゲー部; key enter; wait 2500; shot /tmp/a.png; quit" \
  cargo run -p babel-desktop -- --profile alice
```

`drop X Y <path>` drops a file onto the window as if dragged from another
app (attaching it, over a channel).

Coordinates are the window's logical pixels. Screenshots are of the whole
screen in its own pixels, so convert with the window's origin and scale
(`MD3E_SCRIPT_DEBUG=1` prints the viewport and scale factor).

## Not yet

- A guild's own emoji (next), and pasting images from the clipboard.
- Who reacted with an emoji, and skin tones in the picker.
- Creating categories and moving channels between them from the UI
  (categories made elsewhere are listed).
- The ban list and unbanning, and listing or revoking invites, from the
  UI (the SDK and `babel` have them).
- Keyboard navigation between channels and guilds.
- Selecting across several messages at once.
- Native text drags on X11, Windows and macOS.
- A tray icon and an unread count on the app icon.
