//! Who may call what, for every internal service, in one place.
//!
//! | Service | Callers |
//! |---|---|
//! | Account | Edges; operators (sign-up codes) |
//! | Directory | Edges (placement, memberships to list and watch); Message Cells (memberships to record); operators (moving guilds, reading placements) |
//! | Message Cell | Edges; the Directory (creating and moving guilds) |
//! | Translation Cell | Edges, Message Cells, tenants (other platforms, each in its own spaces); operators (setting plans) |
//! | Engine | Translation Cells |
//! | Media | Edges (files in and out); Message Cells (files messages took in or dropped); operators (listing, usage, deleting) |
//!
//! Every service also lets its own standbys copy it ([`Policy::with_replication`]).

use babel_common::Role;

use crate::Policy;

pub fn account() -> Policy {
    Policy::new(|caller, path| match path {
        "/account/create_signup_code" => caller.role == Role::Admin,
        _ => caller.role == Role::Edge,
    })
}

pub fn directory() -> Policy {
    Policy::new(|caller, path| match path {
        "/directory/add_membership" | "/directory/remove_membership" => {
            caller.role == Role::MessageCell
        }
        "/directory/move_guild" => caller.role == Role::Admin,
        "/directory/get_placement" | "/directory/watch_placements" => {
            matches!(caller.role, Role::Edge | Role::Admin)
        }
        _ => caller.role == Role::Edge,
    })
}

pub fn message_cell() -> Policy {
    Policy::new(|caller, path| match path {
        "/message_cell/create_guild"
        | "/message_cell/freeze_guild"
        | "/message_cell/export_guild"
        | "/message_cell/import_guild"
        | "/message_cell/finish_move"
        | "/message_cell/discard_guild" => caller.role == Role::Directory,
        _ => caller.role == Role::Edge,
    })
}

pub fn translation_cell() -> Policy {
    Policy::new(|caller, path| match path {
        // Plans are what spaces pay for: billing (an operator for now) sets
        // them, never a tenant for itself.
        "/translation_cell/set_plan" => caller.role == Role::Admin,
        _ => matches!(caller.role, Role::Edge | Role::MessageCell | Role::Tenant),
    })
}

pub fn engine() -> Policy {
    Policy::roles(&[Role::TranslationCell])
}

pub fn media() -> Policy {
    Policy::new(|caller, path| match path {
        "/media/attach_blobs" | "/media/delete_channel" | "/media/delete_guild" => {
            caller.role == Role::MessageCell
        }
        "/media/delete_blobs" => matches!(caller.role, Role::MessageCell | Role::Admin),
        "/media/list_blobs" | "/media/get_usage" => caller.role == Role::Admin,
        // get_blob, and the file routes (`/media/blobs`, `/media/blobs/…`).
        _ => caller.role == Role::Edge,
    })
}

#[cfg(test)]
mod tests {
    use babel_common::Identity;

    use super::*;

    fn caller(role: Role) -> Identity {
        Identity {
            role,
            name: "x".into(),
        }
    }

    #[test]
    fn only_the_directory_moves_guilds_between_cells() {
        let cell = message_cell();
        assert!(cell.allows(&caller(Role::Edge), "/message_cell/send_message"));
        assert!(!cell.allows(&caller(Role::Edge), "/message_cell/export_guild"));
        assert!(!cell.allows(&caller(Role::Edge), "/message_cell/create_guild"));
        assert!(cell.allows(&caller(Role::Directory), "/message_cell/import_guild"));
        assert!(!cell.allows(&caller(Role::Directory), "/message_cell/send_message"));

        let directory = directory();
        assert!(directory.allows(&caller(Role::Admin), "/directory/move_guild"));
        assert!(!directory.allows(&caller(Role::Edge), "/directory/move_guild"));
        assert!(!directory.allows(&caller(Role::Admin), "/directory/create_guild"));
        assert!(!directory.allows(&caller(Role::Edge), "/directory/add_membership"));
    }

    #[test]
    fn only_operators_make_sign_up_codes() {
        let account = account();
        assert!(account.allows(&caller(Role::Edge), "/account/register"));
        assert!(!account.allows(&caller(Role::Edge), "/account/create_signup_code"));
        assert!(account.allows(&caller(Role::Admin), "/account/create_signup_code"));
        assert!(!account.allows(&caller(Role::Admin), "/account/login"));
    }

    #[test]
    fn only_edges_pass_files() {
        let media = media();
        assert!(media.allows(&caller(Role::Edge), "/media/blobs"));
        assert!(media.allows(&caller(Role::Edge), "/media/blobs/g/c/media-0.x"));
        assert!(!media.allows(&caller(Role::MessageCell), "/media/blobs"));
        assert!(media.allows(&caller(Role::MessageCell), "/media/delete_guild"));
        assert!(!media.allows(&caller(Role::Edge), "/media/delete_guild"));
        assert!(media.allows(&caller(Role::Admin), "/media/list_blobs"));
        assert!(!media.allows(&caller(Role::Admin), "/media/blobs"));
    }

    #[test]
    fn only_operators_set_plans() {
        let cell = translation_cell();
        assert!(cell.allows(&caller(Role::Tenant), "/translation_cell/ingest"));
        assert!(!cell.allows(&caller(Role::Tenant), "/translation_cell/set_plan"));
        assert!(!cell.allows(&caller(Role::Edge), "/translation_cell/set_plan"));
        assert!(cell.allows(&caller(Role::Admin), "/translation_cell/set_plan"));
        assert!(!cell.allows(&caller(Role::Admin), "/translation_cell/ingest"));
    }
}
