//! The composer band's last row: mode, model, provider, effort, and the meter.
//!
//! The webview puts these controls inside the composer, and so does the terminal —
//! they describe what the next send will carry, not what the session is. It is one
//! row, because the band's bottom padding row was already there and the surface
//! should not grow a second row of chrome.
//!
//! **Clipping is ordered.** A narrow terminal loses the meter first, then the
//! provider, then the effort, then the model. The mode word is never cut: it is the
//! one fact the row exists to state, and the one `Ctrl-T` changes.

use crate::adapters::model_source::EffectiveChoice;
use crate::agent::providers::ProviderKind;
use crate::domain::chat_session::model::AgentMode;
use crate::tui::state::context::{self, Estimate};

/// One piece of the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// `Agent`, `Ask`, `Plan` — never dropped.
    Mode(&'static str, AgentMode),
    Model(String),
    Provider(&'static str),
    /// Owned, because the effort is a stored string that the settings screen can
    /// change: a `&'static str` here would mean leaking one per frame.
    Effort(String),
    /// `214k / 1.0M` or `214k tokens`.
    Meter(String),
}

impl Segment {
    /// The cells the segment takes, separators included.
    pub fn width(&self) -> usize {
        self.text().chars().count()
    }

    pub fn text(&self) -> String {
        match self {
            Self::Mode(label, _) => (*label).to_owned(),
            Self::Model(label) => label.clone(),
            Self::Provider(name) => (*name).to_owned(),
            Self::Effort(label) => label.clone(),
            Self::Meter(text) => text.clone(),
        }
    }

    /// Whether this segment may be dropped when the band is narrow.
    fn droppable(&self) -> bool {
        !matches!(self, Self::Mode(..))
    }
}

/// The row, ready to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub segments: Vec<Segment>,
}

/// The mode's own word, as the band spells it.
pub fn mode_label(mode: AgentMode) -> &'static str {
    match mode {
        AgentMode::Ask => "Ask",
        AgentMode::Plan => "Plan",
        AgentMode::Agent => "Agent",
    }
}

/// `low` → `Low`. An effort the setting cannot spell is shown as it was written.
fn effort_label(effort: &str) -> String {
    let mut chars = effort.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

impl Facts {
    /// Build the row.
    ///
    /// `effective` is the resolution from `adapters::model_source`, so the row says
    /// what the next turn will really use — the mode's setting and the fallback
    /// included, not just the session's own override.
    pub fn build(
        mode: AgentMode,
        effective: Option<&EffectiveChoice>,
        estimate: Estimate,
        model_label: impl Fn(&str) -> String,
    ) -> Self {
        let mut segments = vec![Segment::Mode(mode_label(mode), mode)];
        if let Some(effective) = effective {
            segments.push(Segment::Model(model_label(&effective.model)));
            segments.push(Segment::Provider(
                ProviderKind::of(&effective.model).display_name(),
            ));
            if let Some(effort) = &effective.effort {
                segments.push(Segment::Effort(effort_label(effort)));
            }
        }
        if let Some(meter) = meter_text(estimate) {
            segments.push(Segment::Meter(meter));
        }
        Self { segments }
    }

    /// The row's text at `width` cells, dropping the least useful segments first.
    ///
    /// Returns the segments that survive, so the view can style each one. The mode
    /// is kept even when it alone overflows: a clipped mode is still better than a
    /// blank row, and the band clips it in the view.
    pub fn fitted(&self, width: usize) -> Vec<Segment> {
        let mut kept: Vec<Segment> = self.segments.clone();
        // Drop order is the reverse of importance. The mode is never in this list.
        const DROP_ORDER: [usize; 4] = [4, 3, 2, 1];
        loop {
            if total_width(&kept) <= width {
                return kept;
            }
            let Some(next) = DROP_ORDER
                .iter()
                .filter_map(|index| kept.get(*index))
                .find(|segment| segment.droppable())
                .cloned()
            else {
                return kept;
            };
            kept.retain(|segment| segment != &next);
        }
    }

    /// The left group and the meter, split, so the view can right-align the meter.
    ///
    /// `None` when the meter did not survive the fit.
    pub fn split(&self, width: usize) -> (Vec<Segment>, Option<Segment>) {
        let fitted = self.fitted(width);
        match fitted.iter().position(|s| matches!(s, Segment::Meter(_))) {
            Some(index) => {
                let mut left = fitted.clone();
                let meter = left.remove(index);
                (left, Some(meter))
            }
            None => (fitted, None),
        }
    }
}

fn total_width(segments: &[Segment]) -> usize {
    let texts: usize = segments.iter().map(Segment::width).sum();
    // ` · ` between each pair.
    texts + segments.len().saturating_sub(1) * 3
}

fn meter_text(estimate: Estimate) -> Option<String> {
    let used = estimate.used?;
    match estimate.window {
        Some(window) => Some(format!(
            "{} / {} \u{b7} {}%",
            context::format_tokens(used),
            context::format_tokens(window),
            estimate.percent.unwrap_or(0)
        )),
        None => Some(format!("{} tokens", context::format_tokens(used))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use robi_core::message::Usage;

    fn effective(model: &str, effort: Option<&str>) -> EffectiveChoice {
        EffectiveChoice {
            model: model.to_owned(),
            effort: effort.map(str::to_owned),
        }
    }

    fn estimate(used: u64, window: Option<u64>) -> Estimate {
        Estimate {
            reported: Some(used),
            uncounted: 0,
            used: Some(used),
            percent: window.map(|window| (used * 100 / window).min(100)),
            last_turn: Some(Usage::default()),
            window,
        }
    }

    fn plain(model: &str) -> String {
        model.to_owned()
    }

    fn texts(segments: &[Segment]) -> Vec<String> {
        segments.iter().map(Segment::text).collect()
    }

    #[test]
    fn the_row_reads_mode_model_provider_effort_then_the_meter() {
        let facts = Facts::build(
            AgentMode::Plan,
            Some(&effective("ocg_glm-5.3", Some("high"))),
            estimate(214_000, Some(1_000_000)),
            |model| format!("labelled {model}"),
        );
        assert_eq!(
            texts(&facts.segments),
            [
                "Plan",
                "labelled ocg_glm-5.3",
                "OpenCode Go",
                "High",
                "214k / 1.0M \u{b7} 21%"
            ]
        );
        assert_eq!(facts.segments[0], Segment::Mode("Plan", AgentMode::Plan));
    }

    #[test]
    fn a_mode_with_no_effort_and_no_usage_says_less() {
        let facts = Facts::build(
            AgentMode::Ask,
            Some(&effective("ant_claude", None)),
            Estimate::default(),
            plain,
        );
        assert_eq!(texts(&facts.segments), ["Ask", "ant_claude", "Anthropic"]);
    }

    #[test]
    fn a_window_the_caller_does_not_know_reads_as_tokens() {
        let facts = Facts::build(
            AgentMode::Agent,
            Some(&effective("ocg_glm-5.3", None)),
            estimate(214_000, None),
            plain,
        );
        assert_eq!(
            facts.segments.last().map(Segment::text),
            Some("214k tokens".to_owned())
        );
    }

    #[test]
    fn before_the_first_read_the_row_still_names_the_mode() {
        let facts = Facts::build(AgentMode::Agent, None, Estimate::default(), plain);
        assert_eq!(texts(&facts.segments), ["Agent"]);
    }

    #[test]
    fn a_narrow_band_drops_the_meter_then_the_provider_then_the_effort() {
        let facts = Facts::build(
            AgentMode::Plan,
            Some(&effective("ocg_glm-5.3", Some("high"))),
            estimate(214_000, Some(1_000_000)),
            plain,
        );
        let wide = texts(&facts.fitted(200));
        assert_eq!(wide.len(), 5);

        // The meter goes first.
        let narrower = texts(&facts.fitted(50));
        assert_eq!(narrower.len(), 4);
        assert!(!narrower.iter().any(|text| text.contains('/')));

        // Then the provider, then the effort.
        let tighter = texts(&facts.fitted(30));
        assert!(tighter.len() <= 3);
        assert!(!tighter.iter().any(|text| text == "OpenCode Go"));

        // The mode survives even when nothing else can fit.
        let tiny = texts(&facts.fitted(3));
        assert_eq!(tiny, ["Plan"]);
    }

    #[test]
    fn the_meter_is_split_out_so_the_view_can_right_align_it() {
        let facts = Facts::build(
            AgentMode::Agent,
            Some(&effective("ocg_glm-5.3", Some("low"))),
            estimate(1_000, Some(10_000)),
            plain,
        );
        let (left, meter) = facts.split(200);
        assert_eq!(texts(&left), ["Agent", "ocg_glm-5.3", "OpenCode Go", "Low"]);
        assert_eq!(
            meter.map(|segment| segment.text()),
            Some("1.0k / 10k \u{b7} 10%".to_owned())
        );

        // A band too narrow for the meter still returns the left group.
        let (left, meter) = facts.split(20);
        assert!(meter.is_none(), "the meter was dropped");
        assert!(!left.is_empty());
    }
}
