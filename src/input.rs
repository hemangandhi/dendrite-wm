use std::ffi::OsStr;
use std::io::Error;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::thread;

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
        KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
    },
    input::{
        keyboard::{FilterResult, Keysym},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    utils::SERIAL_COUNTER,
    wayland::{seat::WaylandFocus, xdg_activation::XdgActivationToken},
};

use crate::layout::action::Action;
use crate::state::DendriteState;

impl DendriteState {
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event, .. } => {
                let serial = SERIAL_COUNTER.next_serial();
                let time = Event::time_msec(&event);

                self.seat.get_keyboard().unwrap().input::<(), _>(
                    self,
                    event.key_code(),
                    event.state(),
                    serial,
                    time,
                    |this, mods, keysym| {
                        let pressed = event.state() == KeyState::Pressed;
                        if !pressed || !mods.alt {
                            return FilterResult::Forward;
                        }

                        match keysym.raw_latin_sym_or_raw_current_sym() {
                            Some(Keysym::Return) => {
                                let (token, _) =
                                    this.xdg_activation_state.create_external_token(None);
                                spawn_sync("contour", Some(token.clone()));

                                return FilterResult::Intercept(());
                            }
                            Some(Keysym::q) => {
                                this.layout.send_close();
                                return FilterResult::Intercept(());
                            }
                            Some(Keysym::h) => {
                                this.layout.handle_action(Action::MoveLeft);
                                return FilterResult::Intercept(());
                            }
                            Some(Keysym::j) => {
                                this.layout.handle_action(Action::MoveDown);
                                return FilterResult::Intercept(());
                            }
                            Some(Keysym::k) => {
                                this.layout.handle_action(Action::MoveUp);
                                return FilterResult::Intercept(());
                            }
                            Some(Keysym::l) => {
                                this.layout.handle_action(Action::MoveRight);
                                return FilterResult::Intercept(());
                            }
                            _ => return FilterResult::Forward,
                        };
                    },
                );
            }
            InputEvent::PointerMotion { .. } => {}
            InputEvent::PointerMotionAbsolute { event, .. } => {
                let output = self.space.outputs().next().unwrap();

                let output_geo = self.space.output_geometry(output).unwrap();

                let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();

                let serial = SERIAL_COUNTER.next_serial();

                let pointer = self.seat.get_pointer().unwrap();

                let under = self.surface_under(pos);

                pointer.motion(
                    self,
                    under,
                    &MotionEvent {
                        location: pos,
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerButton { event, .. } => {
                let pointer = self.seat.get_pointer().unwrap();
                let serial = SERIAL_COUNTER.next_serial();
                let button = event.button_code();
                let button_state = event.state();

                pointer.button(
                    self,
                    &ButtonEvent {
                        button,
                        state: button_state,
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerAxis { event, .. } => {
                let source = event.source();

                let horizontal_amount = event.amount(Axis::Horizontal).unwrap_or_else(|| {
                    event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.
                });
                let vertical_amount = event.amount(Axis::Vertical).unwrap_or_else(|| {
                    event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.
                });
                let horizontal_amount_discrete = event.amount_v120(Axis::Horizontal);
                let vertical_amount_discrete = event.amount_v120(Axis::Vertical);

                let mut frame = AxisFrame::new(event.time_msec()).source(source);
                if horizontal_amount != 0.0 {
                    frame = frame.value(Axis::Horizontal, horizontal_amount);
                    if let Some(discrete) = horizontal_amount_discrete {
                        frame = frame.v120(Axis::Horizontal, discrete as i32);
                    }
                }
                if vertical_amount != 0.0 {
                    frame = frame.value(Axis::Vertical, vertical_amount);
                    if let Some(discrete) = vertical_amount_discrete {
                        frame = frame.v120(Axis::Vertical, discrete as i32);
                    }
                }

                if source == AxisSource::Finger {
                    if event.amount(Axis::Horizontal) == Some(0.0) {
                        frame = frame.stop(Axis::Horizontal);
                    }
                    if event.amount(Axis::Vertical) == Some(0.0) {
                        frame = frame.stop(Axis::Vertical);
                    }
                }

                let pointer = self.seat.get_pointer().unwrap();
                pointer.axis(self, frame);
                pointer.frame(self);
            }
            _ => {}
        }
    }
}

fn spawn_sync<T: AsRef<OsStr> + Send + 'static>(command: T, token: Option<XdgActivationToken>) {
    let tracing_span = tracing::info_span!("spawn");
    let _span_enter = tracing_span.enter();

    let res = thread::Builder::new()
        .name("Spawn thread".to_owned())
        .spawn(move || {
            let mut process = Command::new(command.as_ref());
            process
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());

            if let Some(token) = token.as_ref() {
                process.env("XDG_ACTIVATION_TOKEN", token.as_str());
                process.env("DESKTOP_STARTUP_ID", token.as_str());
            }

            let Some(mut child) = do_spawn(command.as_ref(), process) else {
                return;
            };

            match child.wait() {
                Ok(status) => {
                    if !status.success() {
                        let command_str = command.as_ref().to_str();
                        tracing::warn!("Spawn for {command_str:?} failed with status {status:?}");
                    }
                }
                Err(e) => {
                    let command_str = command.as_ref().to_str();
                    tracing::warn!("Spawn for {command_str:?} failed.");
                }
            }
        });

    if let Err(e) = res {
        tracing::warn!("Spawn't {e:?}");
    }
}

fn do_spawn(command: &OsStr, mut process: Command) -> Option<Child> {
    unsafe {
        // Double-fork to avoid having to waitpid the child.
        process.pre_exec(move || {
            match libc::fork() {
                -1 => return Err(Error::last_os_error()),
                0 => (),
                _ => libc::_exit(0),
            }

            // TODO: fix the rlimit?
            // restore_nofile_rlimit();

            Ok(())
        });
    }

    let child = match process.spawn() {
        Ok(child) => child,
        Err(err) => {
            tracing::warn!("error spawning {command:?}: {err:?}");
            return None;
        }
    };

    Some(child)
}
