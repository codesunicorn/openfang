//! Channel Bridge Layer for the OpenFang Agent OS.
//!
//! Provides pluggable messaging integrations that convert platform messages
//! into unified `ChannelMessage` events for the kernel.
//!
//! Adapter compilation is feature-gated: the four core platforms (Discord,
//! Telegram, Slack, Matrix) are always built; everything else is opt-in so a
//! stock build does not carry ~25K lines of adapters the operator never runs.
//! Enable extras with `--features openfang-channels/all` (the CLI does this by
//! default — see the workspace `full-channels` feature).

pub mod bridge;
pub mod formatter;
pub mod router;
pub mod types;

// ── Core adapters (always compiled) ────────────────────────────────
pub mod discord;
pub mod matrix;
pub mod slack;
pub mod telegram;

#[cfg(feature = "email")]
pub mod email;
#[cfg(feature = "google_chat")]
pub mod google_chat;
#[cfg(feature = "irc")]
pub mod irc;
#[cfg(feature = "mattermost")]
pub mod mattermost;
#[cfg(feature = "rocketchat")]
pub mod rocketchat;
#[cfg(feature = "signal")]
pub mod signal;
#[cfg(feature = "teams")]
pub mod teams;
#[cfg(feature = "twitch")]
pub mod twitch;
#[cfg(feature = "whatsapp")]
pub mod whatsapp;
#[cfg(feature = "xmpp")]
pub mod xmpp;
#[cfg(feature = "zulip")]
pub mod zulip;

// Wave 3 — High-value channels
#[cfg(feature = "bluesky")]
pub mod bluesky;
#[cfg(feature = "feishu")]
pub mod feishu;
#[cfg(feature = "line")]
pub mod line;
#[cfg(feature = "mastodon")]
pub mod mastodon;
#[cfg(feature = "messenger")]
pub mod messenger;
#[cfg(feature = "reddit")]
pub mod reddit;
#[cfg(feature = "revolt")]
pub mod revolt;
#[cfg(feature = "viber")]
pub mod viber;

// Wave 4 — Enterprise & community channels
#[cfg(feature = "flock")]
pub mod flock;
#[cfg(feature = "guilded")]
pub mod guilded;
#[cfg(feature = "keybase")]
pub mod keybase;
#[cfg(feature = "nextcloud")]
pub mod nextcloud;
#[cfg(feature = "nostr")]
pub mod nostr;
#[cfg(feature = "pumble")]
pub mod pumble;
#[cfg(feature = "threema")]
pub mod threema;
#[cfg(feature = "twist")]
pub mod twist;
#[cfg(feature = "webex")]
pub mod webex;

// Wave 5 — Niche & differentiating channels
#[cfg(feature = "dingtalk")]
pub mod dingtalk;
#[cfg(feature = "dingtalk_stream")]
pub mod dingtalk_stream;
#[cfg(feature = "discourse")]
pub mod discourse;
#[cfg(feature = "gitter")]
pub mod gitter;
#[cfg(feature = "gotify")]
pub mod gotify;
#[cfg(feature = "linkedin")]
pub mod linkedin;
#[cfg(feature = "mumble")]
pub mod mumble;
#[cfg(feature = "mqtt")]
pub mod mqtt;
#[cfg(feature = "ntfy")]
pub mod ntfy;
#[cfg(feature = "webhook")]
pub mod webhook;
#[cfg(feature = "wecom")]
pub mod wecom;
