//! Services in other processes, behind the same traits as in-process ones.
//!
//! Each `Remote*` wraps the generated HTTP client of its API and converts
//! between the client's types and the server crate's types, whose JSON is
//! identical. A service that cannot be reached answers `unavailable`, like
//! one that is overloaded.

use babel_common::{Identity, convert};

/// Implements an API trait by forwarding every function to the generated
/// client of the same API.
macro_rules! remote {
    (
        $(#[$doc:meta])*
        $remote:ident: $api:ident::$service:ident via $client:ident, $error:ident;
        $( fn $name:ident($($arg:ident: $ty:ty),*) -> $ret:ty; )*
    ) => {
        $(#[$doc])*
        pub struct $remote {
            client: $client::types::$service,
            peer: Identity,
        }

        impl $remote {
            /// `client` should come from [`crate::tls::Credentials::http_client`]
            /// for `peer`.
            pub fn new(peer: Identity, url: impl Into<String>, client: reqwest::Client) -> Self {
                Self {
                    client: $client::types::$service {
                        url: url.into().trim_end_matches('/').to_string(),
                        client,
                    },
                    peer,
                }
            }

            fn failed(
                &self,
                error: $client::types::ThrowsOrOtherError<$client::types::$error>,
            ) -> $api::types::$error {
                match error {
                    $client::types::ThrowsOrOtherError::Throws(error) => convert(&error),
                    $client::types::ThrowsOrOtherError::Other(error) => {
                        tracing::warn!(peer = %self.peer, %error, "call failed");
                        $api::types::$error::Unavailable
                    }
                }
            }
        }

        #[async_trait::async_trait]
        impl $api::types::$service for $remote {
            $(
                async fn $name(&self, $($arg: $ty),*) -> Result<$ret, $api::types::$error> {
                    self.client
                        .$name($(convert(&$arg)),*)
                        .await
                        .map(|value| convert(&value))
                        .map_err(|error| self.failed(error))
                }
            )*
        }
    };
}

mod account {
    use babel_api_account::types::*;

    use super::*;

    remote! {
        /// The Account service.
        RemoteAccount: babel_api_account::Account via babel_api_account_client, AccountError;
        fn register(body: Registration) -> Token;
        fn login(body: Credentials) -> Token;
        fn get_user(user_id: String) -> User;
        fn get_settings(user_id: String) -> UserSettings;
        fn put_settings(body: UserSettings, user_id: String) -> UserSettings;
        fn change_password(body: PasswordChange, user_id: String) -> Token;
        fn sign_out_others(user_id: String) -> Token;
        fn create_signup_code(body: NewSignupCode) -> SignupCode;
        fn watch_revocations(after: i64, wait_ms: i64) -> RevocationBatch;
    }
}

mod directory {
    use babel_api_directory::types::*;

    use super::*;

    remote! {
        /// The Directory.
        RemoteDirectory: babel_api_directory::Directory via babel_api_directory_client, DirectoryError;
        fn create_guild(body: GuildRequest) -> Placement;
        fn get_placement(guild_id: String) -> Placement;
        fn watch_placements(after: i64, wait_ms: i64) -> PlacementBatch;
        fn add_membership(user_id: String, guild_id: String) -> ();
        fn remove_membership(user_id: String, guild_id: String) -> ();
        fn list_memberships(user_id: String) -> Vec<String>;
        fn watch_memberships(after: i64, wait_ms: i64) -> MembershipBatch;
        fn move_guild(guild_id: String, message_cell: String) -> Placement;
    }
}

mod message_cell {
    use babel_api_message_cell::types::*;

    use super::*;

    remote! {
        /// A Message Cell.
        RemoteMessageCell: babel_api_message_cell::MessageCell via babel_api_message_cell_client, MessageCellError;
        fn create_guild(body: NewGuild) -> Guild;
        fn update_guild(body: GuildUpdate) -> Guild;
        fn delete_guild(guild_id: String, user_id: String) -> ();
        fn get_guild(guild_id: String, user_id: String) -> GuildSnapshot;
        fn join_guild(guild_id: String, invite: String, user_id: String, user_name: String) -> GuildSnapshot;
        fn create_invite(body: NewInvite) -> Invite;
        fn revoke_invite(guild_id: String, secret: String, user_id: String) -> ();
        fn get_invites(guild_id: String, user_id: String) -> Vec<Invite>;
        fn leave_guild(guild_id: String, user_id: String) -> ();
        fn remove_member(guild_id: String, user_id: String, member_id: String, ban: bool) -> ();
        fn unban_member(guild_id: String, user_id: String, member_id: String) -> ();
        fn get_bans(guild_id: String, user_id: String) -> Vec<Ban>;
        fn ack(guild_id: String, channel_id: String, user_id: String, message_id: String) -> ReadState;
        fn put_notification_settings(body: NotificationSettings, user_id: String) -> NotificationSettings;
        fn check_permission(guild_id: String, user_id: String, permission: Permission) -> ();
        fn create_channel(body: NewChannel) -> Channel;
        fn update_channel(body: ChannelUpdate) -> Channel;
        fn delete_channel(guild_id: String, channel_id: String, user_id: String) -> ();
        fn add_upload(body: NewUpload) -> ();
        fn create_emoji(body: NewEmoji) -> Emoji;
        fn delete_emoji(guild_id: String, emoji_id: String, user_id: String) -> ();
        fn get_emoji(guild_id: String, user_id: String) -> Vec<Emoji>;
        fn send_message(body: NewMessage) -> Message;
        fn edit_message(body: MessageEdit) -> Message;
        fn delete_message(guild_id: String, channel_id: String, message_id: String, user_id: String) -> ();
        fn add_reaction(guild_id: String, channel_id: String, message_id: String, user_id: String, emoji: String) -> Message;
        fn remove_reaction(guild_id: String, channel_id: String, message_id: String, user_id: String, emoji: String) -> Message;
        fn typing(guild_id: String, channel_id: String, user_id: String) -> ();
        fn get_messages(guild_id: String, channel_id: String, user_id: String, before: Option<String>, limit: i64) -> Vec<Message>;
        fn poll_events(guild_id: String, after: i64, wait_ms: i64) -> GuildEventBatch;
        fn freeze_guild(guild_id: String) -> ();
        fn export_guild(guild_id: String, cursor: Option<String>, limit: i64) -> GuildExport;
        fn import_guild(body: GuildImport) -> ();
        fn finish_move(guild_id: String, moved: bool) -> ();
        fn discard_guild(guild_id: String) -> ();
    }
}

mod translation_cell {
    use babel_api_translation_cell::types::*;

    use super::*;

    remote! {
        /// A Translation Cell.
        RemoteTranslationCell: babel_api_translation_cell::TranslationCell via babel_api_translation_cell_client, TranslationCellError;
        fn put_space(body: SpaceContext) -> ();
        fn put_node(body: ContextNode) -> ();
        fn delete_node(space: String, node: String) -> ();
        fn delete_space(space: String) -> ();
        fn get_policy(space: String) -> SpacePolicy;
        fn put_policy(body: PolicyUpdate) -> SpacePolicy;
        fn set_plan(space: String, plan: Plan) -> SpacePolicy;
        fn list_models(space: String) -> Vec<TranslationModel>;
        fn normalize(space: String, tag: String) -> String;
        fn resolve_viewer(space: String, locale: String, translate_outside_set: bool) -> ViewerLanguage;
        fn ingest(body: IngestMessage) -> ();
        fn delete_message(body: MessageRef) -> ();
        fn get_translations(body: TranslationQuery) -> Vec<MessageTranslation>;
        fn poll_events(space: String, after: i64, wait_ms: i64) -> TranslationEventBatch;
        fn get_usage(space: String) -> SpaceUsage;
    }
}

pub use account::RemoteAccount;
pub use directory::RemoteDirectory;
pub use message_cell::RemoteMessageCell;
pub use translation_cell::RemoteTranslationCell;
