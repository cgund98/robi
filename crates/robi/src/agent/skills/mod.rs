//! Skill directories and the catalog the prompt lists.
//!
//! A skill is a directory with `SKILL.md`. Home roots are scanned first, then
//! each directory from the git root down to the workspace. A later root
//! replaces the same id. The bundled skills are first, so a file of that id
//! replaces them.

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use robi_core::message::SkillLoad;

const ID_MAX: usize = 64;
const DESCRIPTION_MAX: usize = 1024;
const CATALOG_DESCRIPTION_MAX: usize = 256;
const CATALOG_MAX_BYTES: usize = 8 * 1024;
const BODY_MAX_BYTES: usize = 32 * 1024;
const FILE_LIMIT: usize = 10;

const CREATE_SKILL: &str = include_str!("create-skill/SKILL.md");
const CONFIGURE_MCP: &str = include_str!("configure-mcp/SKILL.md");

/// Where a skill was found. The menu uses this to say home or this workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillScope {
    User,
    Project,
}

/// One valid skill after precedence has been applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub id: String,
    pub label: String,
    pub description: String,
    pub directory: PathBuf,
    pub scope: SkillScope,
    pub model_invocable: bool,
    pub user_invocable: bool,
    pub body: String,
    pub files: Vec<String>,
    pub files_truncated: bool,
    rank: u32,
    depth: u32,
}

impl Skill {
    pub fn load(&self) -> SkillLoad {
        let (body, cut) = cap_body(&self.body);
        let mut body = body;
        if cut {
            body.push_str("\n\n[The tail of this skill was cut.]\n");
        }
        SkillLoad {
            id: self.id.clone(),
            description: self.description.clone(),
            directory: self.directory.display().to_string(),
            files: self.files.clone(),
            body,
        }
    }

    pub fn scope_name(&self) -> &'static str {
        match self.scope {
            SkillScope::User => "user",
            SkillScope::Project => "project",
        }
    }
}

/// Skills visible to this session, lowest precedence first.
///
/// `home` is `~/`. When it is absent the home roots are skipped. `workspace`
/// is the workspace root. No git root scans that directory only.
pub fn scan(home: Option<&Path>, workspace: Option<&Path>) -> Vec<Skill> {
    let mut rank = 0;
    let mut chosen: Vec<Skill> = Vec::new();
    for skill in bundled(&mut rank) {
        push_skill(&mut chosen, skill);
    }
    if let Some(home) = home {
        if home.is_dir() {
            for root in home_roots(home) {
                merge_root(&mut chosen, &scan_root(&root, SkillScope::User, &mut rank));
            }
        }
    }
    if let Some(workspace) = workspace {
        for dir in workspace_chain(workspace) {
            for root in project_roots(&dir) {
                merge_root(
                    &mut chosen,
                    &scan_root(&root, SkillScope::Project, &mut rank),
                );
            }
        }
    }
    chosen.sort_by(|left, right| {
        left.rank
            .cmp(&right.rank)
            .then(left.depth.cmp(&right.depth))
    });
    chosen
}

/// `<skills>` for skills the model may load. Empty when none qualify.
pub fn catalog_block(skills: &[Skill]) -> Option<String> {
    let mut lines: Vec<String> = skills
        .iter()
        .filter(|skill| skill.model_invocable)
        .map(|skill| {
            format!(
                "{}: {}",
                skill.id,
                cut_chars(&skill.description, CATALOG_DESCRIPTION_MAX)
            )
        })
        .collect();
    if lines.is_empty() {
        return None;
    }
    let mut omitted = 0;
    loop {
        let block = render_catalog(&lines, omitted);
        if block.len() <= CATALOG_MAX_BYTES || lines.is_empty() {
            if lines.is_empty() {
                return None;
            }
            return Some(block);
        }
        lines.remove(0);
        omitted += 1;
    }
}

/// `@id` tokens in `text`, in order, loaded once each.
///
/// An unknown word stays in the text and is not a load.
pub fn loads_for_text(text: &str, skills: &[Skill]) -> Vec<SkillLoad> {
    let mut loads = Vec::new();
    let mut seen = Vec::new();
    for id in mention_ids(text) {
        if seen.iter().any(|seen: &String| seen == &id) {
            continue;
        }
        let Some(skill) = skills.iter().find(|skill| skill.id == id) else {
            continue;
        };
        seen.push(id);
        loads.push(skill.load());
    }
    loads
}

/// Read allow for one skill directory, matched on the workspace-relative path.
pub fn read_allow(relative: &str) -> String {
    format!(r"^{}(/|$)", regex::escape(relative))
}

/// The provider block appended after the typed user text.
pub fn skill_block(load: &SkillLoad) -> String {
    let mut files = load.files.join(", ");
    if files.is_empty() {
        files = "(none)".to_owned();
    }
    format!(
        "\n<skill name=\"{}\" directory=\"{}\">\n{}\n\nFiles: {}\n</skill>",
        load.id,
        load.directory,
        load.body.trim_end(),
        files
    )
}

pub fn mention_ids(text: &str) -> Vec<String> {
    let pattern = Regex::new(r"(^|\s)@([a-z0-9]+(?:-[a-z0-9]+)*)").expect("mention pattern");
    pattern
        .captures_iter(text)
        .filter_map(|caps| caps.get(2).map(|id| id.as_str().to_owned()))
        .filter(|id| id.len() <= ID_MAX)
        .collect()
}

fn render_catalog(lines: &[String], omitted: usize) -> String {
    let mut body = lines.join("\n");
    if omitted > 0 {
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(&format!("[{omitted} skills omitted]"));
    }
    format!("<skills>\n{body}\n</skills>")
}

fn bundled(rank: &mut u32) -> Vec<Skill> {
    let mut skills = Vec::new();
    for (id, text) in [
        ("create-skill", CREATE_SKILL),
        ("configure-mcp", CONFIGURE_MCP),
    ] {
        *rank += 1;
        skills.push(
            parse_skill(id, PathBuf::new(), text, SkillScope::User, *rank, 0)
                .expect("bundled skill is valid"),
        );
    }
    skills
}

fn home_roots(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".claude/skills"),
        home.join(".codex/skills"),
        home.join(".cursor/skills"),
        home.join(".opencode/skills"),
        home.join(".config/opencode/skills"),
        home.join(".agents/skills"),
        home.join(".robi/skills"),
    ]
}

fn project_roots(dir: &Path) -> Vec<PathBuf> {
    vec![
        dir.join(".claude/skills"),
        dir.join(".codex/skills"),
        dir.join(".cursor/skills"),
        dir.join(".opencode/skills"),
        dir.join(".agents/skills"),
        dir.join(".robi/skills"),
    ]
}

fn workspace_chain(workspace: &Path) -> Vec<PathBuf> {
    let root = git_root(workspace);
    let Ok(relative) = workspace.strip_prefix(&root) else {
        return vec![workspace.to_path_buf()];
    };
    if relative.as_os_str().is_empty() {
        return vec![root];
    }
    let mut dirs = vec![root.clone()];
    let mut current = root;
    for part in relative.components() {
        current.push(part);
        dirs.push(current.clone());
    }
    dirs
}

fn git_root(dir: &Path) -> PathBuf {
    let mut current = dir.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return current;
        }
        if !current.pop() {
            return dir.to_path_buf();
        }
    }
}

fn merge_root(chosen: &mut Vec<Skill>, found: &[Skill]) {
    for skill in found {
        if let Some(existing) = chosen.iter_mut().find(|existing| existing.id == skill.id) {
            *existing = skill.clone();
        } else {
            chosen.push(skill.clone());
        }
    }
}

fn scan_root(root: &Path, scope: SkillScope, rank: &mut u32) -> Vec<Skill> {
    *rank += 1;
    let rank = *rank;
    if !root.is_dir() {
        return Vec::new();
    }
    let mut found = Vec::new();
    walk(root, root, 0, scope, rank, &mut found);
    let mut kept: Vec<Skill> = Vec::new();
    for skill in found {
        if let Some(existing) = kept.iter_mut().find(|existing| existing.id == skill.id) {
            if skill.depth < existing.depth {
                tracing::warn!(
                    id = %skill.id,
                    root = %root.display(),
                    "skipped a deeper skill with the same id"
                );
                *existing = skill;
            } else {
                tracing::warn!(
                    id = %skill.id,
                    root = %root.display(),
                    "skipped a skill because a shallower path has the same id"
                );
            }
        } else {
            kept.push(skill);
        }
    }
    kept
}

fn walk(root: &Path, dir: &Path, depth: u32, scope: SkillScope, rank: u32, found: &mut Vec<Skill>) {
    let skill_md = dir.join("SKILL.md");
    if skill_md.is_file() {
        match fs::read_to_string(&skill_md) {
            Ok(text) => {
                let id = dir
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("")
                    .to_owned();
                match parse_skill(&id, dir.to_path_buf(), &text, scope, rank, depth) {
                    Some(skill) => found.push(skill),
                    None => tracing::warn!(path = %skill_md.display(), "skipped a skill"),
                }
            }
            Err(err) => tracing::warn!(path = %skill_md.display(), %err, "skipped a skill"),
        }
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
            continue;
        }
        let symlink = entry.file_type().is_ok_and(|kind| kind.is_symlink());
        if symlink && depth > 0 {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, depth + 1, scope, rank, found);
        }
    }
    let _ = root;
}

fn parse_skill(
    id: &str,
    directory: PathBuf,
    text: &str,
    scope: SkillScope,
    rank: u32,
    depth: u32,
) -> Option<Skill> {
    if !valid_id(id) {
        return None;
    }
    let (front, body) = split_frontmatter(text)?;
    let meta = parse_frontmatter(front);
    let description = meta.description.or_else(|| first_paragraph(body))?;
    let description = cut_chars(&description, DESCRIPTION_MAX);
    if description.is_empty() {
        return None;
    }
    let (files, files_truncated) = if directory.as_os_str().is_empty() {
        (Vec::new(), false)
    } else {
        list_files(&directory)
    };
    Some(Skill {
        id: id.to_owned(),
        label: meta.name.unwrap_or_else(|| id.to_owned()),
        description,
        directory,
        scope,
        model_invocable: !meta.disable_model && !meta.autoinvoke_false,
        user_invocable: meta.user_invocable,
        body: body.trim().to_owned(),
        files,
        files_truncated,
        rank,
        depth,
    })
}

fn valid_id(id: &str) -> bool {
    let pattern = Regex::new(r"^[a-z0-9]+(-[a-z0-9]+)*$").expect("id pattern");
    !id.is_empty() && id.len() <= ID_MAX && pattern.is_match(id)
}

fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let rest = text
        .strip_prefix("---\r\n")
        .or_else(|| text.strip_prefix("---\n"))?;
    let (front, body) = rest.split_once("\n---")?;
    let body = body
        .strip_prefix("\r\n")
        .or_else(|| body.strip_prefix('\n'))
        .unwrap_or(body);
    Some((front, body))
}

struct Front {
    description: Option<String>,
    name: Option<String>,
    disable_model: bool,
    autoinvoke_false: bool,
    user_invocable: bool,
}

fn parse_frontmatter(front: &str) -> Front {
    let mut meta = Front {
        description: None,
        name: None,
        disable_model: false,
        autoinvoke_false: false,
        user_invocable: true,
    };
    let mut section: Vec<String> = Vec::new();
    for raw in front.lines() {
        if raw.trim().is_empty() || raw.trim_start().starts_with('#') {
            continue;
        }
        let indent = raw.chars().take_while(|ch| *ch == ' ').count() / 2;
        section.truncate(indent);
        let Some((key, value)) = raw.trim().split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = unquote(value.trim());
        if value.is_empty() {
            section.push(key.to_owned());
            continue;
        }
        let path = section_key(&section, key);
        match path.as_str() {
            "description" => meta.description = Some(value),
            "name" => meta.name = Some(value),
            "disable-model-invocation" => meta.disable_model = value == "true",
            "user-invocable" => meta.user_invocable = value != "false",
            "metadata.opencode/autoinvoke" | "metadata.opencode.autoinvoke" => {
                meta.autoinvoke_false = value == "false";
            }
            _ => {}
        }
    }
    meta
}

fn section_key(section: &[String], key: &str) -> String {
    if section.is_empty() {
        return key.to_owned();
    }
    format!("{}.{}", section.join("."), key)
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        return value[1..value.len() - 1].to_owned();
    }
    value.to_owned()
}

fn first_paragraph(body: &str) -> Option<String> {
    let mut lines = Vec::new();
    for line in body.lines() {
        if line.trim().is_empty() {
            if !lines.is_empty() {
                break;
            }
            continue;
        }
        lines.push(line.trim());
    }
    let text = lines.join(" ");
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn list_files(dir: &Path) -> (Vec<String>, bool) {
    let Ok(root) = dir.canonicalize() else {
        return (Vec::new(), false);
    };
    let mut files = Vec::new();
    collect_files(&root, &root, &mut files);
    files.sort();
    let truncated = files.len() > FILE_LIMIT;
    files.truncate(FILE_LIMIT);
    (files, truncated)
}

fn collect_files(root: &Path, dir: &Path, files: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(canonical) = path.canonicalize() else {
            continue;
        };
        if !canonical.starts_with(root) {
            continue;
        }
        if canonical.is_dir() {
            collect_files(root, &canonical, files);
            continue;
        }
        if canonical.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
            continue;
        }
        if let Ok(relative) = canonical.strip_prefix(root) {
            files.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn cap_body(body: &str) -> (String, bool) {
    if body.len() <= BODY_MAX_BYTES {
        return (body.to_owned(), false);
    }
    let mut end = BODY_MAX_BYTES;
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    (body[..end].to_owned(), true)
}

fn cut_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    text.chars().take(max).collect()
}

fn push_skill(chosen: &mut Vec<Skill>, skill: Skill) {
    chosen.push(skill);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("robi-skills-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_skill(dir: &Path, id: &str, front: &str, body: &str) {
        let skill = dir.join(id);
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            format!("---\n{front}\n---\n{body}\n"),
        )
        .unwrap();
    }

    #[test]
    fn workspace_overrides_home_and_robi_overrides_agents() {
        let home = temp("home");
        let repo = temp("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        write_skill(
            &home.join(".claude/skills"),
            "git-release",
            "description: from claude",
            "claude",
        );
        write_skill(
            &home.join(".agents/skills"),
            "git-release",
            "description: from agents",
            "agents",
        );
        write_skill(
            &home.join(".robi/skills"),
            "git-release",
            "description: from home robi",
            "home",
        );
        write_skill(
            &repo.join(".agents/skills"),
            "git-release",
            "description: from project agents",
            "project agents",
        );
        write_skill(
            &repo.join(".robi/skills"),
            "git-release",
            "description: from project",
            "project",
        );

        let skills = scan(Some(&home), Some(&repo));
        let skill = skills
            .iter()
            .find(|skill| skill.id == "git-release")
            .unwrap();
        assert_eq!(skill.body, "project");
        assert_eq!(skill.scope, SkillScope::Project);

        let _ = fs::remove_dir_all(home);
        let _ = fs::remove_dir_all(repo);
    }

    #[test]
    fn claude_loses_to_agents_in_the_same_directory() {
        let repo = temp("order");
        write_skill(
            &repo.join(".claude/skills"),
            "review",
            "description: claude",
            "claude",
        );
        write_skill(
            &repo.join(".agents/skills"),
            "review",
            "description: agents",
            "agents",
        );
        let skills = scan(None, Some(&repo));
        let skill = skills.iter().find(|skill| skill.id == "review").unwrap();
        assert_eq!(skill.body, "agents");
        let _ = fs::remove_dir_all(repo);
    }

    #[test]
    fn a_manual_only_skill_is_left_out_of_the_catalog_and_still_loads() {
        let repo = temp("manual");
        write_skill(
            &repo.join(".robi/skills"),
            "ship-it",
            "description: Ship the release\ndisable-model-invocation: true",
            "steps",
        );
        let skills = scan(None, Some(&repo));
        let block = catalog_block(&skills).unwrap();
        assert!(!block.contains("ship-it"));
        assert!(block.contains("create-skill"));
        let loads = loads_for_text("please @ship-it now", &skills);
        assert_eq!(loads.len(), 1);
        assert_eq!(loads[0].id, "ship-it");
        assert_eq!(loads[0].body, "steps");
        let _ = fs::remove_dir_all(repo);
    }

    #[test]
    fn a_hidden_menu_skill_stays_in_the_catalog() {
        let repo = temp("hidden");
        write_skill(
            &repo.join(".robi/skills"),
            "internal-review",
            "description: Review privately\nuser-invocable: false",
            "steps",
        );
        let skills = scan(None, Some(&repo));
        let skill = skills
            .iter()
            .find(|skill| skill.id == "internal-review")
            .unwrap();
        assert!(!skill.user_invocable);
        assert!(skill.model_invocable);
        assert!(catalog_block(&skills).unwrap().contains("internal-review"));
        let _ = fs::remove_dir_all(repo);
    }

    #[test]
    fn an_unknown_mention_is_not_a_load() {
        let skills = scan(None, None);
        assert!(loads_for_text("see @not-a-skill", &skills).is_empty());
        assert_eq!(mention_ids("see @not-a-skill").len(), 1);
    }

    #[test]
    fn a_file_replaces_the_bundled_create_skill() {
        let home = temp("bundle");
        write_skill(
            &home.join(".robi/skills"),
            "create-skill",
            "description: local creator",
            "local steps",
        );
        let skills = scan(Some(&home), None);
        let skill = skills
            .iter()
            .find(|skill| skill.id == "create-skill")
            .unwrap();
        assert_eq!(skill.body, "local steps");
        assert_ne!(skill.directory, PathBuf::new());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn the_shallower_path_wins_inside_one_root() {
        let repo = temp("shallow");
        write_skill(
            &repo.join(".agents/skills"),
            "land-it",
            "description: shallow",
            "shallow",
        );
        write_skill(
            &repo.join(".agents/skills/shipping"),
            "land-it",
            "description: deep",
            "deep",
        );
        let skills = scan(None, Some(&repo));
        let skill = skills.iter().find(|skill| skill.id == "land-it").unwrap();
        assert_eq!(skill.body, "shallow");
        let _ = fs::remove_dir_all(repo);
    }
}
