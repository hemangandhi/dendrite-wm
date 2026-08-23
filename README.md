# Dendrite

A [smithay](https://github.com/Smithay/smithay)-based WM inspired by [niri](https://github.com/niri-wm/niri).

The name is somewhat inspired by fractals and trees because I'm essentially taking the idea of niri and adding a tree structure, like [i3](https://i3wm.org/)'s.

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

(The above is definitely not a full specification, and barring some compromises, it, uh... works as implemented.)

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

## TODOs

The more active TODOs are, in order of complexity or expected difficulty:

1. [x] Debug Fuzzel's popup not working.
    1. Note: there might be some race condition with the keyboard focus.
2. [ ] Set up XWayland satellite.
    1. [x] Support XWayland satellite itself
    2. [ ] (Maybe?) Spawn XWayland satellite on startup.
3. [ ] Support resizing and moving windows around.
4. [ ] Handle workspaces.
5. [ ] Handle multiple outputs.

There's definitely stuff to do after this, but I haven't planned those yet.
I'm vaguely aware that I ought to:

1. Test more. (+ me actually using this for real)
2. Figure out an IPC. (Particularly since multi-output support will probably force me.)
3. Figure out some sort of config file.
    1. Also, if I do this, I should figure out what things I actually want to let people configure.
        1. Opactiy?
        2. Focus rings (like niri)?
    2. Also how much of what _I_ need is just "spawn at startup" kinda stuff.
4. Determine if I want tabbed or floating windows at all. (The point being: if I have this fractal tree thing, is there a reason to have tabs or floating windows.)
5. Implement that global search thingy?
