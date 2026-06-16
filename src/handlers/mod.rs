mod compositor;
mod xdg_shell;

use crate::DendriteState;

//
// Wl Seat
//

use smithay::backend::input::KeyState;
use smithay::input::keyboard::{keysyms, FilterResult, Keysym};
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::wayland::output::OutputHandler;
use smithay::wayland::selection::data_device::{
    set_data_device_focus, ClientDndGrabHandler, DataDeviceHandler, DataDeviceState,
    ServerDndGrabHandler,
};
use smithay::wayland::selection::SelectionHandler;

use smithay::wayland::xdg_activation::{
    XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
};
use smithay::{delegate_data_device, delegate_output, delegate_seat, delegate_xdg_activation};

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
        token_data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        // Yeah, I gotta figure out what activation means ig.
    }
}
delegate_xdg_activation!(DendriteState);
