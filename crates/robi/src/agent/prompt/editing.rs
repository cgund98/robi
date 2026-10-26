//! The shell hints that replace the read and edit tools.
//!
//! When `read-tools` and `edit-tools` are off, the shell is the only way to read
//! or change a file, so this block is the tool schema. Each line is reviewed
//! copy: the quoted heredoc and the fail-closed replace are the two that carry
//! correctness risk.
//!
//! Absent entirely when `read-tools` is on: the tools speak for themselves.

use robi_core::prompt::PromptBlock;

use super::PromptSource;

pub struct ShellEditing;

impl PromptSource for ShellEditing {
    fn load(&self) -> Option<PromptBlock> {
        #[cfg(feature = "read-tools")]
        {
            None
        }
        #[cfg(not(feature = "read-tools"))]
        {
            Some(PromptBlock {
                tag: Some("shell_editing"),
                body: BODY.to_owned(),
            })
        }
    }
}

// Unused when `read-tools` restores the tools: `load` returns `None` there.
#[cfg_attr(feature = "read-tools", allow(dead_code))]
const BODY: &str = "\
<shell_editing>
There are no read or edit tools in this build. The shell is how you read, search,
and change the workspace. These are the forms that work; prefer them over
inventing your own.

Read
- A whole small file: cat path
- A window: sed -n '40,80p' path (or sed -n '1,120p' path for the head)
- The end: tail -n 60 path. Count lines: wc -l path
- Never start a pager or an editor (less, more, vim, nano): the command would
  wait on stdin and time out.

Search
- Find a file by name: git ls-files | grep -i name, or
  find . -name '*name*' -not -path './target/*'
- Contents: grep -rn 'pattern' . --include='*.rs' (use rg when it is installed)
- Map a source file before reading it:
  grep -nE '^(pub )?(fn|struct|enum|impl|trait) ' path
- Narrow the path before you broaden the pattern. Output is capped; if a read was
  folded, retrieve the stream by its id instead of running the command again.

Write
- Full overwrite: cat > path <<'EOF'
  ...file text...
  EOF
  The delimiter must be quoted in that exact form. An unquoted EOF expands $VAR
  and backticks inside the text you are writing, which corrupts scripts and
  templates.
- One exact replacement: a short Python that reads, replaces once, and asserts:
  python3 - <<'EOF'
  import pathlib
  p = pathlib.Path('path')
  text = p.read_text()
  assert text.count('old text') == 1, text.count('old text')
  p.write_text(text.replace('old text', 'new text', 1))
  EOF
  The assert is the point: it fails closed instead of editing the wrong match.
- Append: printf '%s\\n' 'line' >> path
- Delete: ls path first, then rm path. Never rm -rf a path you have not listed.
- Create a directory: mkdir -p dir

Verify
- Read the change before you claim it: git diff -- path, git status --short
- If the file is untracked, git diff will show nothing: cat the file instead.
- A shell command that exits non-zero changed nothing you can rely on. Check
  exit_code, then read the diff.

Avoid
- sed -i. It is subtly different between GNU and BSD, and an unanchored pattern
  edits more than you intended. Use the Python replace above.
- Commands that wait on stdin, and interactive editors.
- Chaining a write and its verification with && into one call: you lose the exit
  code of the first when the second fails.
</shell_editing>";
