# Reading files

Select a file to see it beside the tree:

- **Source** — syntax highlighted, with line numbers unless you turn them off.
- **Markdown** — rendered, including maths in `$…$` and `$$…$$`, and `mermaid`
  blocks drawn as diagrams.
- **CSV and TSV** — a table, with the heading row kept in view as you scroll;
  the first 5000 rows are shown.
- **JSON** — laid out, and JSON Lines a record at a time. A file that does not
  parse is shown as written.
- **HTML** — the page drawn, with its scripts off.
- **A `.gz` file** — unpacked, and shown as what its name says under the
  `.gz`: `syslog.2.gz` as text, `data.csv.gz` as a table. A packed picture,
  recording or PDF is left packed.
- **Images**, **audio** and **video** — shown and played in place.
- **PDF** — the platform's viewer, or the app's own where it has one.
- **Anything else that is text** — as text.
- **Anything else** — what kind of file it is, where its first bytes say
  (a program, an archive, a database…), and those bytes in hex.

## Preview, Source, Raw, and Download

The header shows how the file is shown as one set of buttons — **Preview**
(an eye), **Source** (`</>`) and **Raw** (a sheet) — with the one on screen
lit. **Preview** is the file as this app draws it, **Source** the highlighted
text, and **Raw** the file as it is, as text, with the header still shown, so
you can get back. An HTML file's Raw is the HTML as written.

A picture, film, sound or PDF has **Preview**, the file alone, and **Source**:
its size, date and kind — a picture's size in pixels, a PDF's version — then
the first 4 KiB of it in hex, eight bytes to a line on a phone and sixteen on
a wider window. Any other file that is not text has **Source** alone. None of
these, nor a `.gz` file, has Raw: there is no showing them as they are.

## Full screen

**Full screen**, beside **Refresh**, shows the file alone: no header, no
sidebar, no status line, and the window full screen on its monitor — on a
phone, the status and navigation bars hidden too. It is on the view a file
opens in: **Preview**, or **Source** where a file has no Preview. It stays on
from one file to the next, and ends when you open a folder.

A tap shows, for a moment, the way out at the top right and the file's name,
size and date at the top left, and a tap while they show hides them; a tap on
a link follows the link. On a computer, moving the mouse shows it too.
Scrolling and zooming stay in full screen, and so does a link to a heading in
the same file: Back then returns to where you were, still in full screen. To
leave, tap the way out, or press **Esc** or **F11**, or Back on a phone.

What can be done with the file — **Print**, **Download**, and any action the
app adds — is a second set. **Download** saves a copy as stored. A file can
also be saved without opening it, from its row in the folder: see below.

While a copy is saved, the status line at the bottom says how far it has got,
with **Cancel**.

## A folder's rows

Each row shows its name and size and when it was last modified, with how
long ago; on a narrow screen the time goes under the name. The first row,
**.. (to folder …)**, names the folder above and goes up to it. At a row's
right end are its buttons:

- **Download** (the downward arrow), on a file, saves a copy, as above.
- **Delete** (the bin), on a computer's own folders, moves the file or folder
  to the Trash (the Recycle Bin on Windows), after asking. Where the Trash
  will not take it, such as a network drive, a file or an empty folder can
  still be deleted for good, asked again in those words; a folder with things
  in it is never deleted that way.

The app may add its own: a line under the row with words to tap, and a
bookmark on the row's icon that switches something on and off — tapping the
icon toggles it.

**New folder** turns the filter box into a box for the new folder's name;
**Cancel** brings the filter back.

## Big files

A text file over 2 MB is too big to highlight: it opens in **Raw**, with
**Source** dimmed. A big archive or other file that is not text opens in
**Source**, which reads only its first bytes. Video, audio, and large PDFs play without being read all at
once.
