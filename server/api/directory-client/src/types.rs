use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum ThrowsOrOtherError<T> {
    Throws(T),
    Other(reqwest::Error),
}

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

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct MembershipBatch {
    pub changes: Vec<MembershipChange>,
    /// Pass as `after` to the next watch.
    pub version: i64,
    /// Changes up to this version were trimmed. A follower whose `after` is
    /// below it has missed some.
    pub trimmed: i64,
}

/// Someone joined or left a guild.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct MembershipChange {
    pub user_id: String,
    pub guild_id: String,
    pub joined: bool,
    /// Position in the stream of membership changes.
    pub version: i64,
}



pub struct Directory {
    pub url: String,
    pub client: reqwest::Client,
}

impl Directory {
    pub fn new(url: impl Into<String>) -> Self {
        let client = reqwest::Client::new();
        Self { url: url.into(), client }
    }

    /// Picks cells for a new guild, creates it there and commits the
    /// placement. The returned placement is already visible to get_placement.
    pub async fn create_guild(&self, __body: GuildRequest) -> Result<Placement, ThrowsOrOtherError<DirectoryError>> {
        let mut request = self.client.post(format!("{}/directory/create_guild", &self.url));
        request = request.header("Accept", "application/json");
        request = request.json(&__body);
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn get_placement(&self, guild_id: String) -> Result<Placement, ThrowsOrOtherError<DirectoryError>> {
        let mut request = self.client.get(format!("{}/directory/get_placement", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Long poll: placements committed after version `after`.
    pub async fn watch_placements(&self, after: i64, wait_ms: i64) -> Result<PlacementBatch, ThrowsOrOtherError<DirectoryError>> {
        let mut request = self.client.get(format!("{}/directory/watch_placements", &self.url));
        request = request.query(&[("after", serde_json::to_string(&after).unwrap()), ("wait_ms", serde_json::to_string(&wait_ms).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn add_membership(&self, user_id: String, guild_id: String) -> Result<(), ThrowsOrOtherError<DirectoryError>> {
        let mut request = self.client.post(format!("{}/directory/add_membership", &self.url));
        request = request.query(&[("user_id", serde_json::to_string(&user_id).unwrap()), ("guild_id", serde_json::to_string(&guild_id).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn remove_membership(&self, user_id: String, guild_id: String) -> Result<(), ThrowsOrOtherError<DirectoryError>> {
        let mut request = self.client.post(format!("{}/directory/remove_membership", &self.url));
        request = request.query(&[("user_id", serde_json::to_string(&user_id).unwrap()), ("guild_id", serde_json::to_string(&guild_id).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn list_memberships(&self, user_id: String) -> Result<Vec<String>, ThrowsOrOtherError<DirectoryError>> {
        let mut request = self.client.get(format!("{}/directory/list_memberships", &self.url));
        request = request.query(&[("user_id", serde_json::to_string(&user_id).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Long poll: membership changes after version `after`. Edges follow it
    /// so that a user's sessions on every Edge learn about a guild they
    /// joined, wherever they joined it. An `after` beyond the newest change
    /// returns at once with the newest version, to start from there.
    pub async fn watch_memberships(&self, after: i64, wait_ms: i64) -> Result<MembershipBatch, ThrowsOrOtherError<DirectoryError>> {
        let mut request = self.client.get(format!("{}/directory/watch_memberships", &self.url));
        request = request.query(&[("after", serde_json::to_string(&after).unwrap()), ("wait_ms", serde_json::to_string(&wait_ms).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Moves a guild to another Message Cell (operators only). Its writes
    /// wait while it is copied; then the placement switches and Edges
    /// follow it there.
    pub async fn move_guild(&self, guild_id: String, message_cell: String) -> Result<Placement, ThrowsOrOtherError<DirectoryError>> {
        let mut request = self.client.post(format!("{}/directory/move_guild", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("message_cell", serde_json::to_string(&message_cell).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
}
