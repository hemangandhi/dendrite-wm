# Dendrite

A [smithay](https://github.com/Smithay/smithay)-based WM inspired by [niri](https://github.com/niri-wm/niri).

The name is somewhat inspired by fractals and trees because I'm essentially taking the idea of niri and adding a tree structure, like i3's.

For now, I'm going to waffle on about these dreams, but eventually this should be some brief user guide.

## The premise

- Workspaces are bound to outputs.
- Inside the workspace, the top left is the "root".

```rust
enum Orientation { Horizontal, Vertical }

enum Arrangement {
    Tiled,
    Tabbed,
    Floating { x: i32, y: i32 }
}

enum Tree{
    Container{
        orientation: Orientation,
        arrangement: Arrangement,
        children: Vec<Tree>
    },
    Leaf(Window /* from like Smithay; but with a size */)
}
```

- There would be a command to select the parent and move around that layer.
- Movement of windows and focus would be in x and y, with a bias for the top left.
- The sizing will be relative to the output's size.
- Containers will have a frame with the same size as a sibling window.
- Child windows that don't fit into their containers scroll off on the relevant axis.

## Side-quests

Other than the above, I do:

1. Want to see if there's any way to make Japanese input easier.
2. Have some search feature for the windows.
3. Have some IPC that's a little... nuts.

### The search

I want to have a search that looks like Fuzzel but with property-based filters like
those of [Helix's pickers](https://docs.helix-editor.com/pickers.html).

### The nutty IPC

This is crazier than usual, since I really just want a list of trees as my entire window manager's state,
I'm wondering if the IPC can literally be a FUSE mount:

1. A root directory will have the list of workspaces.
2. Each workspace directory represents the root of its tree.
3. Each container node (`Tree::Container` in the premise above) will be a directory with:
    1. Subdirectories for each inner container.
    2. A file for each leaf (`.crystal`, persay).
    3. A file specifying the container's property (`.geode`).
4. All nodes would have a unique ID to keep the files sensible

Then:

- File-system interactions will either be rejected if invalid or actually just manipulate the windows to match the new file structure.
- Adding the file system into version control could make a WM-level undo or something... possible?
