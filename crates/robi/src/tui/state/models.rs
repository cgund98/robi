//! The model and effort picker: `Ctrl-M`.
//!
//! One flat list with two groups, because the picker in the webview is two menus
//! and a terminal has no submenus worth the keystrokes. `Use default` is a row in
//! each group — the same word the webview uses — and the thing in force wears a
//! dot.
//!
//! **A stale row.** A session can hold a model id its provider filter no longer
//! offers, because the setting that hid it is on the settings screen. The effective
//! id is inserted at the top of its group so the list still shows what the session
//! points at, rather than silently offering a default instead: the webview's
//! `Composer.tsx::modelOptions` unshifts the selected id for the same reason.

use crate::adapters::model_source::EffectiveChoice;
use crate::agent::providers::catalog::menu_label;
use crate::agent::providers::{ModelCatalog, ModelId, ModelInfo, ProviderKind, ReasoningEffort};
use crate::domain::chat_session::model::{ChoiceKey, ModeOverride};

/// Which group a row belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Model,
    Effort,
}

/// What picking a row writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// Clear the key: inherit the mode's setting, then the fallback setting.
    Default(Group),
    /// Set a model id.
    Model(String),
    /// Set `low`, `medium`, or `high`.
    Effort(String),
}

impl Pick {
    pub fn key(&self) -> ChoiceKey {
        match self {
            Self::Default(Group::Model) | Self::Model(_) => ChoiceKey::Model,
            Self::Default(Group::Effort) | Self::Effort(_) => ChoiceKey::Effort,
        }
    }

    /// The value to write. `None` clears the key, which is `Use default`.
    pub fn value(&self) -> Option<String> {
        match self {
            Self::Default(_) => None,
            Self::Model(model) => Some(model.clone()),
            Self::Effort(effort) => Some(effort.clone()),
        }
    }
}

/// One row of the picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub group: Group,
    /// A group's heading, which the selection skips.
    pub heading: Option<&'static str>,
    pub label: String,
    /// The value the row stands for; `None` on a heading.
    pub pick: Option<Pick>,
    /// What the session would use now.
    pub current: bool,
}

impl Row {
    /// A heading row, which cannot be selected.
    pub fn is_heading(&self) -> bool {
        self.heading.is_some()
    }
}

/// A group's label: no value, so the selection steps over it.
fn heading(group: Group, label: &'static str) -> Row {
    Row {
        group,
        heading: Some(label),
        label: String::new(),
        pick: None,
        current: false,
    }
}

/// How the three efforts are spelled, in menu order.
const EFFORTS: [(ReasoningEffort, &str); 3] = [
    (ReasoningEffort::Low, "Low"),
    (ReasoningEffort::Medium, "Medium"),
    (ReasoningEffort::High, "High"),
];

/// The open picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    pub rows: Vec<Row>,
    pub selected: usize,
}

impl Picker {
    /// Build the list for one session.
    ///
    /// The dot marks the **stored** choice, which is what the webview's radio group
    /// does: `Use default` when the session has no override for that key, the
    /// override's own row otherwise. Marking the *effective* value instead would dot
    /// two rows at once — `Use default` and the model it resolves to — and leave the
    /// reader unable to tell what `Enter` would clear. `effective` is here so the
    /// stale row knows which id to show, not so it can mark it.
    pub fn new(
        models: &[ModelInfo],
        override_: &ModeOverride,
        effective: &EffectiveChoice,
    ) -> Self {
        let mut rows = Vec::new();

        let stored_effort = override_.reasoning_effort.as_deref();
        let stored_model = override_.model.as_deref();
        // Effort first: three fixed rows, and a reader changing it should not
        // scroll past every model to reach them.
        rows.push(heading(Group::Effort, "Effort"));
        rows.push(Row {
            group: Group::Effort,
            heading: None,
            label: "Use default".to_owned(),
            pick: Some(Pick::Default(Group::Effort)),
            current: override_.reasoning_effort.is_none(),
        });
        for (effort, label) in EFFORTS {
            rows.push(Row {
                group: Group::Effort,
                heading: None,
                label: label.to_owned(),
                pick: Some(Pick::Effort(effort.as_str().to_owned())),
                current: stored_effort == Some(effort.as_str()),
            });
        }
        rows.push(heading(Group::Model, "Model"));
        rows.push(Row {
            group: Group::Model,
            heading: None,
            label: "Use default".to_owned(),
            pick: Some(Pick::Default(Group::Model)),
            current: override_.model.is_none(),
        });

        // The stale row: the effective id, when the filter has hidden it.
        let listed = models
            .iter()
            .any(|info| info.id.as_str() == effective.model);
        if !listed {
            rows.push(Row {
                group: Group::Model,
                heading: None,
                label: stale_label(&effective.model),
                pick: Some(Pick::Model(effective.model.clone())),
                // Only when the session itself names this id. A hidden *setting* is
                // not a stored choice, and dotting it would make `Use default` look
                // like it does nothing.
                current: stored_model == Some(effective.model.as_str()),
            });
        }

        for info in models {
            let current = stored_model == Some(info.id.as_str());
            rows.push(Row {
                group: Group::Model,
                heading: None,
                label: menu_label(info),
                pick: Some(Pick::Model(info.id.as_str().to_owned())),
                current,
            });
        }

        let mut picker = Self { rows, selected: 0 };
        picker.select_current();
        picker.skip_heading(1);
        picker
    }

    /// Put the selection on the first row in force.
    fn select_current(&mut self) {
        if let Some(index) = self
            .rows
            .iter()
            .position(|row| row.current && !row.is_heading())
        {
            self.selected = index;
        }
    }

    /// Move, skipping a heading.
    pub fn move_by(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() as isize - 1;
        let mut next = (self.selected as isize + delta).clamp(0, last) as usize;
        let step = if delta.is_negative() { -1 } else { 1 };
        // A heading is a label, not a choice: step over it rather than landing on
        // it, which would leave `Enter` with nothing to write.
        while self.rows[next].is_heading() {
            let moved = next as isize + step;
            if moved < 0 || moved > last {
                return;
            }
            next = moved as usize;
        }
        self.selected = next;
    }

    fn skip_heading(&mut self, delta: isize) {
        if self.current().is_some_and(Row::is_heading) {
            self.move_by(delta);
        }
    }

    pub fn current(&self) -> Option<&Row> {
        self.rows.get(self.selected)
    }

    /// What `Enter` writes, or nothing on a heading.
    pub fn pick(&self) -> Option<&Pick> {
        self.current().and_then(|row| row.pick.as_ref())
    }

    /// The rows as drawable lines, with the current row marked.
    pub fn lines(&self) -> Vec<(String, bool, bool)> {
        self.rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let label = match row.heading {
                    Some(heading) => heading.to_owned(),
                    None => {
                        let dot = if row.current { "\u{25cf}" } else { " " };
                        format!("{dot} {}", row.label)
                    }
                };
                (label, index == self.selected, row.is_heading())
            })
            .collect()
    }
}

/// The label for an id the filter is hiding: the catalog's name when it has one,
/// the raw id when the catalog does not list it at all.
fn stale_label(model: &str) -> String {
    match ModelCatalog::all().get(&ModelId::new(model)) {
        Some(found) => menu_label(found),
        None => model.to_owned(),
    }
}

/// The provider a model id names, for the facts row.
pub fn provider_of(model: &str) -> ProviderKind {
    ProviderKind::of(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, name: &str) -> ModelInfo {
        ModelInfo {
            id: ModelId::new(id),
            display_name: name.to_owned(),
            context_window: 1_000_000,
            max_output: 1_000,
            supports_tools: true,
            supports_reasoning: false,
            supports_effort: false,
            supports_vision: false,
        }
    }

    fn models() -> Vec<ModelInfo> {
        vec![
            model("ocg_glm-5.3", "GLM-5.3"),
            model("ant_claude", "Claude"),
        ]
    }

    fn effective(model: &str, effort: Option<&str>) -> EffectiveChoice {
        EffectiveChoice {
            model: model.to_owned(),
            effort: effort.map(str::to_owned),
        }
    }

    fn picker(override_: ModeOverride, effective: &EffectiveChoice) -> Picker {
        Picker::new(&models(), &override_, effective)
    }

    #[test]
    fn the_list_is_two_groups_each_with_a_default_row() {
        let picker = picker(ModeOverride::default(), &effective("ocg_glm-5.3", None));
        let drawn: Vec<String> = picker
            .lines()
            .into_iter()
            .map(|(label, _, _)| label.trim_start_matches('\u{25cf}').trim().to_owned())
            .collect();
        assert_eq!(
            drawn,
            [
                // Effort first: three fixed rows, reachable without scrolling past
                // every model.
                "Effort",
                "Use default",
                "Low",
                "Medium",
                "High",
                "Model",
                "Use default",
                "OCG - GLM-5.3",
                "Claude"
            ]
        );
        assert_eq!(picker.rows.iter().filter(|row| row.is_heading()).count(), 2);
    }

    #[test]
    fn the_dot_marks_the_stored_choice_so_use_default_means_something() {
        // No override: `Use default` is dotted in both groups, even though the
        // setting resolves the model to one of the rows. Dotting both would make
        // `Enter` on `Use default` a surprise.
        let rows = picker(
            ModeOverride::default(),
            &effective("ant_claude", Some("low")),
        )
        .rows;
        let dotted: Vec<&str> = rows
            .iter()
            .filter(|row| row.current && !row.is_heading())
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(dotted, ["Use default", "Use default"]);

        // With an override, that override's row is dotted instead.
        let override_ = ModeOverride {
            model: Some("ant_claude".into()),
            reasoning_effort: Some("high".into()),
        };
        let rows = picker(override_, &effective("ant_claude", Some("high"))).rows;
        let dotted: Vec<&str> = rows
            .iter()
            .filter(|row| row.current && !row.is_heading())
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(dotted, ["High", "Claude"], "effort first, then the model");
    }

    #[test]
    fn a_model_the_filter_hid_is_still_offered_and_marked() {
        // The session points at a model the provider filter has hidden.
        let rows = picker(
            ModeOverride::default(),
            &effective("dsk_deepseek-flash", None),
        )
        .rows;
        let row = rows
            .iter()
            .find(|row| matches!(row.pick, Some(Pick::Model(_))))
            .expect("the stale row comes first, before the listed models");
        assert_eq!(row.pick, Some(Pick::Model("dsk_deepseek-flash".into())));
        assert!(row.label.starts_with("DeepSeek"));
        // The catalog knows the id, so it is labelled, not printed raw.
        assert!(
            row.label.starts_with("DeepSeek"),
            "the catalog's own name: {}",
            row.label
        );
        assert!(
            !row.current,
            "the dot marks the stored choice, not the effective one"
        );
    }

    #[test]
    fn an_id_the_catalog_does_not_list_is_shown_verbatim() {
        let rows = picker(ModeOverride::default(), &effective("gone_model", None)).rows;
        let row = rows
            .iter()
            .find(|row| matches!(row.pick, Some(Pick::Model(_))))
            .expect("the stale row");
        assert_eq!(row.label, "gone_model");
    }

    #[test]
    fn the_selection_starts_on_the_current_row_and_skips_headings() {
        let mut picker = picker(
            ModeOverride::default(),
            &effective("ant_claude", Some("high")),
        );
        assert_eq!(picker.current().unwrap().label, "Use default");
        assert_eq!(picker.pick(), Some(&Pick::Default(Group::Effort)));

        // Down through the efforts, over the second heading, onto the models.
        let mut labels = Vec::new();
        for _ in 0..6 {
            picker.move_by(1);
            labels.push(picker.current().unwrap().label.clone());
        }
        assert_eq!(
            labels,
            [
                "Low",
                "Medium",
                "High",
                "Use default",
                "OCG - GLM-5.3",
                "Claude"
            ]
        );
        picker.move_by(1);
        assert_eq!(
            picker.current().unwrap().label,
            "Claude",
            "clamped at the end"
        );
    }

    #[test]
    fn use_default_writes_nothing_and_a_model_writes_its_id() {
        let picker = picker(ModeOverride::default(), &effective("ocg_glm-5.3", None));
        let default = picker.pick().unwrap();
        assert_eq!(default.value(), None);
        assert_eq!(
            default.key(),
            ChoiceKey::Effort,
            "effort is the first group"
        );

        let pick = Pick::Model("ant_claude".into());
        assert_eq!(pick.value().as_deref(), Some("ant_claude"));
        assert_eq!(pick.key(), ChoiceKey::Model);
        let effort = Pick::Effort("high".into());
        assert_eq!(effort.key(), ChoiceKey::Effort);
    }

    #[test]
    fn the_drawn_lines_mark_selection_and_heading() {
        let picker = picker(ModeOverride::default(), &effective("ocg_glm-5.3", None));
        let lines = picker.lines();
        assert_eq!(lines[0].0, "Effort");
        assert!(lines[0].2, "a heading is flagged so the view can dim it");
        assert!(!lines[0].1, "the selection never starts on a heading");
        assert!(lines[1].1, "it starts on the row in force");
        assert!(
            lines[1].0.starts_with('\u{25cf}'),
            "the dot marks the stored choice"
        );
    }
}
