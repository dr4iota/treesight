# Opening folders

The window starts on a page that says what there is to open, and serves nothing
until you choose. Three ways to choose, and they are different in kind.

## Places

The platform's list, resolved when the program starts: your home directory, the
desktop, documents and downloads where the system has them, the drive letters
that exist on Windows, and the filesystem root elsewhere. The same rows every
launch — that is the point of it. Nothing you open is added here.

A row that is greyed and says *missing* or *not available* was asked about after
the page was drawn: a drive that is not ready, a share whose host has gone. It
stays a link, because the only way to find out whether it is back is to ask for
it, and clicking one costs whatever that wait costs.

Where a platform has no folders to browse outside the program's own storage, that
is the row you get, and it is the one row that is always readable.

## Recent

What you have opened, newest first, kept across launches. This one grows on its
own, and each row has a button to forget it. An entry whose folder has gone is
dropped from the list on the next launch rather than while you are looking at it —
a row vanishing from under the pointer is worse than one that says what is wrong
with it.

## Open a folder…

The system's own folder dialog, or `Ctrl+O`. Whatever it returns is served and
remembered in Recent.

## Once something is open

The header carries the path, and every directory row in the tree has a button
that serves *that* folder as the root — which is how you go down without going
through the dialog again. **Close** puts the window back on the start page; what
was open is in Recent, so there is nothing to confirm.
