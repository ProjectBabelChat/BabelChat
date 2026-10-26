//! The context hierarchy of a conversation, turned into engine layers.
//!
//! ```text
//! space        name, description (or its digest), glossary   kind = space
//! category     title and description                         kind = conversation
//! channel      title and description                         kind = conversation
//! thread       frozen fork of its parent                     kind = fork
//! summary      what left the conversation's window           kind = summary
//! ```
//!
//! A new thread is never translated without context: it inherits the space
//! and its parents, plus a snapshot of where it forked from.

use babel_api_engine::types::{ContextLayer, LayerKind};
use babel_api_translation_cell::types::{NodeKind, SpaceContext};

use crate::records::{NodeRecord, SpaceRecord, WindowEntry, WindowRecord};

/// How far up the node tree a prompt reaches.
pub const MAX_DEPTH: usize = 4;

pub fn space_layer(space: &str, record: &SpaceRecord) -> Option<ContextLayer> {
    let context = record.context.as_ref()?;
    Some(ContextLayer {
        id: format!("space:{space}"),
        version: record.context_version,
        kind: LayerKind::Space,
        lang: context.description_lang.clone(),
        content: render_space(context, record.digest.as_deref()),
    })
}

fn render_space(context: &SpaceContext, digest: Option<&str>) -> String {
    let mut content = format!("Community: {}\n", context.name);
    if let Some(description) = digest.or(context.description.as_deref()) {
        content += &format!("About: {description}\n");
    }
    if !context.glossary.is_empty() {
        content += "Glossary:\n";
        for term in &context.glossary {
            match &term.note {
                Some(note) => content += &format!("- {}: {}\n", term.term, note),
                None => content += &format!("- {}\n", term.term),
            }
        }
    }
    content
}

/// Layers for the nodes from the root down to `chain[0]`'s conversation.
/// `chain` lists the conversation first, then its ancestors.
pub fn node_layers(chain: &[NodeRecord]) -> Vec<ContextLayer> {
    chain
        .iter()
        .rev()
        .filter_map(|record| {
            let node = &record.node;
            match (&record.fork, node.kind) {
                (Some(fork), NodeKind::Thread) => Some(ContextLayer {
                    id: format!("node:{}", node.id),
                    version: record.version,
                    kind: LayerKind::Fork,
                    lang: None,
                    content: fork.clone(),
                }),
                _ if node.title.is_empty() && node.description.is_none() => None,
                _ => Some(ContextLayer {
                    id: format!("node:{}", node.id),
                    version: record.version,
                    kind: LayerKind::Conversation,
                    lang: None,
                    content: render_node(record),
                }),
            }
        })
        .collect()
}

fn render_node(record: &NodeRecord) -> String {
    let node = &record.node;
    let kind = match node.kind {
        NodeKind::Category => "Category",
        NodeKind::Channel => "Channel",
        NodeKind::Thread => "Thread",
    };
    let mut content = format!("{kind}: {}\n", node.title);
    if let Some(description) = &node.description {
        content += &format!("Topic: {description}\n");
    }
    content
}

pub fn summary_layer(conversation: &str, window: &WindowRecord) -> Option<ContextLayer> {
    window.summary.as_ref().map(|summary| ContextLayer {
        id: format!("summary:{conversation}"),
        version: window.summary_version,
        kind: LayerKind::Summary,
        lang: None,
        content: summary.clone(),
    })
}

/// The frozen context of a new thread: its parent, the message it started
/// from and the last few messages before it.
pub fn fork_snapshot(
    thread_title: &str,
    parent: Option<&NodeRecord>,
    parent_window: &WindowRecord,
    starter_id: Option<&str>,
    recent: usize,
) -> String {
    let parent_title = parent
        .map(|parent| parent.node.title.as_str())
        .unwrap_or_default();
    let mut content = format!("Thread \"{thread_title}\" forked from \"{parent_title}\".\n");

    if let Some(summary) = &parent_window.summary {
        content += &format!("Earlier in the parent: {summary}\n");
    }

    let entries = &parent_window.entries;
    let end = starter_id
        .and_then(|id| entries.iter().position(|entry| entry.id == id))
        .map(|index| index + 1)
        .unwrap_or(entries.len());
    let start = end.saturating_sub(recent);

    if start < end {
        content += "Messages before the thread:\n";
        for WindowEntry { author, text, .. } in &entries[start..end] {
            content += &format!("{author}: {text}\n");
        }
    }
    if let Some(starter) = starter_id.and_then(|id| entries.iter().find(|entry| entry.id == id)) {
        content += &format!(
            "The thread started from {}: {}\n",
            starter.author, starter.text
        );
    }
    content
}
