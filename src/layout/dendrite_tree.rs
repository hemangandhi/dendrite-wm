use smithay::desktop::Window;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Rectangle, Size};
use smithay::wayland::seat::WaylandFocus;
use smithay::wayland::shell::xdg::ToplevelSurface;

use crate::layout::action::Action;
use crate::render::RenderData;

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

pub enum DendriteTree {
    Leaf {
        window: Window,
        // Always in the output frame (so far...)
        geometry: Rectangle<i32, Logical>,
    },
    Container {
        children: Vec<DendriteTree>,
        // Always in the output frame (so far...)
        geometry: Rectangle<i32, Logical>,
        orientation: Orientation,
        is_tabbed: bool,
    },
}

impl Default for DendriteTree {
    fn default() -> Self {
        Self::Container {
            children: vec![],
            geometry: Default::default(),
            orientation: Orientation::Horizontal,
            is_tabbed: false,
        }
    }
}

impl From<Size<i32, Logical>> for DendriteTree {
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

fn configure_surface_in_size(size: Size<i32, Logical>, surface: ToplevelSurface) -> Window {
    surface.with_pending_state(|p| p.bounds = Some(size));
    surface.send_pending_configure();
    return Window::new_wayland_window(surface);
}

fn delete_child_and_suggest_focus(
    children: &mut Vec<DendriteTree>,
    i: usize,
) -> (FocusSuggestion, bool) {
    children.remove(i);
    if i != 0 {
        (FocusSuggestion::singleton(i - 1), false)
    } else if children.is_empty() {
        (FocusSuggestion::default(), true)
    } else {
        (FocusSuggestion::singleton(1), false)
    }
}

fn scroll_window_into_view(
    new_window_geometry: Rectangle<i32, Logical>,
    orientation: Orientation,
    parent_geometry: Rectangle<i32, Logical>,
    children: &mut [DendriteTree],
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

impl DendriteTree {
    pub fn render_to_space(&self, active_window: Option<&[usize]>, render_data: &mut RenderData) {
        let geometry = self.geometry();
        match self {
            DendriteTree::Leaf { window, .. } => {
                render_data.render_or_map(window, geometry.loc, active_window.is_some());
            }
            DendriteTree::Container { children, .. } => {
                for (i, child) in children.iter().enumerate() {
                    // TODO: track children that just went off-screen -- they need a render.
                    // let child_geometry = child.geometry();
                    // if !geometry.contains(child_geometry.loc)
                    //     && !geometry.contains(child_geometry.loc + child_geometry.size)
                    // {
                    //     continue;
                    // }
                    child.render_to_space(
                        active_window.and_then(|a| match a {
                            [] => Some(&[] as &[usize]),
                            [x, xs @ ..] if *x == i => Some(xs),
                            _ => None,
                        }),
                        render_data,
                    );
                    // TODO: we may have to raise any windows at our level since the space behaves that way.
                }
            }
        }
    }

    pub fn new_toplevel(&mut self, surface: ToplevelSurface, focus: &[usize]) {
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
            let new_win = configure_surface_in_size(geometry.size, surface);
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
        let new_win = configure_surface_in_size(new_window_size, surface);

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

    pub fn path_to_window<'a>(&'a self, window: &Window) -> Option<(FocusSuggestion, &'a Window)> {
        match window.wl_surface() {
            Some(s) => self.path_to_surface(&*s.clone()),
            None => None,
        }
    }

    pub fn path_to_surface<'a>(
        &'a self,
        surface: &WlSurface,
    ) -> Option<(FocusSuggestion, &'a Window)> {
        match self {
            DendriteTree::Leaf {
                window: real_window,
                ..
            } => {
                return if real_window
                    .wl_surface()
                    .map(|s| *s == *surface)
                    .unwrap_or(false)
                {
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

    pub fn window_at_path<'a>(&'a self, path: &[usize]) -> Option<&'a Window> {
        match (self, path) {
            (DendriteTree::Leaf { window, .. }, []) => Some(window),
            (DendriteTree::Leaf { .. }, _x) => None,
            (DendriteTree::Container { children, .. }, [x]) => children[*x].window_at_path(&[]),
            (DendriteTree::Container { children, .. }, [x, xs @ ..]) => {
                children[*x].window_at_path(xs)
            }
            (DendriteTree::Container { .. }, []) => None,
        }
    }

    pub fn toplevel_destroyed(&mut self, path: &[usize]) -> (FocusSuggestion, bool) {
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

    fn handle_move(
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
            tracing::warn!("Invalid move of focus on leaf");
            return None;
        };

        if index >= focus.len() {
            tracing::warn!("Focus {focus:?} don't have index {index:?}");
            return None;
        }

        let mut suggested_point = geometry.loc;
        let mut moved_up = false;
        if focus.len() - index > 1 {
            let Some((residual_action, inner_suggested_point)) =
                children[index].handle_move(focus, index + 1, action)
            else {
                return None;
            };
            tracing::warn!("Bubbling up from {focus:?} at {index:?}");
            action = residual_action;
            suggested_point = inner_suggested_point;
            moved_up = true;
        }

        let child_index_offset: isize = match (action, *orientation) {
            (Action::MoveUp | Action::MoveDown, Orientation::Horizontal) => {
                return Some((action, suggested_point));
            }
            (Action::MoveUp, Orientation::Vertical) => -1,
            (Action::MoveDown, Orientation::Vertical) => 1,
            (Action::MoveLeft | Action::MoveRight, Orientation::Vertical) => {
                return Some((action, suggested_point));
            }
            (Action::MoveLeft, Orientation::Horizontal) => -1,
            (Action::MoveRight, Orientation::Horizontal) => 1,
        };

        let new_child_index = (focus[index] as isize) + child_index_offset;
        if new_child_index < 0 || new_child_index >= (children.len() as isize) {
            // TODO: is this wise or do I need index - 1
            if index > 0 {
                focus.truncate(index);
            }
            return Some((action, suggested_point));
        }

        focus[index] = new_child_index as usize;
        scroll_window_into_view(
            children[new_child_index as usize].geometry(),
            *orientation,
            *geometry,
            children,
        );
        if !moved_up {
            return None;
        }

        let mut child: &mut DendriteTree = &mut children[new_child_index as usize];
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

    pub fn handle_action(&mut self, focus: &mut Vec<usize>, action: Action) -> Option<Action> {
        match action {
            Action::MoveUp | Action::MoveDown | Action::MoveLeft | Action::MoveRight => {
                self.handle_move(focus, 0, action).map(|(a, _i)| a)
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
