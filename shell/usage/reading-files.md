# Reading files

Whatever a row turns out to be, the pane tries to show the thing rather than a
description of it.

- **Source** — syntax highlighted, with line numbers unless you turn them off.
- **Markdown** — rendered, including maths in `$…$` and `$$…$$`, and `mermaid`
  fenced blocks drawn as diagrams.
- **Images**, **audio** and **video** — shown and played in place.
- **PDF** — embedded, using whatever the platform's viewer is.
- **Anything else** — the bytes, as text where it is text.

## Rendered, raw, and the file itself

A rendered view has a flag in its header to show the **source** instead: the
Markdown behind the page, the text behind the highlighting. The flag toggles
back.

Beside it, **download** hands over the bytes as they are on disk.

The raw view keeps the page's chrome around the file — the path, the flags, a way
back — rather than replacing the window with the file. A file served as itself has
none of that, and in a window with no chrome of its own there would be no way out
of it but a keystroke nothing on screen suggests. Tools, and every image, video
and PDF the page embeds, get the file itself; only somebody looking at it gets
the frame.

## Big files

Highlighting is capped: past a size the file is served as plain text rather than
kept waiting for a colouring nobody asked for. Anything that streams — video,
audio, a large PDF — is read in ranges as the player asks for them, not read
whole first.
