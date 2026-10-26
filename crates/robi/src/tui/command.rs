//! The `/` table, and what a submitted line means.
//!
//! Two sigils, two kinds of name. `/` is a command: the terminal acts and the
//! field clears. `@` is a skill: the text is sent unchanged and the runtime loads
//! the skill from the mention. A leading token of either kind that names nothing
//! is refused with the reason, because the alternative — sending `/nope` to the
//! model as prose — is the terminal pretending a command it does not have ran.
//!
//! Every command is a keybinding's twin **or takes an argument no key could
//! carry**. They are not a second interface with powers of their own: a twin is
//! the discoverable spelling of a key a reader may not know yet, and the help view
//! prints the pair. The exception is `/attach`, `/detach` and `/open`, whose whole
//! value is the path — a binding has nowhere to put one, so `key` is empty and the
//! menu omits the column rather than printing a separator with nothing after it.

use crate::tui::state::completion::SkillEntry;

/// A command the terminal implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltIn {
    Models,
    Sessions,
    Workspaces,
    New,
    Rename,
    Mode,
    Compact,
    Help,
    Settings,
    Attach,
    Detach,
    Open,
}

/// How many arguments a command takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arg {
    /// Nothing. `/models` with a word after it is a mistake worth naming.
    None,
    /// Zero or one. `/rename` opens the prompt, `/rename Ship it` sets it.
    Optional,
    /// Exactly one. `/attach <path>`, `/open <path>`.
    Required,
}

/// One row of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub arg: Arg,
    /// The key that does the same thing, as the help view spells it. Empty for a
    /// command that exists only as a command, because what it needs is an
    /// argument rather than a keystroke.
    pub key: &'static str,
    /// One line, for the completion menu and the help view.
    pub about: &'static str,
    pub built_in: BuiltIn,
}

/// Every command, in the order the menu lists them.
///
/// A command exists because its surface does: `/settings` was withheld until T5
/// built the page, and adding it before that would have been a menu entry that
/// lies.
pub const BUILT_INS: &[CommandSpec] = &[
    CommandSpec {
        name: "models",
        arg: Arg::None,
        key: "Ctrl-M",
        about: "the model and effort picker",
        built_in: BuiltIn::Models,
    },
    CommandSpec {
        name: "sessions",
        arg: Arg::None,
        key: "Ctrl-S",
        about: "every chat session in this workspace",
        built_in: BuiltIn::Sessions,
    },
    CommandSpec {
        name: "workspaces",
        arg: Arg::None,
        key: "Ctrl-W",
        about: "switch workspace",
        built_in: BuiltIn::Workspaces,
    },
    CommandSpec {
        name: "new",
        arg: Arg::None,
        key: "Ctrl-N",
        about: "start a new chat",
        built_in: BuiltIn::New,
    },
    CommandSpec {
        name: "rename",
        arg: Arg::Optional,
        key: "Ctrl-R",
        about: "rename this chat",
        built_in: BuiltIn::Rename,
    },
    CommandSpec {
        name: "mode",
        arg: Arg::Optional,
        key: "Ctrl-T",
        about: "cycle the mode, or set ask, plan, or agent",
        built_in: BuiltIn::Mode,
    },
    CommandSpec {
        name: "compact",
        arg: Arg::None,
        key: "Ctrl-L",
        about: "summarize older messages",
        built_in: BuiltIn::Compact,
    },
    CommandSpec {
        name: "help",
        arg: Arg::None,
        key: "?",
        about: "every key and command",
        built_in: BuiltIn::Help,
    },
    CommandSpec {
        name: "settings",
        arg: Arg::None,
        key: "Ctrl-,",
        about: "every setting the app has",
        built_in: BuiltIn::Settings,
    },
    CommandSpec {
        name: "attach",
        arg: Arg::Required,
        key: "",
        about: "put a file or an image on this message",
        built_in: BuiltIn::Attach,
    },
    CommandSpec {
        name: "detach",
        arg: Arg::Optional,
        key: "",
        about: "take an attachment off, or all of them",
        built_in: BuiltIn::Detach,
    },
    CommandSpec {
        name: "open",
        arg: Arg::Required,
        key: "",
        about: "open a workspace file in your editor",
        built_in: BuiltIn::Open,
    },
];

/// The command `name` names, without its sigil.
pub fn find(name: &str) -> Option<&'static CommandSpec> {
    BUILT_INS.iter().find(|spec| spec.name == name)
}

/// A command that was asked for, with the argument it was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub built_in: BuiltIn,
    /// `Some` when the line carried more than the command's name.
    pub argument: Option<String>,
}

/// What a submitted line is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Prose, or a mention that names a real skill. Sent to the model as typed.
    Send,
    /// A command. The terminal acts, and the field clears.
    Run(Run),
    /// A name that does not exist. Nothing is sent; the text stays.
    Refuse(String),
}

/// Read the first token of `text` and decide what the whole line means.
///
/// Only the **first** token is checked. `either/or` mid-line, a path, and a
/// mention the domain cannot resolve are prose: the runtime already ignores an
/// unknown mention, and refusing a message because of what it says in the middle
/// would be the terminal reading over the user's shoulder.
pub fn verdict(text: &str, skills: &[SkillEntry]) -> Verdict {
    let trimmed = text.trim_start();
    // A command is a message, so only the first line's first word counts.
    let first_line = trimmed.lines().next().unwrap_or("");
    let token = first_line.split_whitespace().next().unwrap_or("");
    let rest = trimmed
        .strip_prefix(token)
        .map(str::trim)
        .filter(|rest| !rest.is_empty())
        .map(str::to_owned);

    if let Some(name) = token.strip_prefix('/') {
        if name.is_empty() {
            return Verdict::Refuse("type a command name after /".to_owned());
        }
        let Some(spec) = find(name) else {
            return Verdict::Refuse(if names_skill(skills, name) {
                format!("/{name} is a skill \u{2014} type @{name}")
            } else {
                format!("/{name} is not a command")
            });
        };
        return match (spec.arg, rest) {
            (Arg::None, Some(_)) => Verdict::Refuse(format!("/{} takes no argument", spec.name)),
            (Arg::Required, None) => Verdict::Refuse(format!("/{} needs an argument", spec.name)),
            (_, rest) => Verdict::Run(Run {
                built_in: spec.built_in,
                argument: rest,
            }),
        };
    }

    if let Some(name) = token.strip_prefix('@') {
        if name.is_empty() {
            return Verdict::Refuse("type a skill name after @".to_owned());
        }
        return if names_skill(skills, name) {
            Verdict::Send
        } else {
            Verdict::Refuse(format!("@{name} is not a skill"))
        };
    }

    Verdict::Send
}

fn names_skill(skills: &[SkillEntry], name: &str) -> bool {
    skills.iter().any(|skill| skill.id == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::state::completion::SkillScope;

    fn skill(id: &str) -> SkillEntry {
        SkillEntry {
            id: id.to_owned(),
            label: id.to_owned(),
            description: String::new(),
            scope: SkillScope::Project,
            manual: false,
        }
    }

    fn skills() -> Vec<SkillEntry> {
        vec![skill("create-skill"), skill("configure-mcp")]
    }

    fn run(text: &str) -> Run {
        match verdict(text, &skills()) {
            Verdict::Run(run) => run,
            other => panic!("{text:?} is not a command: {other:?}"),
        }
    }

    fn refusal(text: &str) -> String {
        match verdict(text, &skills()) {
            Verdict::Refuse(message) => message,
            other => panic!("{text:?} was accepted: {other:?}"),
        }
    }

    #[test]
    fn prose_is_sent_and_is_never_second_guessed() {
        for text in [
            "what does the parser do?",
            "either/or is a fine phrase",
            "someone@example.com",
            "see docs/src for /open",
            "a @mention of nothing",
            "",
        ] {
            assert_eq!(verdict(text, &skills()), Verdict::Send, "{text:?}");
        }
    }

    #[test]
    fn a_known_skill_mention_is_sent_unchanged() {
        assert_eq!(verdict("@create-skill", &skills()), Verdict::Send);
        // Mid-sentence too: the domain loads a mention wherever it finds one.
        assert_eq!(
            verdict("please @configure-mcp now", &skills()),
            Verdict::Send
        );
    }

    #[test]
    fn every_command_is_reachable_by_its_name() {
        for spec in BUILT_INS {
            // A command that requires an argument is written with one, because
            // the point here is reachability, not the refusal the next test
            // covers.
            let (line, expected) = match spec.arg {
                Arg::Required => (format!("/{} src/main.rs", spec.name), Some("src/main.rs")),
                Arg::None | Arg::Optional => (format!("/{}", spec.name), None),
            };
            let parsed = match verdict(&line, &skills()) {
                Verdict::Run(run) => run,
                other => panic!("{line} did not run: {other:?}"),
            };
            assert_eq!(parsed.built_in, spec.built_in);
            assert_eq!(parsed.argument.as_deref(), expected);
        }
    }

    #[test]
    fn a_required_argument_is_demanded_and_then_passed_through() {
        for name in ["attach", "open"] {
            assert_eq!(
                refusal(&format!("/{name}")),
                format!("/{name} needs an argument")
            );
            assert_eq!(
                run(&format!("/{name} src/app.rs:40-80"))
                    .argument
                    .as_deref(),
                Some("src/app.rs:40-80")
            );
        }
    }

    #[test]
    fn detach_takes_a_name_or_nothing_at_all() {
        // `Optional`, so both spellings are the command's: no name means every
        // attachment, and the empty argument reaches `run_command` as `None`.
        assert_eq!(run("/detach").argument, None);
        assert_eq!(run("/detach app.rs").argument.as_deref(), Some("app.rs"));
        assert_eq!(
            run("/detach a/mod.rs").argument.as_deref(),
            Some("a/mod.rs")
        );
    }

    #[test]
    fn a_command_with_no_keybinding_prints_no_separator() {
        // `/attach`, `/detach` and `/open` exist only as commands: they need a
        // path, and a keystroke has nowhere to put one.
        for spec in BUILT_INS {
            if matches!(
                spec.built_in,
                BuiltIn::Attach | BuiltIn::Detach | BuiltIn::Open
            ) {
                assert_eq!(spec.key, "", "/{} is not a keybinding's twin", spec.name);
            } else {
                assert!(!spec.key.is_empty(), "/{} has no key", spec.name);
            }
        }
    }

    #[test]
    fn an_optional_argument_is_passed_through() {
        assert_eq!(run("/rename Ship it").argument.as_deref(), Some("Ship it"));
        assert_eq!(run("/mode").argument, None);
        assert_eq!(run("/mode plan ").argument.as_deref(), Some("plan"));
    }

    #[test]
    fn a_command_that_takes_nothing_refuses_a_word() {
        assert_eq!(refusal("/models please"), "/models takes no argument");
    }

    #[test]
    fn an_unknown_command_is_named_without_being_sent() {
        assert_eq!(refusal("/nope"), "/nope is not a command");
    }

    #[test]
    fn a_skill_typed_with_the_wrong_sigil_says_which_one_to_use() {
        // The two front ends spell a mention differently, so this is the one
        // mistake a reader is most likely to make.
        assert_eq!(
            refusal("/create-skill"),
            "/create-skill is a skill \u{2014} type @create-skill"
        );
    }

    #[test]
    fn an_unknown_mention_is_named_without_being_sent() {
        assert_eq!(refusal("@nope"), "@nope is not a skill");
    }

    #[test]
    fn a_lone_sigil_says_what_is_missing() {
        assert_eq!(refusal("/"), "type a command name after /");
        assert_eq!(refusal("@"), "type a skill name after @");
    }

    #[test]
    fn a_sigil_mid_line_is_prose_not_a_command() {
        assert_eq!(verdict("what is /models", &skills()), Verdict::Send);
        assert_eq!(verdict("mail me @now", &skills()), Verdict::Send);
    }

    #[test]
    fn only_the_first_token_of_the_first_line_decides() {
        assert_eq!(
            verdict("/models\nand then some prose", &skills()),
            Verdict::Refuse("/models takes no argument".to_owned())
        );
        assert_eq!(
            verdict("prose\n/models", &skills()),
            Verdict::Send,
            "a command on a later line is not a command"
        );
    }
}
