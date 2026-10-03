use chrono::{DateTime, Utc};
use robi_core::ids::{SessionId, WorkspaceId};

/// Regex lists that decide which paths a session may read or write.
///
/// An empty allow list grants no exceptions. A write allow that beats a write
/// deny also permits the read. When several patterns match one
/// path, the match that ends furthest into the path wins. At the same end
/// byte, the pattern with more literal characters wins, and a deny wins a
/// remaining tie. Allowing a parent does not outrank a deny of a child name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathRules {
    pub allow_read: Vec<String>,
    pub allow_write: Vec<String>,
    pub deny_read: Vec<String>,
    pub deny_write: Vec<String>,
}

impl PathRules {
    /// Built-in patterns first, then the patterns stored on the session.
    ///
    /// The database holds only the session additions. Allow lists have no
    /// built-in patterns, so the stored allow list is the whole allow list.
    pub fn with_system_defaults(&self) -> Self {
        Self {
            allow_read: self.allow_read.clone(),
            allow_write: self.allow_write.clone(),
            deny_read: append_patterns(default_deny_patterns(), &self.deny_read),
            deny_write: append_patterns(default_deny_patterns(), &self.deny_write),
        }
    }

    /// Compile every stored pattern. A bad regex is the caller's `BadRequest`.
    pub fn validate(&self) -> Result<(), String> {
        for pattern in self
            .allow_read
            .iter()
            .chain(&self.allow_write)
            .chain(&self.deny_read)
            .chain(&self.deny_write)
        {
            validate_pattern(pattern)?;
        }
        Ok(())
    }
}

/// Secret files, VCS internals, and anything outside the workspace.
///
/// `^\.\.(/|$)` matches a relative path that starts with `..`. A grant of
/// `../gopi` reaches further, so that tree is allowed and a sibling is not.
pub fn default_deny_patterns() -> Vec<String> {
    vec![
        r"^\.\.(/|$)".to_owned(),
        r"(^|/)\.git(/|$)".to_owned(),
        r"(^|/)\.env$".to_owned(),
        r"(^|/)\.env\.[^/]+$".to_owned(),
        r"(^|/)[^/]+\.(pem|key)$".to_owned(),
        r"(^|/)id_rsa$".to_owned(),
        r"(^|/)id_ed25519$".to_owned(),
        r"(^|/)credentials\.json$".to_owned(),
        r"(^|/)secrets\.json$".to_owned(),
    ]
}

fn append_patterns(mut base: Vec<String>, extra: &[String]) -> Vec<String> {
    base.extend(extra.iter().cloned());
    base
}

pub fn validate_pattern(pattern: &str) -> Result<(), String> {
    regex::Regex::new(pattern)
        .map(|_| ())
        .map_err(|err| format!("invalid path pattern `{pattern}`: {err}"))
}

/// Which tool set and prompt prefix a session uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentMode {
    Ask,
    Plan,
    #[default]
    Agent,
}

impl AgentMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Plan => "plan",
            Self::Agent => "agent",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "ask" => Ok(Self::Ask),
            "plan" => Ok(Self::Plan),
            "agent" => Ok(Self::Agent),
            other => Err(format!("mode must be ask, plan, or agent: {other}")),
        }
    }
}

/// Model and effort for one mode.
///
/// An absent key inherits that mode's setting, then the fallback setting.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModeOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

impl ModeOverride {
    pub fn is_empty(&self) -> bool {
        self.model.is_none() && self.reasoning_effort.is_none()
    }
}

/// Per-mode model and effort overrides.
///
/// `{}` means every mode inherits. A stored object from before modes, with
/// top-level `model` and `reasoning_effort`, loads as the agent override.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct ModelConfig {
    #[serde(default, skip_serializing_if = "ModeOverride::is_empty")]
    pub agent: ModeOverride,
    #[serde(default, skip_serializing_if = "ModeOverride::is_empty")]
    pub ask: ModeOverride,
    #[serde(default, skip_serializing_if = "ModeOverride::is_empty")]
    pub plan: ModeOverride,
}

impl ModelConfig {
    pub fn for_mode(&self, mode: AgentMode) -> &ModeOverride {
        match mode {
            AgentMode::Ask => &self.ask,
            AgentMode::Plan => &self.plan,
            AgentMode::Agent => &self.agent,
        }
    }

    pub fn for_mode_mut(&mut self, mode: AgentMode) -> &mut ModeOverride {
        match mode {
            AgentMode::Ask => &mut self.ask,
            AgentMode::Plan => &mut self.plan,
            AgentMode::Agent => &mut self.agent,
        }
    }
}

impl<'de> serde::Deserialize<'de> for ModelConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Wire {
            #[serde(default)]
            model: Option<String>,
            #[serde(default)]
            reasoning_effort: Option<String>,
            #[serde(default)]
            agent: ModeOverride,
            #[serde(default)]
            ask: ModeOverride,
            #[serde(default)]
            plan: ModeOverride,
        }

        let wire = Wire::deserialize(deserializer)?;
        let mut config = ModelConfig {
            agent: wire.agent,
            ask: wire.ask,
            plan: wire.plan,
        };
        if config.agent.is_empty() && (wire.model.is_some() || wire.reasoning_effort.is_some()) {
            config.agent = ModeOverride {
                model: wire.model,
                reasoning_effort: wire.reasoning_effort,
            };
        }
        Ok(config)
    }
}

/// Merge into one mode's override.
///
/// `None` leaves that key. `Some(None)` clears it. `Some(Some)` sets it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModeOverrideUpdate {
    pub model: Option<Option<String>>,
    pub reasoning_effort: Option<Option<String>>,
}

impl ModeOverrideUpdate {
    pub fn is_empty(&self) -> bool {
        self.model.is_none() && self.reasoning_effort.is_none()
    }

    pub fn apply(&self, target: &mut ModeOverride) {
        if let Some(model) = &self.model {
            target.model.clone_from(model);
        }
        if let Some(effort) = &self.reasoning_effort {
            target.reasoning_effort.clone_from(effort);
        }
    }
}

/// Merge into a stored [`ModelConfig`]. An absent mode is left as stored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelConfigUpdate {
    pub agent: Option<ModeOverrideUpdate>,
    pub ask: Option<ModeOverrideUpdate>,
    pub plan: Option<ModeOverrideUpdate>,
}

impl ModelConfigUpdate {
    pub fn is_empty(&self) -> bool {
        self.agent.as_ref().is_none_or(ModeOverrideUpdate::is_empty)
            && self.ask.as_ref().is_none_or(ModeOverrideUpdate::is_empty)
            && self.plan.as_ref().is_none_or(ModeOverrideUpdate::is_empty)
    }

    pub fn for_mode_mut(&mut self, mode: AgentMode) -> &mut Option<ModeOverrideUpdate> {
        match mode {
            AgentMode::Ask => &mut self.ask,
            AgentMode::Plan => &mut self.plan,
            AgentMode::Agent => &mut self.agent,
        }
    }
}

/// One conversation in one workspace.
///
/// `title` stays unset until something writes one. The model does that after
/// the first turn, unless create or a rename already supplied one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatSession {
    pub id: SessionId,
    pub workspace_id: WorkspaceId,
    pub title: Option<String>,
    pub path_rules: PathRules,
    pub mode: AgentMode,
    pub model_config: ModelConfig,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
}

/// Open a chat session. The adapter mints the id and the timestamps.
///
/// `title` is optional. Absent means the model still has to name the chat
/// session after the first turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateChatSessionCommand {
    pub workspace_id: WorkspaceId,
    pub title: Option<String>,
    pub mode: AgentMode,
    pub model_config: ModelConfig,
}

/// Change a chat session's title, path rules, or model config. The workspace stays put.
///
/// A `None` field is left as stored. `updated_at` moves only when a field is
/// present. `last_used_at` does not move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateChatSessionCommand {
    pub id: SessionId,
    pub title: Option<String>,
    pub allow_read: Option<Vec<String>>,
    pub allow_write: Option<Vec<String>>,
    pub deny_read: Option<Vec<String>>,
    pub deny_write: Option<Vec<String>>,
    pub mode: Option<AgentMode>,
    pub model_config: Option<ModelConfigUpdate>,
}

impl UpdateChatSessionCommand {
    pub fn rename(id: SessionId, title: impl Into<String>) -> Self {
        Self {
            id,
            title: Some(title.into()),
            allow_read: None,
            allow_write: None,
            deny_read: None,
            deny_write: None,
            mode: None,
            model_config: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.allow_read.is_none()
            && self.allow_write.is_none()
            && self.deny_read.is_none()
            && self.deny_write.is_none()
            && self.mode.is_none()
            && self
                .model_config
                .as_ref()
                .is_none_or(ModelConfigUpdate::is_empty)
    }

    /// Validate the patterns this command would write.
    pub fn validate_patterns(&self) -> Result<(), String> {
        for patterns in [
            &self.allow_read,
            &self.allow_write,
            &self.deny_read,
            &self.deny_write,
        ]
        .into_iter()
        .flatten()
        {
            for pattern in patterns {
                validate_pattern(pattern)?;
            }
        }
        Ok(())
    }
}

/// Apply a patch onto `session`. Returns whether any field was present.
pub fn apply_session_update(session: &mut ChatSession, command: &UpdateChatSessionCommand) -> bool {
    if command.is_empty() {
        return false;
    }
    if let Some(title) = &command.title {
        session.title = Some(title.clone());
    }
    if let Some(patterns) = &command.allow_read {
        session.path_rules.allow_read.clone_from(patterns);
    }
    if let Some(patterns) = &command.allow_write {
        session.path_rules.allow_write.clone_from(patterns);
    }
    if let Some(patterns) = &command.deny_read {
        session.path_rules.deny_read.clone_from(patterns);
    }
    if let Some(patterns) = &command.deny_write {
        session.path_rules.deny_write.clone_from(patterns);
    }
    if let Some(mode) = command.mode {
        session.mode = mode;
    }
    if let Some(update) = &command.model_config {
        if let Some(agent) = &update.agent {
            agent.apply(&mut session.model_config.agent);
        }
        if let Some(ask) = &update.ask {
            ask.apply(&mut session.model_config.ask);
        }
        if let Some(plan) = &update.plan {
            plan.apply(&mut session.model_config.plan);
        }
    }
    true
}
