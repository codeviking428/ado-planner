# Edit HTML fields as Markdown only when the round trip is lossless

GPUI has no rich-text editor, so the Rust app edits HTML fields as Markdown. Converting to Markdown and back drops @mentions, colors and merged table cells, and would silently rewrite text other people formatted in Azure DevOps. So the planner edits only **Plain HTML fields**: when the editor opens, it converts HTML to Markdown and back, normalizes both sides, and compares them. A field that doesn't match is shown read-only with an "Edit in Azure DevOps" link.

## Considered Options

- **Always convert and accept the loss.** Rejected: a typo fix would damage someone else's work without telling anyone.
- **Convert, but warn with a diff before saving.** Rejected: it asks the user to judge HTML loss they can't see.
- **Write a rich-text editor on gpui-kit's rope editor.** Deferred: too large for the cutover, and it remains the way to make every HTML field editable.
