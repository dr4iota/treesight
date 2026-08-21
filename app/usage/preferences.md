# Preferences

Three settings, all of them flags in the header, all remembered per window.

- **Theme** — light, dark, or following the system, which is the default.
  Choosing one stores it and puts you back on the page you were reading, drawn
  again in the palette you chose.
- **Line numbers** — on or off, for highlighted source.
- **Pane** — show or hide the left half. Hidden, a file gets the whole window.

They are stored as cookies on the served origin, which is why they survive a
reload and a relaunch and do not travel to any other program.

## Printing

**Print** in the header, or `Ctrl+P`. The printed page is not the page on
screen: the header, the pane and the status line come off, and the light palette
is used whatever the window is showing, because a dark background is not what a
printer should be asked to do.

## Hidden files

Dotfiles are not listed. That is the server's setting rather than a flag on the
page — a folder full of `.git` internals is not what a tree is for.
