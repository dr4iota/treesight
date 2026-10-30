# AGENTS.md

Guidance for AI agents working in this repository.
`CLAUDE.md` and `GEMINI.md` are symlinks to this file.

## Commit messages

Keep them short. A log is scanned, not read.

- **Subject: one line, 50 characters or fewer.** Imperative, capitalised, no
  trailing period. `git log --oneline` is the view that matters.
- **Most commits are subject-only.** If the subject says it, stop there.
- **Write a body only when the diff cannot explain itself** — a non-obvious
  *why*, a constraint that forced the approach, a consequence a reader would
  not predict. Never a summary of what changed; the diff already is that.
- **Cap a body at three lines.** Blank line after the subject, wrap at 72.
- **One subject, one change.** If the message needs paragraphs or a list to
  cover everything, split the commit instead.
- Design rationale, alternatives weighed, and known gaps belong in
  `docs/architecture.md` and `docs/todo.md`, not in the log. Details buried in
  a long message are details nobody reads, and they bury the one line that
  mattered.

Not this — a real commit here, 22 lines of body under a 62-character subject:

    Serve the tree through a Vfs seam a downstream shell can extend

    [five paragraphs on the seam, its callers, and what it does not yet cover]

This — the same work, as the commits it always was:

    Add a Vfs trait behind the tree reader
    Route the local backend through Vfs
    Note the Vfs seam in docs/architecture.md

## Icons

- **Icons have one source: `src/icons.rs`.** Every mark is defined there once
  and drawn with `svg_icon`; a page, a script or another constant takes it
  from there (an embedder through `icons::ALL`). Never paste an SVG path.
