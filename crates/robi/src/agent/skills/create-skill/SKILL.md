---
name: Create skill
description: Interview the user and write a SKILL.md. Load this when the user wants to create or update a skill.
---

Write or update one skill directory. Do not write the file until the user confirms the draft.

1. If this conversation already contains the workflow, take the steps, the commands, and the corrections from it. Otherwise ask what the skill should do, when it should trigger, and what the result looks like.
2. Ask whether the model may load it on its own. The default is yes. A no sets `disable-model-invocation: true`.
3. Ask where it lives. The default is home, `~/.robi/skills/<id>/SKILL.md`. This workspace is `<workspace>/.robi/skills/<id>/SKILL.md`. The id is the directory name. It matches `^[a-z0-9]+(-[a-z0-9]+)*$` and is 1–64 characters.
4. Confirm the id, the description, and the body before writing. The description states what the skill does and the phrases that should trigger it. The body is the steps. Scripts, references, and assets are added only when the user asked for them.
5. Write `SKILL.md`. An id that already exists is left in place until the user agrees to replace it. Ask and plan cannot write files, so those modes show the draft and tell the user to switch to agent. A home path is outside the workspace, so the write pauses on the approval card the edit tools already use. A workspace path follows the same write rules as any other project file.

The reply names the path and says `/id` works on the next message. The scan for the current turn already ran.

Updating a skill is the same procedure. Read the existing `SKILL.md`, apply the change, and write it back after the user confirms.
