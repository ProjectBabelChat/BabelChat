//! A guild's own emoji, used in other guilds too, through the SDK against a
//! whole in-process server.

mod common;

use babel_api_translation_cell::types::Plan;
use babel_sdk::{ApiError, Error, GuildChange, MAX_EMOJI_BYTES, Shown, Update, emoji_code};
use common::{guild, server, user};

/// A PNG of 32 x 32 pixels, padded to `size` bytes.
fn png(size: usize) -> Vec<u8> {
    let mut png = vec![
        0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0,
        0, 0, 32, 0, 0, 0, 32, 8, 6, 0, 0, 0,
    ];
    png.extend((0..size - png.len()).map(|i| (i % 253) as u8));
    png
}

#[tokio::test]
async fn emoji_go_wherever_their_guilds_members_write() {
    let server = server(Plan::Paid3).await;
    let mut alice = user(&server, "alice", "ja-JP").await;
    let mut bob = user(&server, "bob", "en-US").await;
    let mut carol = user(&server, "carol", "en-US").await;
    // Alice's guild has the emoji; Carol is not in it, only in another.
    let home = guild(&mut alice, &mut [&mut bob], &["ja", "en"], &["ja", "en"]).await;
    let home_guild = alice.session.channel(&home).unwrap().guild_id;
    let club = guild(&mut alice, &mut [&mut carol], &["ja", "en"], &["ja", "en"]).await;

    // Owners add them; names fit to type, images small.
    let party = alice
        .session
        .create_emoji(&home_guild, "party", png(2_000))
        .await
        .unwrap();
    assert_eq!(party.guild_id, home_guild);
    assert!(matches!(
        bob.session
            .create_emoji(&home_guild, "cake", png(2_000))
            .await,
        Err(Error::Api(ApiError::NotFound))
    ));
    assert!(matches!(
        alice
            .session
            .create_emoji(&home_guild, "Party", png(2_000))
            .await,
        Err(Error::Api(ApiError::NameTaken))
    ));
    assert!(matches!(
        alice
            .session
            .create_emoji(&home_guild, "big", png(MAX_EMOJI_BYTES as usize + 1))
            .await,
        Err(Error::Api(ApiError::LimitExceeded))
    ));
    assert!(matches!(
        alice
            .session
            .create_emoji(&home_guild, "notes", b"not a picture".to_vec())
            .await,
        Err(Error::Api(ApiError::InvalidRequest))
    ));
    // Members hear of it.
    bob.until(|session| {
        session
            .guild(&home_guild)
            .filter(|view| view.emoji.len() == 1)
    })
    .await;
    assert!(bob.session.find_emoji(&home_guild, "party").is_some());
    assert!(carol.session.find_emoji(&home_guild, "party").is_none());

    // Alice writes it in the other guild; Carol sees it, translated, and
    // can show its image without being in its guild.
    let code = emoji_code(&party);
    let sent = alice
        .session
        .send(&club, "今夜は :party: です")
        .await
        .unwrap();
    assert_eq!(sent.message.text, format!("今夜は {code} です"));
    let seen = carol.settled(&club, &sent.message.id).await;
    assert!(matches!(seen.display.shown, Shown::Server { .. }));
    assert!(seen.display.text.contains(&code), "{}", seen.display.text);
    let image = carol
        .session
        .emoji_image(&party.guild_id, &party.id)
        .await
        .unwrap();
    assert_eq!(image, png(2_000));

    // Emoji alone are not translated.
    let only = alice.session.send(&club, &code).await.unwrap();
    let seen = carol.settled(&club, &only.message.id).await;
    assert_eq!(seen.display.shown, Shown::Original);

    // Carol cannot use it: typed or pasted, it stays a name.
    let typed = carol.session.send(&club, "nice :party:").await.unwrap();
    assert_eq!(typed.message.text, "nice :party:");
    let pasted = carol
        .session
        .send(&club, &format!("nice {code}"))
        .await
        .unwrap();
    assert_eq!(pasted.message.text, "nice :party:");
    assert!(matches!(
        carol.session.react(&club, &sent.message.id, &code).await,
        Err(Error::Api(ApiError::InvalidRequest))
    ));
    // Alice can react with it, and Carol sees that.
    alice
        .session
        .react(&club, &sent.message.id, &code)
        .await
        .unwrap();
    carol
        .until(|session| {
            session.message(&club, &sent.message.id).filter(|message| {
                message
                    .message
                    .reactions
                    .iter()
                    .any(|reaction| reaction.emoji == code)
            })
        })
        .await;

    // Deleted, its image goes; codes stay, to be shown as names.
    alice
        .session
        .delete_emoji(&home_guild, &party.id)
        .await
        .unwrap();
    bob.next(|update| match update {
        Update::Guild {
            change: GuildChange::Emoji,
            ..
        } => Some(()),
        _ => None,
    })
    .await;
    assert!(bob.session.guild(&home_guild).unwrap().emoji.is_empty());
    let fresh = user(&server, "dave", "en-US").await;
    assert!(matches!(
        fresh.session.emoji_image(&party.guild_id, &party.id).await,
        Err(Error::Api(ApiError::NotFound))
    ));
    let after = alice
        .session
        .send(&club, &format!("still {code}"))
        .await
        .unwrap();
    assert_eq!(after.message.text, "still :party:");
}
