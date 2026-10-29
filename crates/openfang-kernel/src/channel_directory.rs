//! Channel directory — a learned name→id map of reachable chat channels.
//!
//! Agents (and humans) refer to places by name ("post it in #taau"), but
//! platform APIs speak opaque ids. This module remembers every channel the bot
//! has actually seen traffic in, plus an optional operator-declared alias map,
//! so `channel_send` can resolve a human name to a real id.
//!
//! Sources, in increasing priority:
//! 1. Observed traffic — every inbound message records (channel_type, id,
//!    display name when known). Free: the bot already sees these.
//! 2. Operator aliases — `[channels.<type>] channel_names = { "#taau" = "123" }`.
//!
//! Entries are persisted to `{data_dir}/channel_directory.json` so the map
//! survives restarts and is inspectable/editable by hand.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

/// One known channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryEntry {
    /// Channel adapter type (`discord`, `telegram`, ...).
    pub channel_type: String,
    /// Platform channel/chat id.
    pub id: String,
    /// Human-readable name (`#taau`, `salon-expert`, ...).
    pub name: String,
    /// `observed` (learned from traffic) or `config` (operator-declared).
    pub source: String,
    /// RFC3339 timestamp of the last observation.
    pub last_seen: String,
}

/// Thread-safe, file-backed channel directory.
pub struct ChannelDirectory {
    path: PathBuf,
    /// Keyed by (channel_type, lowercase name) for case-insensitive lookup.
    entries: RwLock<BTreeMap<(String, String), DirectoryEntry>>,
}

impl ChannelDirectory {
    /// Load the directory from `data_dir/channel_directory.json`, creating an
    /// empty one if the file is missing or unreadable.
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join("channel_directory.json");
        let mut entries = BTreeMap::new();
        if let Ok(raw) = std::fs::read_to_string(&path) {
            match serde_json::from_str::<Vec<DirectoryEntry>>(&raw) {
                Ok(list) => {
                    for e in list {
                        entries.insert((e.channel_type.clone(), e.name.to_lowercase()), e);
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, path = %path.display(), "channel directory unreadable; starting empty");
                }
            }
        }
        Self {
            path,
            entries: RwLock::new(entries),
        }
    }

    fn persist(&self, map: &BTreeMap<(String, String), DirectoryEntry>) {
        let list: Vec<&DirectoryEntry> = map.values().collect();
        match serde_json::to_string_pretty(&list) {
            Ok(json) => {
                if let Some(parent) = self.path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::write(&self.path, json) {
                    tracing::warn!(error = %e, "failed to persist channel directory");
                }
            }
            Err(e) => tracing::warn!(error = %e, "failed to serialize channel directory"),
        }
    }

    /// Record traffic in a channel (called on every inbound message).
    ///
    /// `name` is whatever the platform gave us — a guild channel name, a
    /// group title, or the chat id as a last resort.
    pub fn observe(&self, channel_type: &str, id: &str, name: Option<&str>) {
        if id.trim().is_empty() {
            return;
        }
        let display = name
            .map(|n| n.trim().trim_start_matches('#'))
            .filter(|n| !n.is_empty())
            .unwrap_or(id);
        let key = (channel_type.to_string(), display.to_lowercase());
        let now = chrono::Utc::now().to_rfc3339();

        let mut map = match self.entries.write() {
            Ok(m) => m,
            Err(_) => return,
        };
        let entry = map.entry(key).or_insert_with(|| DirectoryEntry {
            channel_type: channel_type.to_string(),
            id: id.to_string(),
            name: display.to_string(),
            source: "observed".to_string(),
            last_seen: now.clone(),
        });
        // Refreshes always win: the id can change (e.g. a re-created guild
        // channel) and we want the newest observed mapping. Operator-declared
        // entries are not overwritten by observation.
        if entry.source == "observed" {
            entry.id = id.to_string();
            entry.last_seen = now;
        }
        self.persist(&map);
    }

    /// Add or replace an operator-declared alias (config-sourced, wins over
    /// observations for the same name).
    pub fn set_config_alias(&self, channel_type: &str, name: &str, id: &str) {
        let display = name.trim().trim_start_matches('#');
        if display.is_empty() || id.trim().is_empty() {
            return;
        }
        let key = (channel_type.to_string(), display.to_lowercase());
        let mut map = match self.entries.write() {
            Ok(m) => m,
            Err(_) => return,
        };
        map.insert(
            key,
            DirectoryEntry {
                channel_type: channel_type.to_string(),
                id: id.trim().to_string(),
                name: display.to_string(),
                source: "config".to_string(),
                last_seen: chrono::Utc::now().to_rfc3339(),
            },
        );
        self.persist(&map);
    }

    /// Resolve a human name (or a raw id) to a platform channel id.
    ///
    /// Accepts `#name`, `name`, or an already-valid id (returned as-is when
    /// nothing matches but the input looks like an id).
    pub fn resolve(&self, channel_type: &str, name_or_id: &str) -> Option<String> {
        let needle = name_or_id.trim().trim_start_matches('#');
        if needle.is_empty() {
            return None;
        }
        let map = self.entries.read().ok()?;
        if let Some(e) = map.get(&(channel_type.to_string(), needle.to_lowercase())) {
            return Some(e.id.clone());
        }
        // Fall back to matching by id (so callers can pass raw ids).
        map.values()
            .find(|e| e.channel_type == channel_type && e.id == needle)
            .map(|e| e.id.clone())
    }

    /// All entries, config-sourced first then most-recently-seen.
    pub fn list(&self) -> Vec<DirectoryEntry> {
        let Ok(map) = self.entries.read() else {
            return Vec::new();
        };
        let mut out: Vec<DirectoryEntry> = map.values().cloned().collect();
        out.sort_by(|a, b| {
            b.source
                .cmp(&a.source) // "observed" < "config" reversed → config first
                .then_with(|| b.last_seen.cmp(&a.last_seen))
        });
        out
    }

    /// Compact human-readable listing for prompting an LLM.
    pub fn render(&self, channel_type: Option<&str>) -> String {
        let entries: Vec<DirectoryEntry> = self
            .list()
            .into_iter()
            .filter(|e| channel_type.is_none_or(|t| e.channel_type == t))
            .collect();
        if entries.is_empty() {
            return "No channels known yet. Channels are learned from traffic — \
                    post somewhere once and it will appear here."
                .to_string();
        }
        let mut out = String::new();
        for e in entries {
            out.push_str(&format!(
                "- {}#{name} → {id} ({source}, seen {seen})\n",
                if e.channel_type.is_empty() {
                    String::new()
                } else {
                    format!("{} ", e.channel_type)
                },
                name = e.name,
                id = e.id,
                source = e.source,
                seen = e.last_seen.get(..10).unwrap_or(&e.last_seen),
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("of-chandir-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_observe_and_resolve() {
        let dir = tmp_dir();
        let d = ChannelDirectory::load(&dir);
        d.observe("discord", "1234", Some("#taau"));
        assert_eq!(d.resolve("discord", "#taau").as_deref(), Some("1234"));
        assert_eq!(d.resolve("discord", "TAau").as_deref(), Some("1234"));
        assert_eq!(d.resolve("discord", "1234").as_deref(), Some("1234"));
        assert_eq!(d.resolve("telegram", "#taau"), None);
        assert_eq!(d.resolve("discord", "#nope"), None);
    }

    #[test]
    fn test_config_alias_beats_observation() {
        let dir = tmp_dir();
        let d = ChannelDirectory::load(&dir);
        d.observe("discord", "111", Some("general"));
        d.set_config_alias("discord", "#general", "999");
        assert_eq!(d.resolve("discord", "general").as_deref(), Some("999"));
        // Re-observing must not clobber an operator alias.
        d.observe("discord", "222", Some("general"));
        assert_eq!(d.resolve("discord", "general").as_deref(), Some("999"));
    }

    #[test]
    fn test_persistence_roundtrip() {
        let dir = tmp_dir();
        {
            let d = ChannelDirectory::load(&dir);
            d.observe("discord", "42", Some("schoolify"));
        }
        let reloaded = ChannelDirectory::load(&dir);
        assert_eq!(reloaded.resolve("discord", "schoolify").as_deref(), Some("42"));
        assert!(dir.join("channel_directory.json").exists());
    }
}
