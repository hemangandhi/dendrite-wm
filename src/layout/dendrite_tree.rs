use smithay::desktop::Window;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Rectangle, Size};
use smithay::wayland::seat::WaylandFocus;
use smithay::wayland::shell::xdg::ToplevelSurface;

use crate::layout::action::Action;
use crate::render::{RenderData, RenderableElement};

#[derive(Default)]
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

    fn singleton(x: usize) -> Self {
        Self(vec![x])
    }
}

#[derive(Copy, Clone, PartialEq)]
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
    },
}

impl<W> Default for DendriteTree<W> {
    fn default() -> Self {
        Self::Container {
            children: vec![],
            geometry: Default::default(),
            orientation: Orientation::Horizontal,
            is_tabbed: false,
        }
    }
}

impl<W> From<Size<i32, Logical>> for DendriteTree<W> {
    fn from(value: Size<i32, Logical>) -> Self {
        Self::Container {
            children: vec![],
            geometry: Rectangle {
                loc: Point::new(0, 0),
                size: value,
            },
            orientation: if value.w >= value.h {
                Orientation::Horizontal
            } else {
                Orientation::Vertical
            },
            is_tabbed: false,
        }
    }
}

fn delete_child_and_suggest_focus<W>(
    children: &mut Vec<DendriteTree<W>>,
    i: usize,
) -> (FocusSuggestion, bool) {
    children.remove(i);
    if children.is_empty() {
        (FocusSuggestion::default(), true)
    } else if i != 0 {
        (FocusSuggestion::singleton(i - 1), false)
    } else {
        (FocusSuggestion::singleton(0), false)
    }
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

impl<W> DendriteTree<W> {
    fn update_position(&mut self, delta: Point<i32, Logical>) {
        match self {
            DendriteTree::Leaf { geometry, .. } => geometry.loc += delta,
            DendriteTree::Container { geometry, .. } => geometry.loc += delta,
        }
    }

    fn geometry(&self) -> Rectangle<i32, Logical> {
        match self {
            DendriteTree::Leaf { geometry, .. } => *geometry,
            DendriteTree::Container { geometry, .. } => *geometry,
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
        }
    }
}

impl<W: RenderableElement> DendriteTree<W> {
    fn render_to_space(
        &self,
        active_window: Option<&[usize]>,
        render_data: &mut RenderData,
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
        }
    }

    pub fn render_to_space_root(
        &self,
        active_window: Option<&[usize]>,
        render_data: &mut RenderData,
    ) {
        self.render_to_space(active_window, render_data, self.geometry(), 0);
    }

    pub fn new_toplevel(&mut self, surface: W::TopLevelSurfaceType, focus: &[usize]) {
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

        if *is_tabbed {
            let new_win = W::from_toplevel(geometry.size, surface);
            children.push(DendriteTree::Leaf {
                window: new_win,
                geometry: *geometry,
            });
            return;
        }

        let new_window_size = match orientation {
            Orientation::Vertical => Size::new(geometry.size.w, geometry.size.h / 2),
            Orientation::Horizontal => Size::new(geometry.size.w / 2, geometry.size.h),
        };
        let new_win = W::from_toplevel(new_window_size, surface);

        let Some(x) = focus.first() else {
            children.push(DendriteTree::Leaf {
                window: new_win,
                geometry: Rectangle {
                    loc: geometry.loc,
                    size: new_window_size,
                },
            });
            return;
        };

        tracing::warn!("Inserting after {x:?}");
        let new_geometry = Rectangle::new(
            match orientation {
                Orientation::Vertical => Point::new(
                    geometry.loc.x,
                    children[*x].geometry().loc.y + children[*x].geometry().size.h,
                ),
                Orientation::Horizontal => Point::new(
                    children[*x].geometry().loc.x + children[*x].geometry().size.w,
                    geometry.loc.y,
                ),
            },
            new_window_size,
        );
        children.insert(
            x + 1,
            DendriteTree::Leaf {
                window: new_win,
                geometry: new_geometry,
            },
        );
        scroll_window_into_view(
            new_geometry,
            *orientation,
            *geometry,
            &mut children[..=(x + 1)],
        );
        for further_window in &mut children[(x + 2)..] {
            further_window.update_position(Point::new(new_window_size.w, new_window_size.h));
        }
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
        }
    }

    pub fn toplevel_destroyed(&mut self, path: &[usize]) -> (FocusSuggestion, bool) {
        tracing::info!("Destroying {path:?}");
        let DendriteTree::Container {
            children,
            orientation,
            is_tabbed,
            ..
        } = self
        else {
            tracing::warn!("toplevel_destroyed called on a leaf!");
            return (FocusSuggestion::default(), false);
        };

        if let [x, xs @ ..] = path
            && !xs.is_empty()
        {
            let (mut focus_recommendation, delete_child) = children[*x].toplevel_destroyed(xs);
            if !delete_child {
                focus_recommendation.push(*x);
                return (focus_recommendation, false);
            }
            return delete_child_and_suggest_focus(children, *x);
        }

        let [i] = path else {
            tracing::warn!("toplevel_destroyed called with empty path?");
            return (FocusSuggestion::default(), false);
        };

        children[*i].send_close();

        let offset_direction = if *i == children.len() - 1 { 1 } else { -1 };
        let offset = match orientation {
            Orientation::Vertical => {
                Point::new(0, offset_direction * children[*i].geometry().size.h)
            }
            Orientation::Horizontal => {
                Point::new(offset_direction * children[*i].geometry().size.w, 0)
            }
        };

        let new_focus = delete_child_and_suggest_focus(children, *i);
        if *is_tabbed {
            return new_focus;
        }

        if *i == children.len() {
            for child in children {
                child.update_position(offset);
            }
        } else {
            for child in children.iter_mut().skip(*i) {
                child.update_position(offset);
            }
        }
        return new_focus;
    }

    fn handle_move_focus(
        &mut self,
        focus: &mut Vec<usize>,
        index: usize,
        mut action: Action,
    ) -> Option<(Action, Point<i32, Logical>)> {
        let DendriteTree::Container {
            children,
            orientation,
            geometry,
            ..
        } = self
        else {
            tracing::warn!("Invalid move of focus on leaf! Focus {focus:?}, index {index:?}");
            return None;
        };

        if index >= focus.len() {
            tracing::warn!("Focus {focus:?} doesn't have index {index:?}");
            return None;
        }

        let mut suggested_point = geometry.loc;
        let mut moved_up = false;
        if index < focus.len() - 1 {
            let Some((residual_action, inner_suggested_point)) =
                children[focus[index]].handle_move_focus(focus, index + 1, action)
            else {
                return None;
            };
            action = residual_action;
            suggested_point = inner_suggested_point;
            moved_up = true;
        }

        let child_index_offset: isize = match (action, *orientation) {
            (Action::MoveFocusUp | Action::MoveFocusDown, Orientation::Horizontal) => {
                return Some((action, suggested_point));
            }
            (Action::MoveFocusUp, Orientation::Vertical) => -1,
            (Action::MoveFocusDown, Orientation::Vertical) => 1,
            (Action::MoveFocusLeft | Action::MoveFocusRight, Orientation::Vertical) => {
                return Some((action, suggested_point));
            }
            (Action::MoveFocusLeft, Orientation::Horizontal) => -1,
            (Action::MoveFocusRight, Orientation::Horizontal) => 1,
            // TODO: panic instead? Or be smarter about types here?
            _ => return Some((action, suggested_point)),
        };

        let new_child_index = (focus[index] as isize) + child_index_offset;
        if new_child_index < 0 || new_child_index >= (children.len() as isize) {
            return Some((action, suggested_point));
        }

        if moved_up {
            focus.truncate(index + 1);
        }

        focus[index] = new_child_index as usize;
        scroll_window_into_view(
            children[new_child_index as usize].geometry(),
            *orientation,
            *geometry,
            children,
        );

        let mut child: &mut DendriteTree<W> = &mut children[new_child_index as usize];
        while let DendriteTree::Container {
            children: grandchildren,
            orientation: child_orientation,
            geometry: child_geometry,
            ..
        } = child
        {
            let (new_child_geometry, i) = {
                let Some((i, closest)) =
                    grandchildren.iter_mut().enumerate().min_by_key(|(_i, w)| {
                        let Point { x: x1, y: y1, .. } = w.geometry().loc;
                        let Point { x: x2, y: y2, .. } = suggested_point;
                        (x1 - x2) * (x1 - x2) + (y1 - y2) * (y1 - y2)
                    })
                else {
                    tracing::warn!("Focussing into an empty tree node?");
                    break;
                };
                focus.push(i);
                let size = closest.geometry();
                (size, i)
            };
            scroll_window_into_view(
                new_child_geometry,
                *child_orientation,
                *child_geometry,
                grandchildren,
            );
            child = &mut grandchildren[i];
        }
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
            Action::MoveFocusUp
            | Action::MoveFocusDown
            | Action::MoveFocusLeft
            | Action::MoveFocusRight => self.handle_move_focus(focus, 0, action).map(|(a, _i)| a),
            Action::MakeInnerTree => {
                self.make_inner_tree(focus, 0);
                None
            }
            Action::CloseWindow => {
                let _ = std::mem::replace(focus, self.toplevel_destroyed(&focus).0.into());
                None
            }
        }
    }

    pub fn resize_output(&mut self, new_size: Size<i32, Logical>) {
        match self {
            DendriteTree::Leaf { geometry, .. } => geometry.size = new_size,
            // TODO: scale children?
            DendriteTree::Container { geometry, .. } => geometry.size = new_size,
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
}
