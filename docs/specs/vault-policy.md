# Markdown Workspace Policy

The workspace contains user-owned Markdown. Log Inbox owns only explicitly reviewed managed blocks: Daily records and separately confirmed Product KB updates.

## Allowed writes

- Resolve the destination from the reviewed workspace profile and frozen day settings.
- Preview the exact old and new managed block before Apply.
- Create missing date-derived folders and a note from the frozen reviewed template, or a minimal note when no template is configured.
- Replace only the single valid Log Inbox block while preserving frontmatter, line endings, permissions, and all surrounding content.
- Journal the exact approved revision, destination, hashes, temporary identity, and recovery material before mutation.

## Forbidden behavior

- No raw log dumps, proposal inbox files, automatic Apply, arbitrary note edits, moves, renames, or deletes.
- No path traversal, symlink following, protected editor/Git locations, or destinations outside the inspected workspace.
- No model-selected destination or model-authorized link.
- No silent overwrite when the file or managed block differs from the reviewed preview.

Canonical product/engineering context is optional and must come from explicit workspace-scoped mappings or reviewed Knowledge collections. Repository branding alone never establishes a product identity.

Atomic replacement and optimistic hashes reduce races but cannot guarantee conflict-free writes against an unrelated editor changing the file at the same instant. Conflicts stay visible and require reconciliation.

## Explicitly reviewed Product KB blocks

The [accepted-work capture contract](knowledge-capture.md#canonical-update) additionally permits a distinct `log-inbox:knowledge:<id>` block in an owner-selected existing eligible Reference note. A separately accepted candidate and exact KB preview/confirmation under `vault:write` are required. The shared atomic writer and workspace capability preserve human content and Daily blocks; the KB journal freezes recovery bytes separately from Daily. KB operations cannot create notes/folders, move a promoted plan silently, or inherit Daily approval.
