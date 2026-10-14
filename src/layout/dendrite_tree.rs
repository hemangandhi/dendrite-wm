use smithay::utils::{Logical, Point, Rectangle, Size};

use crate::layout::action::{Action, Direction};
use crate::render::RenderableElement;

#[derive(Default, Debug)]
pub struct FocusSuggestion(Vec<usize>);

impl From<FocusSuggestion> for Vec<usize> {
    fn from(mut value: FocusSuggestion) -> Self {
        value.0.reverse();
        return value.0;
    }
}

impl PartialEq<Vec<usize>> for FocusSuggestion {
    fn eq(&self, other: &Vec<usize>) -> bool {
        self.0.len() == other.len()
            && self
                .0
                .iter()
                .enumerate()
                .all(|(i, x)| *x == other[other.len() - i - 1])
    }
}

impl FocusSuggestion {
    fn push(&mut self, i: usize) {
        self.0.push(i);
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Orientation {
    Vertical,
    Horizontal,
}

impl Orientation {
    pub fn perpendicular(self) -> Self {
        match self {
            Orientation::Vertical => Orientation::Horizontal,
            Orientation::Horizontal => Orientation::Vertical,
        }
    }
}

pub enum DendriteTree<W> {
    Empty(Size<i32, Logical>),
    Leaf {
        window: W,
        // Always in the output frame (so far...)
        geometry: Rectangle<i32, Logical>,
    },
    Container {
        children: Vec<DendriteTree<W>>,
        // Always in the output frame (so far...)
        geometry: Rectangle<i32, Logical>,
        orientation: Orientation,
        is_tabbed: bool,
        last_focus: usize,
    },
}

impl<W> Default for DendriteTree<W> {
    fn default() -> Self {
        Self::Empty(Size::default())
    }
}

impl<W> From<Size<i32, Logical>> for DendriteTree<W> {
    fn from(value: Size<i32, Logical>) -> Self {
        Self::Empty(value)
    }
}

// TODO: reduce out params?
fn delete_child_and_suggest_focus<W>(
    children: &mut Vec<DendriteTree<W>>,
    index_to_remove: usize,
    new_focus: &mut Vec<usize>,
    last_focus: &mut usize,
) -> bool {
    children.remove(index_to_remove);
    if children.is_empty() {
        return true;
    }
    let index = if index_to_remove != 0 {
        index_to_remove - 1
    } else {
        0
    };
    if *last_focus == index_to_remove {
        *last_focus = index;
    }
    new_focus.push(index);
    children[index].deepen_focus_from_last(new_focus);
    return false;
}

fn scroll_window_into_view<W>(
    new_window_geometry: Rectangle<i32, Logical>,
    orientation: Orientation,
    parent_geometry: Rectangle<i32, Logical>,
    children: &mut [DendriteTree<W>],
) {
    let bump = match orientation {
        Orientation::Horizontal => {
            if new_window_geometry.loc.x < parent_geometry.loc.x {
                Point::new(parent_geometry.loc.x - new_window_geometry.loc.x, 0)
            } else if parent_geometry.loc.x + parent_geometry.size.w
                < new_window_geometry.loc.x + new_window_geometry.size.w
            {
                Point::new(
                    (parent_geometry.loc.x + parent_geometry.size.w)
                        - (new_window_geometry.loc.x + new_window_geometry.size.w),
                    0,
                )
            } else {
                return;
            }
        }
        Orientation::Vertical => {
            if new_window_geometry.loc.y < parent_geometry.loc.y {
                Point::new(0, parent_geometry.loc.y - new_window_geometry.loc.y)
            } else if parent_geometry.loc.y + parent_geometry.size.h
                < new_window_geometry.loc.y + new_window_geometry.size.h
            {
                Point::new(
                    0,
                    (parent_geometry.loc.y + parent_geometry.size.h)
                        - (new_window_geometry.loc.y + new_window_geometry.size.h),
                )
            } else {
                return;
            }
        }
    };
    for child in children {
        child.update_position(bump);
    }
}

// To be called after children[i] was removed.
fn offset_for_removed<W>(
    children: &mut [DendriteTree<W>],
    i: usize,
    orientation: Orientation,
    removed_geometry: Rectangle<i32, Logical>,
) {
    // If there's only 1 window, sliding it doesn't help.
    if i == children.len() && i < 2 {
        return;
    }
    let offset_direction = if i == children.len() { 1 } else { -1 };
    let offset = match orientation {
        Orientation::Vertical => Point::new(0, offset_direction * removed_geometry.size.h),
        Orientation::Horizontal => Point::new(offset_direction * removed_geometry.size.w, 0),
    };
    if i == children.len() {
        for child in children {
            child.update_position(offset);
        }
    } else {
        for child in children.iter_mut().skip(i) {
            child.update_position(offset);
        }
    }
}

fn deepen_focus_from_last_container<'b, W>(
    mut child: &'b mut DendriteTree<W>,
    focus: &mut Vec<usize>,
    focus_on_deepest_container: bool,
) -> &'b mut DendriteTree<W> {
    loop {
        // This should be a while-let and Polonius might do it
        // But I want to avoid flags and NLL would borrow stuff for too long.
        if let DendriteTree::Container {
            children,
            orientation,
            geometry,
            last_focus,
            ..
        } = &mut *child
        {
            if *last_focus >= children.len() {
                tracing::warn!("Resetting out-of-bounds focus {last_focus} under {focus:?}");
                *last_focus = 0;
            }
            scroll_window_into_view(
                children[*last_focus].geometry(),
                *orientation,
                *geometry,
                children,
            );

            // Breaking here apparently trips up the borrow-checker.
            // children would be borrowed for too long since it'd
            // still be borrowed when we return and
            // it'd be borrowed while assigning child = children[last_focus]
            if focus_on_deepest_container
                && matches!(children[*last_focus], DendriteTree::Leaf { .. })
            {
                break;
            }
        } else {
            break;
        }

        if let DendriteTree::Container {
            children,
            last_focus,
            ..
        } = child
        {
            child = &mut children[*last_focus];
            focus.push(*last_focus);
        }
    }

    child
}

fn add_toplevel_into_container<W: RenderableElement>(
    is_tabbed: bool,
    mut toplevel: DendriteTree<W>,
    children: &mut Vec<DendriteTree<W>>,
    geometry: Rectangle<i32, Logical>,
    orientation: Orientation,
    focus_index: Option<usize>,
) {
    if is_tabbed {
        toplevel.set_geometry(geometry);
        children.insert(focus_index.unwrap_or(children.len()), toplevel);
        return;
    }

    let new_window_size = match orientation {
        Orientation::Vertical => Size::new(geometry.size.w, geometry.size.h / 2),
        Orientation::Horizontal => Size::new(geometry.size.w / 2, geometry.size.h),
    };

    let Some(x) = focus_index else {
        toplevel.set_geometry(Rectangle {
            loc: geometry.loc,
            size: new_window_size,
        });
        children.push(toplevel);
        return;
    };

    let nearby_child_index = if x == 0 { 0 } else { x - 1 };

    let new_geometry = Rectangle::new(
        match orientation {
            Orientation::Vertical => Point::new(
                geometry.loc.x,
                children[nearby_child_index].geometry().loc.y
                    + (if x == 0 {
                        0
                    } else {
                        children[nearby_child_index].geometry().size.h
                    }),
            ),
            Orientation::Horizontal => Point::new(
                children[nearby_child_index].geometry().loc.x
                    + (if x == 0 {
                        0
                    } else {
                        children[nearby_child_index].geometry().size.w
                    }),
                geometry.loc.y,
            ),
        },
        new_window_size,
    );
    toplevel.set_geometry(new_geometry);
    children.insert(x, toplevel);
    scroll_window_into_view(new_geometry, orientation, geometry, &mut children[..=x]);
    for further_window in &mut children[(x + 1)..] {
        further_window.update_position(match orientation {
            Orientation::Vertical => Point::new(0, new_window_size.h),
            Orientation::Horizontal => Point::new(new_window_size.w, 0),
        });
    }
}

impl<W> DendriteTree<W> {
    fn update_position(&mut self, delta: Point<i32, Logical>) {
        match self {
            DendriteTree::Leaf { geometry, .. } => geometry.loc += delta,
            DendriteTree::Container {
                geometry, children, ..
            } => {
                for child in children {
                    child.update_position(delta);
                }
                geometry.loc += delta
            }
            DendriteTree::Empty(_) => {}
        }
    }

    fn geometry(&self) -> Rectangle<i32, Logical> {
        match self {
            DendriteTree::Leaf { geometry, .. } => *geometry,
            DendriteTree::Container { geometry, .. } => *geometry,
            DendriteTree::Empty(s) => Rectangle {
                loc: Point::new(0, 0),
                size: *s,
            },
        }
    }

    pub fn window_at_path<'a>(&'a self, path: &[usize]) -> Option<&'a W> {
        match (self, path) {
            (DendriteTree::Leaf { window, .. }, []) => Some(window),
            (DendriteTree::Leaf { .. }, _x) => None,
            (DendriteTree::Container { children, .. }, [x]) => {
                children.get(*x).and_then(move |t| t.window_at_path(&[]))
            }
            (DendriteTree::Container { children, .. }, [x, xs @ ..]) => {
                children.get(*x).and_then(move |t| t.window_at_path(xs))
            }
            (DendriteTree::Container { .. }, []) => None,
            (DendriteTree::Empty(..), _) => None,
        }
    }

    pub fn resize_output(&mut self, new_size: Size<i32, Logical>) {
        match self {
            DendriteTree::Leaf { geometry, .. } => geometry.size = new_size,
            // TODO: scale children?
            DendriteTree::Container { geometry, .. } => geometry.size = new_size,
            DendriteTree::Empty(s) => *s = new_size,
        }
    }

    pub fn apply_focus(&mut self, mut focus: &[usize]) {
        let mut node = self;
        while let (
            DendriteTree::Container {
                children,
                last_focus,
                ..
            },
            [i, new_focus @ ..],
        ) = (node, focus)
        {
            *last_focus = *i;
            node = &mut children[*i];
            focus = new_focus;
        }
    }

    pub fn deepen_focus_from_last(&mut self, focus: &mut Vec<usize>) {
        deepen_focus_from_last_container(self, focus, false);
    }
}

impl<W: RenderableElement> DendriteTree<W> {
    fn set_geometry(&mut self, new_geometry: Rectangle<i32, Logical>) {
        match self {
            DendriteTree::Leaf { geometry, window } => {
                *geometry = new_geometry;
                window.set_size(new_geometry.size);
            }
            DendriteTree::Container { geometry, .. } => *geometry = new_geometry,
            DendriteTree::Empty(s) => *s = new_geometry.size,
        };
    }

    fn render_to_space<'a>(
        &self,
        active_window: Option<&[usize]>,
        render_data: &mut W::Renderer<'a>,
        parent_geometry: Rectangle<i32, Logical>,
        layer_num: u8,
    ) {
        let geometry = self.geometry();
        match self {
            DendriteTree::Leaf { window, .. } => {
                if !parent_geometry.overlaps_or_touches(geometry) {
                    window.unmap(render_data);
                } else {
                    // Note: active windows will never actually have something atop them.
                    window.render_or_map(
                        render_data,
                        geometry.loc,
                        active_window.is_some(),
                        30 - layer_num,
                    );
                }
            }
            DendriteTree::Container { children, .. } => {
                for (i, child) in children.iter().enumerate() {
                    child.render_to_space(
                        active_window.and_then(|a| match a {
                            [] => Some(&[] as &[usize]),
                            [x, xs @ ..] if *x == i => Some(xs),
                            _ => None,
                        }),
                        render_data,
                        geometry,
                        layer_num + 1,
                    );
                    // TODO: we may have to raise any windows at our level since the space behaves that way.
                }
            }
            _ => {}
        }
    }

    pub fn render_to_space_root<'a>(
        &self,
        active_window: Option<&[usize]>,
        render_data: &mut W::Renderer<'a>,
    ) {
        self.render_to_space(active_window, render_data, self.geometry(), 0);
    }

    pub fn new_toplevel(&mut self, surface: W::TopLevelSurfaceType, focus: &[usize]) {
        if let DendriteTree::Empty(s) = self {
            *self = DendriteTree::Container {
                children: vec![],
                geometry: Rectangle {
                    loc: Point::new(0, 0),
                    size: *s,
                },
                orientation: if s.w <= s.h {
                    Orientation::Vertical
                } else {
                    Orientation::Horizontal
                },
                is_tabbed: false,
                last_focus: 0,
            }
        }
        let DendriteTree::Container {
            children,
            orientation,
            geometry,
            is_tabbed,
            ..
        } = self
        else {
            tracing::warn!("new_toplevel called on a leaf!");
            return;
        };

        if let [x, xs @ ..] = focus
            && !xs.is_empty()
        {
            children[*x].new_toplevel(surface, xs);
            return;
        }

        add_toplevel_into_container(
            *is_tabbed,
            DendriteTree::Leaf {
                window: W::from_toplevel(geometry.size, surface),
                geometry: *geometry,
            },
            children,
            *geometry,
            *orientation,
            focus.first().map(|x| x + 1),
        );
    }

    pub fn path_to_window<'a>(&'a self, window: &W) -> Option<(FocusSuggestion, &'a W)> {
        window.to_surface().and_then(|s| self.path_to_surface(&s))
    }

    pub fn path_to_surface<'a>(
        &'a self,
        surface: &W::SurfaceType,
    ) -> Option<(FocusSuggestion, &'a W)> {
        match self {
            DendriteTree::Leaf {
                window: real_window,
                ..
            } => {
                return if real_window.contains_surface(surface) {
                    Some((FocusSuggestion::default(), real_window))
                } else {
                    None
                };
            }
            DendriteTree::Container { children, .. } => {
                for (i, child) in children.iter().enumerate() {
                    if let Some((mut path, real_window)) = child.path_to_surface(surface) {
                        path.push(i);
                        return Some((path, real_window));
                    }
                }
                return None;
            }
            DendriteTree::Empty(..) => return None,
        };
    }

    fn send_close(&self) {
        match self {
            DendriteTree::Leaf { window, .. } => {
                window.send_close();
            }
            DendriteTree::Container { children, .. } => {
                for c in children {
                    c.send_close()
                }
            }
            _ => {}
        }
    }

    fn toplevel_destroyed_helper(&mut self, focus: &mut Vec<usize>, index: usize) -> bool {
        tracing::info!("Destroying {focus:?}");
        let DendriteTree::Container {
            children,
            orientation,
            is_tabbed,
            last_focus,
            ..
        } = self
        else {
            tracing::warn!("toplevel_destroyed called on a leaf!");
            return false;
        };

        if focus.is_empty() {
            tracing::warn!("no toplevel is focussed for destruction");
            return false;
        }

        if index < focus.len() - 1 {
            let child_index = focus[index];
            let delete_child = children[child_index].toplevel_destroyed_helper(focus, index + 1);
            if !delete_child {
                return false;
            }
            focus.truncate(index);
            return delete_child_and_suggest_focus(children, child_index, focus, last_focus);
        }

        let Some(i) = focus.pop() else {
            tracing::warn!("toplevel_destroyed called with empty path?");
            return false;
        };

        children[i].send_close();
        let removed_geometry = children[i].geometry();

        let new_focus = delete_child_and_suggest_focus(children, i, focus, last_focus);
        if *is_tabbed {
            return new_focus;
        }

        offset_for_removed(children, i, *orientation, removed_geometry);
        return new_focus;
    }

    pub fn toplevel_destroyed(&mut self, focus: &mut Vec<usize>) -> bool {
        self.toplevel_destroyed_helper(focus, 0)
    }

    fn handle_move_focus(
        &mut self,
        focus: &mut Vec<usize>,
        index: usize,
        mut direction: Direction,
    ) -> Option<Direction> {
        let DendriteTree::Container {
            children,
            orientation,
            geometry,
            last_focus,
            ..
        } = self
        else {
            tracing::warn!(
                "Invalid move of focus on leaf or empty! Focus {focus:?}, index {index:?}"
            );
            return None;
        };

        if index >= focus.len() {
            tracing::warn!("Focus {focus:?} doesn't have index {index:?}");
            return None;
        }

        let mut moved_up = false;
        if index < focus.len() - 1 {
            let Some(residual_action) =
                children[focus[index]].handle_move_focus(focus, index + 1, direction)
            else {
                return None;
            };
            direction = residual_action;
            moved_up = true;
        }

        let child_index_offset: isize = match (direction, *orientation) {
            (Direction::Up | Direction::Down, Orientation::Horizontal) => {
                return Some(direction);
            }
            (Direction::Up, Orientation::Vertical) => -1,
            (Direction::Down, Orientation::Vertical) => 1,
            (Direction::Left | Direction::Right, Orientation::Vertical) => {
                return Some(direction);
            }
            (Direction::Left, Orientation::Horizontal) => -1,
            (Direction::Right, Orientation::Horizontal) => 1,
        };

        let new_child_index = (focus[index] as isize) + child_index_offset;
        if new_child_index < 0 || new_child_index >= (children.len() as isize) {
            return Some(direction);
        }

        if moved_up {
            focus.truncate(index + 1);
        }

        focus[index] = new_child_index as usize;
        *last_focus = new_child_index as usize;

        scroll_window_into_view(
            children[new_child_index as usize].geometry(),
            *orientation,
            *geometry,
            children,
        );
        children[new_child_index as usize].deepen_focus_from_last(focus);
        return None;
    }

    fn handle_move_node(
        &mut self,
        focus: &mut Vec<usize>,
        index: usize,
        mut direction: Direction,
    ) -> Option<(Direction, Self)> {
        let DendriteTree::Container {
            is_tabbed,
            children,
            orientation,
            geometry,
            last_focus,
            ..
        } = self
        else {
            tracing::warn!(
                "Invalid move of node on leaf or empty! Focus {focus:?}, index {index:?}"
            );
            return None;
        };

        if index >= focus.len() {
            tracing::warn!("Focus {focus:?} doesn't have index {index:?}");
            return None;
        }

        let mut moved_up = false;
        let migrant = if index < focus.len() - 1 {
            let Some((residual_action, inner_migrant)) =
                children[focus[index]].handle_move_node(focus, index + 1, direction)
            else {
                return None;
            };
            direction = residual_action;
            moved_up = true;
            inner_migrant
        } else {
            let removed = children.remove(focus[index]);
            if *last_focus == index {
                if index == 0 {
                    *last_focus = 0;
                } else {
                    *last_focus = index - 1;
                }
            }
            offset_for_removed(children, focus[index], *orientation, removed.geometry());
            removed
        };

        let child_index_offset: isize = match (direction, *orientation) {
            (Direction::Up | Direction::Down, Orientation::Horizontal) => {
                return Some((direction, migrant));
            }
            (Direction::Up, Orientation::Vertical) => -1,
            (Direction::Down, Orientation::Vertical) => 1,
            (Direction::Left | Direction::Right, Orientation::Vertical) => {
                return Some((direction, migrant));
            }
            (Direction::Left, Orientation::Horizontal) => -1,
            (Direction::Right, Orientation::Horizontal) => 1,
        };

        let (children, geometry, orientation, is_tabbed, last_focus, extend_focus) = if moved_up {
            focus.truncate(index + 1);
            (
                children,
                geometry,
                orientation,
                is_tabbed,
                last_focus,
                false,
            )
        } else if matches!(
            children.get(focus.last().cloned().unwrap_or(0).max(children.len() - 1)),
            Some(&DendriteTree::Container::<W> { .. })
        ) {
            let DendriteTree::Container {
                children: new_children,
                geometry: new_geometry,
                orientation: new_orientation,
                is_tabbed: new_is_tabbed,
                last_focus: new_last_focus,
            } = deepen_focus_from_last_container(&mut children[focus[index]], focus, true)
            else {
                tracing::warn!(
                    "Got non-container out of deepening focus to {focus:?} from level {index}."
                );
                return None;
            };
            (
                new_children,
                new_geometry,
                new_orientation,
                new_is_tabbed,
                new_last_focus,
                true,
            )
        } else {
            (
                children,
                geometry,
                orientation,
                is_tabbed,
                last_focus,
                false,
            )
        };

        let mut new_child_index = (*focus.last().unwrap() as isize) + child_index_offset;
        if new_child_index < 0 {
            new_child_index = 0;
        } else if new_child_index > (children.len() as isize) {
            new_child_index = children.len() as isize;
        }

        *last_focus = new_child_index as usize;
        if extend_focus {
            focus.push(new_child_index as usize);
        } else {
            focus[index] = new_child_index as usize;
        }

        add_toplevel_into_container(
            *is_tabbed,
            migrant,
            children,
            *geometry,
            *orientation,
            Some(new_child_index as usize),
        );
        return None;
    }

    fn make_inner_tree(&mut self, focus: &mut Vec<usize>, index: usize) {
        let DendriteTree::Container {
            children,
            orientation,
            ..
        } = self
        else {
            tracing::warn!("Making inner tree reached a leaf!");
            return;
        };

        if focus.is_empty() {
            tracing::warn!("Empty focus while making an inner tree!");
            return;
        }

        if index < focus.len() - 1 {
            children[focus[index]].make_inner_tree(focus, index + 1);
            return;
        }

        let i = focus[index];
        let mut new_tree = DendriteTree::Container {
            children: vec![],
            geometry: children[i].geometry(),
            orientation: orientation.perpendicular(),
            is_tabbed: false,
            last_focus: 0,
        };
        std::mem::swap(&mut new_tree, &mut children[i]);
        let DendriteTree::Container {
            children: new_children,
            ..
        } = &mut children[i]
        else {
            panic!("The tree that was swapped in wasn't an inner node?");
        };
        new_children.push(new_tree);

        focus.push(0);
    }

    pub fn handle_action(&mut self, focus: &mut Vec<usize>, action: Action) -> Option<Action> {
        tracing::info!("Handle {action:?} at {focus:?}");
        match action {
            Action::MoveFocus(d @ Direction::Up)
            | Action::MoveFocus(d @ Direction::Down)
            | Action::MoveFocus(d @ Direction::Left)
            | Action::MoveFocus(d @ Direction::Right) => self
                .handle_move_focus(focus, 0, d)
                .map(|d| Action::MoveFocus(d)),
            Action::MoveNode(d @ Direction::Up)
            | Action::MoveNode(d @ Direction::Down)
            | Action::MoveNode(d @ Direction::Left)
            | Action::MoveNode(d @ Direction::Right) => self
                .handle_move_node(focus, 0, d)
                .map(|(d, _w)| Action::MoveNode(d)),
            Action::MakeInnerTree => {
                self.make_inner_tree(focus, 0);
                None
            }
            Action::CloseWindow => {
                let _ = self.toplevel_destroyed(focus);
                None
            }
            // TODO: make the types make this impossible
            Action::Spawn => Some(Action::Spawn),
        }
    }
}

#[cfg(test)]
mod test {
    use super::DendriteTree;
    use crate::render::test_render::TestRenderElement;

    #[test]
    fn test_new_toplevel() {
        let mut tree = DendriteTree::<TestRenderElement>::default();
        let focus = vec![];
        let win = TestRenderElement::with_id(1);
        tree.new_toplevel(win, focus.as_ref());
        let DendriteTree::Container { children, .. } = tree else {
            assert!(false);
            return;
        };
        assert_eq!(children.len(), 1);
        let DendriteTree::Leaf { window, .. } = &children[0] else {
            assert!(false);
            return;
        };
        assert_eq!(window.id, 1);
    }

    mod one_layer {
        use super::super::DendriteTree;
        use crate::layout::action::Direction;
        use crate::render::test_render::{RenderRecord, TestRenderElement};
        use smithay::utils::{Point, Size};

        #[test]
        fn test_many_spawns() {
            let mut tree = DendriteTree::<TestRenderElement>::default();
            tree.resize_output(Size::new(256, 256));
            let focus = vec![];
            for i in 1..=5 {
                tree.new_toplevel(TestRenderElement::with_id(i), &focus);
            }
            let DendriteTree::Container { children, .. } = tree else {
                assert!(false);
                return;
            };
            assert_eq!(children.len(), 5);
            assert!(children.iter().enumerate().all(|(i, w)| match w {
                DendriteTree::Leaf { window, .. } =>
                    window.id == ((i + 1) as u32) && window.size == Size::new(256, 128),
                _ => false,
            }));
        }

        #[test]
        fn test_move_focus() {
            let mut tree = DendriteTree::<TestRenderElement>::default();
            tree.resize_output(Size::new(256, 256));
            let mut focus = vec![];
            for i in 1..=5 {
                tree.new_toplevel(TestRenderElement::with_id(i), &focus);
            }
            // Needs focus to actually be set.
            focus.push(0);
            for i in 1..5 {
                tree.handle_move_focus(&mut focus, 0, Direction::Down);
                assert_eq!(focus, vec![i]);
            }
        }

        #[test]
        fn test_get_path() {
            let mut tree = DendriteTree::<TestRenderElement>::default();
            tree.resize_output(Size::new(256, 256));
            let focus = vec![];
            for i in 1..=5 {
                tree.new_toplevel(TestRenderElement::with_id(i), &focus);
            }
            for i in 1..=5u32 {
                let pw = tree.path_to_window(&TestRenderElement::with_id(i));
                assert!(pw.is_some_and(|(p, w)| p == vec![(i as usize) - 1] && w.id == i));
            }
        }

        #[test]
        fn test_render() {
            let mut tree = DendriteTree::<TestRenderElement>::default();
            tree.resize_output(Size::new(256, 256));
            let mut focus = vec![];
            for i in 1..=5 {
                tree.new_toplevel(TestRenderElement::with_id(i), &focus);
                // Push focus right
                if focus.is_empty() {
                    focus.push(0);
                } else {
                    tree.handle_move_focus(&mut focus, 0, Direction::Down);
                    assert_eq!(focus, vec![(i - 1) as usize]);
                }
            }
            for i in 1..5 {
                tree.handle_move_focus(&mut focus, 0, Direction::Up);
                assert_eq!(focus, vec![(4 - i) as usize]);
            }
            let mut renders: Vec<Vec<RenderRecord>> = vec![];
            for i in 1..5 {
                tree.render_to_space_root(Some(&focus), renders.push_mut(vec![]));
                tree.handle_move_focus(&mut focus, 0, Direction::Down);
                assert_eq!(focus, vec![i]);
            }
            let expected_renders = vec![
                vec![
                    RenderRecord::Rendered {
                        id: 1,
                        coords: Point::new(0, 0),
                        active: true,
                        z_index: 29,
                    },
                    RenderRecord::Rendered {
                        id: 2,
                        coords: Point::new(0, 128),
                        active: false,
                        z_index: 29,
                    },
                    RenderRecord::Rendered {
                        id: 3,
                        coords: Point::new(0, 256),
                        active: false,
                        z_index: 29,
                    },
                    RenderRecord::Unmapped(4),
                    RenderRecord::Unmapped(5),
                ],
                vec![
                    RenderRecord::Rendered {
                        id: 1,
                        coords: Point::new(0, 0),
                        active: false,
                        z_index: 29,
                    },
                    RenderRecord::Rendered {
                        id: 2,
                        coords: Point::new(0, 128),
                        active: true,
                        z_index: 29,
                    },
                    RenderRecord::Rendered {
                        id: 3,
                        coords: Point::new(0, 256),
                        active: false,
                        z_index: 29,
                    },
                    RenderRecord::Unmapped(4),
                    RenderRecord::Unmapped(5),
                ],
                vec![
                    RenderRecord::Rendered {
                        id: 1,
                        coords: Point::new(0, -128),
                        active: false,
                        z_index: 29,
                    },
                    RenderRecord::Rendered {
                        id: 2,
                        coords: Point::new(0, 0),
                        active: false,
                        z_index: 29,
                    },
                    RenderRecord::Rendered {
                        id: 3,
                        coords: Point::new(0, 128),
                        active: true,
                        z_index: 29,
                    },
                    RenderRecord::Rendered {
                        id: 4,
                        coords: Point::new(0, 256),
                        active: false,
                        z_index: 29,
                    },
                    RenderRecord::Unmapped(5),
                ],
                vec![
                    RenderRecord::Unmapped(1),
                    RenderRecord::Rendered {
                        id: 2,
                        coords: Point::new(0, -128),
                        active: false,
                        z_index: 29,
                    },
                    RenderRecord::Rendered {
                        id: 3,
                        coords: Point::new(0, 0),
                        active: false,
                        z_index: 29,
                    },
                    RenderRecord::Rendered {
                        id: 4,
                        coords: Point::new(0, 128),
                        active: true,
                        z_index: 29,
                    },
                    RenderRecord::Rendered {
                        id: 5,
                        coords: Point::new(0, 256),
                        active: false,
                        z_index: 29,
                    },
                ],
            ];
            assert_eq!(expected_renders, renders);
        }
    }

    use crate::layout::action::Action;
    use crate::render::test_render::RenderRecord;
    use smithay::utils::Size;

    enum TestAction<'a> {
        Act(Action),
        Spawn(u32),
        Render(&'a mut Vec<RenderRecord>),
    }

    fn run_actions_from_state(
        actions: &mut [TestAction],
        tree: &mut DendriteTree<TestRenderElement>,
        mut focus: &mut Vec<usize>,
    ) {
        for action in actions {
            match action {
                TestAction::Act(a) => {
                    tree.handle_action(&mut focus, *a);
                }
                TestAction::Spawn(i) => {
                    tree.new_toplevel(TestRenderElement::with_id(*i), &focus);
                    *focus = tree
                        .path_to_surface(&TestRenderElement::with_id(*i))
                        .unwrap()
                        .0
                        .into()
                }
                TestAction::Render(r) => {
                    tree.render_to_space_root(Some(&focus), r);
                }
            };
        }
    }

    fn run_actions(actions: &mut [TestAction]) -> (DendriteTree<TestRenderElement>, Vec<usize>) {
        let mut tree = DendriteTree::<TestRenderElement>::default();
        tree.resize_output(Size::new(256, 256));
        let mut focus = vec![];
        run_actions_from_state(actions, &mut tree, &mut focus);
        (tree, focus)
    }

    mod many_layers {
        use super::*;
        use crate::layout::action::Direction;

        use smithay::utils::Point;

        #[test]
        fn test_tree_spawn() {
            let mut renders = vec![];
            let (_tree, focus) = run_actions(&mut [
                TestAction::Spawn(0),
                TestAction::Spawn(1),
                TestAction::Act(Action::MakeInnerTree),
                TestAction::Spawn(2),
                TestAction::Act(Action::MakeInnerTree),
                TestAction::Spawn(3),
                TestAction::Spawn(4),
                TestAction::Render(renders.push_mut(vec![])),
            ]);
            assert_eq!(focus, vec![1, 1, 2]);
            let expected_renders = vec![vec![
                RenderRecord::Rendered {
                    id: 0,
                    coords: Point::new(0, 0),
                    active: false,
                    z_index: 29,
                },
                RenderRecord::Rendered {
                    id: 1,
                    coords: Point::new(-128, 128),
                    active: false,
                    z_index: 28,
                },
                RenderRecord::Rendered {
                    id: 2,
                    coords: Point::new(128, 0),
                    active: false,
                    z_index: 27,
                },
                RenderRecord::Rendered {
                    id: 3,
                    coords: Point::new(128, 128),
                    active: false,
                    z_index: 27,
                },
                RenderRecord::Rendered {
                    id: 4,
                    coords: Point::new(128, 128 + 64),
                    active: true,
                    z_index: 27,
                },
            ]];
            assert_eq!(renders, expected_renders);
        }

        #[test]
        fn test_tree_kills() {
            let (mut tree, mut focus) = run_actions(&mut [
                TestAction::Spawn(0),
                TestAction::Spawn(1),
                TestAction::Act(Action::MakeInnerTree),
                TestAction::Spawn(2),
                TestAction::Act(Action::MakeInnerTree),
                TestAction::Spawn(3),
                TestAction::Spawn(4),
                TestAction::Act(Action::MoveFocus(Direction::Left)),
            ]);
            assert_eq!(focus, vec![1, 0]);
            let mut rendered = vec![];
            run_actions_from_state(
                &mut [
                    TestAction::Act(Action::CloseWindow),
                    TestAction::Render(&mut rendered),
                ],
                &mut tree,
                &mut focus,
            );
            assert_eq!(focus, vec![1, 0, 0]);
            let expected_render = vec![
                RenderRecord::Rendered {
                    id: 0,
                    coords: Point::new(0, 0),
                    active: false,
                    z_index: 29,
                },
                RenderRecord::Rendered {
                    id: 2,
                    coords: Point::new(0, 128),
                    active: true,
                    z_index: 27,
                },
                RenderRecord::Rendered {
                    id: 3,
                    coords: Point::new(0, 256),
                    active: false,
                    z_index: 27,
                },
                RenderRecord::Unmapped(4),
            ];
            assert_eq!(rendered, expected_render);
            run_actions_from_state(
                &mut [
                    TestAction::Act(Action::CloseWindow),
                    TestAction::Act(Action::CloseWindow),
                    TestAction::Act(Action::CloseWindow),
                ],
                &mut tree,
                &mut focus,
            );
            assert_eq!(focus, vec![0]);
        }
    }

    mod move_node {
        use super::*;
        use crate::layout::action::Direction;
        use smithay::utils::Point;

        #[test]
        fn test_simple_move() {
            let mut render_before = vec![];
            let mut render_after = vec![];
            let (_tree, focus) = run_actions(&mut [
                TestAction::Spawn(0),
                TestAction::Spawn(1),
                TestAction::Render(&mut render_before),
                TestAction::Act(Action::MoveNode(Direction::Up)),
                TestAction::Render(&mut render_after),
            ]);

            assert_eq!(focus, vec![0]);
            assert_eq!(
                render_before,
                vec![
                    RenderRecord::Rendered {
                        id: 0,
                        coords: Point::new(0, 0),
                        active: false,
                        z_index: 29,
                    },
                    RenderRecord::Rendered {
                        id: 1,
                        coords: Point::new(0, 128),
                        active: true,
                        z_index: 29,
                    },
                ]
            );
            assert_eq!(focus, vec![0]);
            assert_eq!(
                render_after,
                vec![
                    RenderRecord::Rendered {
                        id: 1,
                        coords: Point::new(0, 0),
                        active: true,
                        z_index: 29,
                    },
                    RenderRecord::Rendered {
                        id: 0,
                        coords: Point::new(0, 128),
                        active: false,
                        z_index: 29,
                    },
                ]
            );
        }

        #[test]
        fn test_move_up() {
            let mut render_before = vec![];
            let mut render_after = vec![];
            let (_tree, focus) = run_actions(&mut [
                TestAction::Spawn(0),
                TestAction::Spawn(1),
                TestAction::Act(Action::MakeInnerTree),
                TestAction::Spawn(2),
                TestAction::Act(Action::MakeInnerTree),
                TestAction::Spawn(3),
                TestAction::Spawn(4),
                TestAction::Render(&mut render_before),
                TestAction::Act(Action::MoveNode(Direction::Right)),
                TestAction::Render(&mut render_after),
            ]);

            assert_eq!(focus, vec![1, 2]);
            assert_eq!(
                render_before,
                vec![
                    RenderRecord::Rendered {
                        id: 0,
                        coords: Point::new(0, 0),
                        active: false,
                        z_index: 29,
                    },
                    RenderRecord::Rendered {
                        id: 1,
                        coords: Point::new(-128, 128),
                        active: false,
                        z_index: 28,
                    },
                    RenderRecord::Rendered {
                        id: 2,
                        coords: Point::new(128, 0),
                        active: false,
                        z_index: 27,
                    },
                    RenderRecord::Rendered {
                        id: 3,
                        coords: Point::new(128, 128),
                        active: false,
                        z_index: 27,
                    },
                    RenderRecord::Rendered {
                        id: 4,
                        coords: Point::new(128, 128 + 64),
                        active: true,
                        z_index: 27,
                    },
                ]
            );
            assert_eq!(
                render_after,
                vec![
                    RenderRecord::Rendered {
                        id: 0,
                        coords: Point::new(0, 0),
                        active: false,
                        z_index: 29,
                    },
                    RenderRecord::Rendered {
                        id: 1,
                        coords: Point::new(-256, 128),
                        active: false,
                        z_index: 28,
                    },
                    RenderRecord::Rendered {
                        id: 2,
                        coords: Point::new(0, 64),
                        active: false,
                        z_index: 27,
                    },
                    RenderRecord::Rendered {
                        id: 3,
                        coords: Point::new(0, 192),
                        active: false,
                        z_index: 27,
                    },
                    RenderRecord::Rendered {
                        id: 4,
                        coords: Point::new(128, 128),
                        active: true,
                        z_index: 28,
                    },
                ]
            );
        }

        #[test]
        fn test_move_down() {
            let mut render_before = vec![];
            let mut render_after = vec![];
            let (_tree, focus) = run_actions(&mut [
                TestAction::Spawn(0),
                TestAction::Spawn(1),
                TestAction::Act(Action::MakeInnerTree),
                TestAction::Spawn(2),
                TestAction::Act(Action::MoveFocus(Direction::Up)),
                TestAction::Render(&mut render_before),
                TestAction::Act(Action::MoveNode(Direction::Down)),
                TestAction::Render(&mut render_after),
            ]);

            assert_eq!(focus, vec![0, 1]);
            assert_eq!(
                render_before,
                vec![
                    RenderRecord::Rendered {
                        id: 0,
                        coords: Point::new(0, 0),
                        active: true,
                        z_index: 29,
                    },
                    RenderRecord::Rendered {
                        id: 1,
                        coords: Point::new(-128, 128),
                        active: false,
                        z_index: 28,
                    },
                    RenderRecord::Rendered {
                        id: 2,
                        coords: Point::new(128, 128),
                        active: false,
                        z_index: 28,
                    },
                ]
            );
            assert_eq!(
                render_after,
                vec![
                    RenderRecord::Rendered {
                        id: 1,
                        coords: Point::new(-128, 0),
                        active: false,
                        z_index: 28,
                    },
                    RenderRecord::Rendered {
                        id: 0,
                        coords: Point::new(128, 0),
                        active: true,
                        z_index: 28,
                    },
                    RenderRecord::Unmapped(2),
                ]
            );
        }

        #[test]
        fn test_move_node_then_focus() {
            let mut render_before = vec![];
            let mut render_after = vec![];
            let (_tree, focus) = run_actions(&mut [
                TestAction::Spawn(0),
                TestAction::Spawn(1),
                TestAction::Act(Action::MakeInnerTree),
                TestAction::Spawn(2),
                TestAction::Act(Action::MakeInnerTree),
                TestAction::Spawn(3),
                TestAction::Spawn(4),
                TestAction::Render(&mut render_before),
                TestAction::Act(Action::MoveNode(Direction::Right)),
                TestAction::Render(&mut render_after),
                TestAction::Act(Action::MoveFocus(Direction::Left)),
            ]);
            assert_eq!(focus, [1, 1, 0]);
        }
    }
}
