#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GuildRequest {
    pub owner_id: String,
    pub owner_name: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Placement {
    pub guild_id: String,
    pub message_cell: String,
    pub translation_cell: String,
    /// Increases when the guild moves; older writers are fenced off.
    pub epoch: i64,
    /// Version of the placement table that introduced this entry.
    pub version: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum DirectoryError {
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "no_capacity")]
    NoCapacity,
    #[serde(rename = "unavailable")]
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PlacementBatch {
    pub placements: Vec<Placement>,
    /// Pass as `after` to the next watch.
    pub version: i64,
}



use serde::{Serialize, Deserialize};
use async_trait::async_trait;

#[async_trait]
pub trait Directory: Send + Sync + 'static {
    /// Picks cells for a new guild, creates it there and commits the
    /// placement. The returned placement is already visible to get_placement.
    async fn create_guild(&self, body: GuildRequest) -> Result<Placement, DirectoryError>;
    async fn get_placement(&self, guild_id: String) -> Result<Placement, DirectoryError>;
    /// Long poll: placements committed after version `after`.
    async fn watch_placements(&self, after: i64, wait_ms: i64) -> PlacementBatch;
    async fn add_membership(&self, user_id: String, guild_id: String);
    async fn remove_membership(&self, user_id: String, guild_id: String);
    async fn list_memberships(&self, user_id: String) -> Vec<String>;
}

