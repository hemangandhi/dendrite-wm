use smithay::desktop::LayerSurface;
use smithay::wayland::shell::wlr_layer::{KeyboardInteractivity, LayerSurfaceData};
use smithay::{
    delegate_xdg_shell,
    desktop::{
        PopupKeyboardGrab, PopupKind, PopupManager, Space, Window, WindowSurfaceType,
        find_popup_root_surface, get_popup_toplevel_coords, layer_map_for_output,
    },
    input::{
        Seat,
        pointer::{Focus, GrabStartData as PointerGrabStartData},
    },
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            Resource,
            protocol::{wl_seat, wl_surface::WlSurface},
        },
    },
    utils::{Rectangle, SERIAL_COUNTER, Serial, Size},
    wayland::{
        compositor::with_states,
        input_method::InputMethodSeat,
        seat::WaylandFocus,
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
            XdgToplevelSurfaceData,
        },
    },
};

use crate::{
    DendriteState,
    grabs::{MoveSurfaceGrab, ResizeSurfaceGrab},
};

impl XdgShellHandler for DendriteState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        self.layout.new_toplevel(surface);
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.unconstrain_popup(&surface);
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            let geometry = positioner.get_geometry();
            state.geometry = geometry;
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn move_request(&mut self, surface: ToplevelSurface, seat: wl_seat::WlSeat, serial: Serial) {
        let seat = Seat::from_resource(&seat).unwrap();

        let wl_surface = surface.wl_surface();

        if let Some(start_data) = check_grab(&seat, wl_surface, serial) {
            let pointer = seat.get_pointer().unwrap();

            let window = self
                .space
                .elements()
                .find(|w| w.toplevel().unwrap().wl_surface() == wl_surface)
                .unwrap()
                .clone();
            let initial_window_location = self.space.element_location(&window).unwrap();

            let grab = MoveSurfaceGrab {
                start_data,
                window,
                initial_window_location,
            };

            pointer.set_grab(self, grab, serial, Focus::Clear);
        }
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        seat: wl_seat::WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        let seat = Seat::from_resource(&seat).unwrap();

        let wl_surface = surface.wl_surface();

        if let Some(start_data) = check_grab(&seat, wl_surface, serial) {
            let pointer = seat.get_pointer().unwrap();

            let window = self
                .space
                .elements()
                .find(|w| w.toplevel().unwrap().wl_surface() == wl_surface)
                .unwrap()
                .clone();
            let initial_window_location = self.space.element_location(&window).unwrap();
            let initial_window_size = window.geometry().size;

            surface.with_pending_state(|state| {
                state.states.set(xdg_toplevel::State::Resizing);
            });

            surface.send_pending_configure();

            let grab = ResizeSurfaceGrab::start(
                start_data,
                window,
                edges.into(),
                Rectangle::new(initial_window_location, initial_window_size),
            );

            pointer.set_grab(self, grab, serial, Focus::Clear);
        }
    }

    fn grab(&mut self, surface: PopupSurface, _seat: wl_seat::WlSeat, serial: Serial) {
        tracing::info!("Here!");
        let popup = PopupKind::Xdg(surface.clone());
        let Ok(root) = find_popup_root_surface(&popup) else {
            tracing::warn!("Grab: no popup root");
            return;
        };
        let Ok(grab) = self.popups.grab_popup(root, popup, &self.seat, serial) else {
            tracing::warn!("Grab: can't grab_popup from manager");
            return;
        };

        self.layout.kill_focus();

        // No double-grab and no grabbing by popups that can't take the keyboard.
        if self.seat.input_method().keyboard_grabbed()
            || layer_map_for_output(self.space.outputs().next().unwrap())
                .layer_for_surface(surface.wl_surface(), WindowSurfaceType::TOPLEVEL)
                .map(|s| s.can_receive_keyboard_focus())
                .unwrap_or(false)
        {
            tracing::warn!("Grab: layer can't take keyboard or keyboard is grabbed");
            return;
        }
        let Some(k) = self.seat.get_keyboard() else {
            tracing::warn!("Grab: no seat keybaord");
            return;
        };
        k.set_grab(self, PopupKeyboardGrab::new(&grab), serial);
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface);
        self.layout.toplevel_destroyed(&window);
    }
}

// Xdg Shell
delegate_xdg_shell!(DendriteState);

fn check_grab(
    seat: &Seat<DendriteState>,
    surface: &WlSurface,
    serial: Serial,
) -> Option<PointerGrabStartData<DendriteState>> {
    let pointer = seat.get_pointer()?;

    // Check that this surface has a click grab.
    if !pointer.has_grab(serial) {
        return None;
    }

    let start_data = pointer.grab_start_data()?;

    let (focus, _) = start_data.focus.as_ref()?;
    // If the focus was for a different surface, ignore the request.
    if !focus.id().same_client_as(&surface.id()) {
        return None;
    }

    Some(start_data)
}

impl DendriteState {
    /// Should be called on `WlSurface::commit`
    pub fn handle_commit(&mut self, surface: &WlSurface) {
        // Handle toplevel commits.
        if let Some(window) = self
            .space
            .elements()
            .find(|w| w.toplevel().unwrap().wl_surface() == surface)
            .cloned()
        {
            let initial_configure_sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .initial_configure_sent
            });

            if !initial_configure_sent {
                window.toplevel().unwrap().send_configure();
            }
        }

        // Handle popup commits.
        self.popups.commit(surface);
        if let Some(popup) = self.popups.find_popup(surface) {
            match popup {
                PopupKind::Xdg(ref xdg) => {
                    if !xdg.is_initial_configure_sent() {
                        // NOTE: This should never fail as the initial configure is always
                        // allowed.
                        xdg.send_configure().expect("initial configure failed");
                    }
                }
                PopupKind::InputMethod(ref _input_method) => {}
            }
        }

        // Wlr popups
        let Some(popup_surface) = self.get_popup_surface_to_focus(surface) else {
            return;
        };

        if self
            .layout
            .get_focused_window()
            .and_then(|w| w.wl_surface())
            .map(|s| *s != popup_surface)
            .unwrap_or(false)
        {
            self.layout.kill_focus();
            let Some(k) = self.seat.get_keyboard() else {
                tracing::warn!("No keyboard");
                return;
            };
            k.set_focus(self, None, SERIAL_COUNTER.next_serial());
            k.set_focus(self, Some(popup_surface), SERIAL_COUNTER.next_serial());
        }
    }

    fn get_popup_surface_to_focus(&self, surface: &WlSurface) -> Option<WlSurface> {
        let mut map = layer_map_for_output(self.space.outputs().next().unwrap());
        map.arrange();
        let Some(layer) = map.layer_for_surface(
            surface,
            WindowSurfaceType::POPUP | WindowSurfaceType::TOPLEVEL,
        ) else {
            return None;
        };
        if !with_states(surface, |s| {
            s.data_map
                .get::<LayerSurfaceData>()
                .map(|data| {
                    data.lock()
                        .map(|data| data.initial_configure_sent)
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        }) {
            layer.layer_surface().send_configure();
            return None;
        }
        let state = layer.cached_state();
        if state.keyboard_interactivity == KeyboardInteractivity::Exclusive
            || state.keyboard_interactivity == KeyboardInteractivity::OnDemand
        {
            return Some(layer.wl_surface().clone());
        }
        return None;
    }

    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else {
            return;
        };
        let Some(window) = self
            .space
            .elements()
            .find(|w| w.toplevel().unwrap().wl_surface() == &root)
        else {
            return;
        };

        let output = self.space.outputs().next().unwrap();
        let output_geo = self.space.output_geometry(output).unwrap();
        let window_geo = self.space.element_geometry(window).unwrap();

        // The target geometry for the positioner should be relative to its parent's geometry, so
        // we will compute that here.
        let mut target = output_geo;
        target.loc -= get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
        target.loc -= window_geo.loc;

        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }
}
