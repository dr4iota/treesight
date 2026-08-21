# Tree and pane

Two halves. The tree on the left is the folder you opened; the rest of the
window is whatever row you are on.

## The tree

Directories first, then names, case ignored. A directory has a disclosure arrow
that opens it in place, and which ones you left open is remembered — so the shape
of the tree survives a navigation and a reload. Files starting with a dot are not
listed.

Each directory row also carries a button that makes it the served root. It is the
one move you cannot otherwise make without the folder dialog and a path you
already have on screen.

The heading over the tree names the open folder and carries two acts on it: **Pin**
to keep it in the Pinned list, and **Close** to serve nothing and go back to the
start page. Opening a different folder is in the status line, which is always on
screen.

## The pane

On the start page it holds the lists — Places, Pinned, Recent, and any list the
program embedding this one adds. Once a folder is open it is the tree.

## The header

The path, and the controls that act on the window rather than on a file:

- **Back**, which is the window's own history — every view here is a page.
  Forward has no button and is `Alt+Right`.
- **Reload**, which re-reads the folder and keeps your place in it.
- **Close**, which serves nothing and shows the start page.
- **Print**, which prints the rendered view.

The path itself is a row of links: each component serves that folder.
