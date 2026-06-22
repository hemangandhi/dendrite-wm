mod compositor;
mod xdg_shell;

use std::time::Duration;

use crate::DendriteState;

//
// Wl Seat
//

use smithay::desktop::{layer_map_for_output, LayerSurface};
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::utils::Size;
use smithay::wayland::output::OutputHandler;
use smithay::wayland::seat::WaylandFocus;
use smithay::wayland::selection::data_device::{
    set_data_device_focus, ClientDndGrabHandler, DataDeviceHandler, DataDeviceState,
    ServerDndGrabHandler,
};
use smithay::wayland::selection::SelectionHandler;

use smithay::wayland::shell::wlr_layer::WlrLayerShellHandler;
use smithay::wayland::xdg_activation::{
    XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
};
use smithay::{
    delegate_data_device, delegate_layer_shell, delegate_output, delegate_seat,
    delegate_xdg_activation,
};

impl SeatHandler for DendriteState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<DendriteState> {
        &mut self.seat_state
    }

    fn cursor_image(
        &mut self,
        _seat: &Seat<Self>,
        _image: smithay::input::pointer::CursorImageStatus,
    ) {
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = &self.display_handle;
        let client = focused.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, client);
    }
}

delegate_seat!(DendriteState);

//
// Wl Data Device
//

impl SelectionHandler for DendriteState {
    type SelectionUserData = ();
}

impl DataDeviceHandler for DendriteState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for DendriteState {}
impl ServerDndGrabHandler for DendriteState {}

delegate_data_device!(DendriteState);

//
// Wl Output & Xdg Output
//

impl OutputHandler for DendriteState {}
delegate_output!(DendriteState);

// Activation
impl XdgActivationHandler for DendriteState {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.xdg_activation_state
    }

    fn request_activation(
        &mut self,
        token: XdgActivationToken,
        _token_data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        if self.xdg_activation_state.remove_token(&token) {
            self.active_pointer = self
                .layout
                .iter()
                .enumerate()
                .find(|(_i, w)| w.wl_surface().map(|cw| *cw == surface).unwrap_or(false))
                .map(|(i, _w)| i);
            self.dirty = true;
        }
    }
}
delegate_xdg_activation!(DendriteState);

impl WlrLayerShellHandler for DendriteState {
    fn shell_state(&mut self) -> &mut smithay::wayland::shell::wlr_layer::WlrLayerShellState {
        &mut self.wlr_layer_state
    }

    fn new_layer_surface(
        &mut self,
        surface: smithay::wayland::shell::wlr_layer::LayerSurface,
        _output: Option<wayland_server::protocol::wl_output::WlOutput>,
        layer: smithay::wayland::shell::wlr_layer::Layer,
        namespace: String,
    ) {
        let Some(output) = self.space.outputs().next() else {
            return;
        };
        let mut map = layer_map_for_output(&output);
        let layer_surface = LayerSurface::new(surface, namespace);
        layer_surface.layer_surface().with_pending_state(|s| {
            let Some(Size { w, h, .. }) = self.space.output_geometry(output).map(|g| g.size) else {
                return;
            };
            s.size = Some(Size::new(w, h));
        });
        layer_surface.layer_surface().send_pending_configure();
        map.map_layer(&layer_surface).unwrap();
        layer_surface.send_frame(
            output,
            self.start_time.elapsed(),
            Some(Duration::ZERO),
            |_, _| Some(output.clone()),
        );
    }
}
delegate_layer_shell!(DendriteState);
