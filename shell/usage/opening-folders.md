# Opening folders

The app opens on the start page, which lists what you can open.

## Places

Standard folders from the system: your home directory, the desktop,
documents and downloads where the system has them, the drive letters on Windows,
and the filesystem root elsewhere. The same rows every launch. Nothing you open
is added here.

Each row is checked after the list appears. A greyed row says why:

- **gone** — it answered, and there is nothing there any more.
- **denied** — it is there and would not let you in.
- **N/A** — it did not answer: a drive that is not ready, or a share whose host
  is gone.

The row stays a link, so you can try it again.

On a phone there are no such folders to browse without asking. The row there
is **Files**, a folder of the app's own. Where a download asks where to save
it, nothing new lands in Files, and the row is listed only while it holds
something.

## Starred

What you chose to keep. **Star** is on the **Files** heading, over the tree. It
stars the folder that is open, not one you have walked into, and the folder gets
a row of its own, kept across launches. Press it again, or the cross on the row
(**Remove from Starred**), and the row goes.

To star a folder further down, start the tree there first — open it, then
**As root** in its header — and then star it.

A starred row that has gone missing greys and says so, and stays until you
remove it.

## Recent

What you have opened, newest first, kept across launches. Each row has a cross
(**Forget this folder**). A folder that is gone is dropped on the next launch,
not while the list is on screen.

## Open a folder…

The system's folder dialog, or `Ctrl+O`. The same control is in the status line
once a folder is open. What you choose is opened and remembered in Recent.

## Once something is open

The header shows the path, and each part of it opens that folder. In a folder
below the top, **As root** starts the tree at that folder, so you can open it
without the dialog. At the top it is greyed: that folder is the tree's already.

**Close** (the arrow leaving a frame, on the tree's heading, which is the
folder's name) returns to the start page. At the
top of the open folder the header button is **Start** and does the same thing.
Nothing asks you to confirm; the folder is in Recent. **Up** is the parent
folder.

## A window of its own

On a desktop, **New window** (beside As root, or Ctrl+N) opens the folder on
screen in a window of its own, a step down and right of the main one. On a
file's page it opens a window with that file alone: no sidebar, and a link in
it to a file beside it opens there, while a link to a folder goes nowhere.

Such a window keeps its folder for as long as it is open. It has no start
page, Places, Starred list or Recent, and nothing in it opens another folder.
At its top, **Main window** in Up's place brings the main window to the front.
It reads the folder the main window already has open, so nothing is opened
twice and nothing goes into Recent. Closing the main window closes it too.

Back, `Alt+Left` or the phone's gesture, walks through the folders you opened
and then leaves you on the start page. On a phone, one more press leaves the
app. Back does not go into a folder you left by opening another one, from
anywhere: it goes to the start page instead. On Linux that needs a recent
system WebKit (2.52 does it); with an older one, Back shows that folder as not
found, and Start in the header is the way out.
