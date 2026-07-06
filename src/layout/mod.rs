use smithay::desktop::Window;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Size};
use smithay::wayland::shell::xdg::ToplevelSurface;

use crate::render::RenderData;

pub mod action;
mod dendrite_tree;

#[derive(Default)]
pub struct Root {
    tree: dendrite_tree::DendriteTree<Window>,
    active_window: Vec<usize>,
    is_dirty: bool,
    windows_to_deactivate: Vec<Window>,
}

impl From<Size<i32, Logical>> for Root {
    fn from(value: Size<i32, Logical>) -> Self {
        Self {
            tree: value.into(),
            active_window: vec![],
            is_dirty: false,
            windows_to_deactivate: vec![],
        }
    }
}

impl Root {
    pub fn render_to_space(&mut self, render_data: &mut RenderData) {
        if self.is_dirty {
            for w in self.windows_to_deactivate.drain(..) {
                w.set_activated(false);
            }
            if let Some(w) = self.tree.window_at_path(&self.active_window) {
                w.set_activated(true);
            }
        }
        self.is_dirty = false;

        self.tree
            .render_to_space_root(Some(&self.active_window), render_data);
    }

    pub fn new_toplevel(&mut self, surface: ToplevelSurface) {
        self.is_dirty = true;
        self.tree.new_toplevel(surface, &self.active_window);
    }

    pub fn toplevel_destroyed(&mut self, window: &Window) {
        self.is_dirty = true;
        let Some((suggestion, w)) = self
            .tree
            .path_to_window(window)
            .map(|(s, w)| (s, w.clone()))
        else {
            tracing::warn!("Destroyed toplevel wasn't in tree.");
            return;
        };

        let path: Vec<usize> = suggestion.into();
        let (new_focus, _d) = self.tree.toplevel_destroyed(&path);
        // If the old window was focused, update the focus.
        if path == self.active_window {
            self.windows_to_deactivate.push(w);
            self.active_window = new_focus.into();
        }
    }

    pub fn handle_action(&mut self, action: action::Action) {
        let old_window = self.tree.window_at_path(&self.active_window).cloned();
        if !self
            .tree
            .handle_action(&mut self.active_window, action)
            .is_none()
        {
            // Move failed or something
            return;
        }
        self.is_dirty = true;
        if let Some(w) = old_window {
            self.windows_to_deactivate.push(w);
        }
    }

    pub fn get_focused_window(&self) -> Option<&Window> {
        self.tree.window_at_path(&self.active_window)
    }

    // TODO: actually store the focused window somewhere.
    pub fn kill_focus(&mut self) {
        if let Some(w) = self.tree.window_at_path(&self.active_window) {
            self.windows_to_deactivate.push(w.clone());
        }
        self.active_window = vec![];
        self.is_dirty = true;
    }

    pub fn focus_surface(&mut self, surface: WlSurface) {
        let old_focus = self.tree.window_at_path(&self.active_window).cloned();
        let Some((p, _w)) = self.tree.path_to_surface(&surface) else {
            tracing::info!("No path to surface {surface:?}");
            return;
        };

        if let Some(w) = old_focus {
            self.windows_to_deactivate.push(w);
        }
        self.active_window = p.into();
        self.is_dirty = true;
    }

    pub fn resize_output(&mut self, new_size: Size<i32, Logical>) {
        self.tree.resize_output(new_size);
    }
}
