use std::time::Duration;

use smithay::wayland::seat::WaylandFocus;
use smithay::{
    backend::{
        renderer::{
            damage::OutputDamageTracker,
            element::{surface::WaylandSurfaceRenderElement, AsRenderElements},
            gles::GlesRenderer,
        },
        winit::{self, WinitEvent},
    },
    desktop::Window,
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::{Point, Rectangle, Transform, SERIAL_COUNTER},
};

use crate::{CalloopData, DendriteState};

pub fn init_winit(
    event_loop: &mut EventLoop<CalloopData>,
    data: &mut CalloopData,
) -> Result<(), Box<dyn std::error::Error>> {
    let display_handle = &mut data.display_handle;
    let state = &mut data.state;

    let (mut backend, winit) = winit::init()?;

    let mode = Mode {
        size: backend.window_size(),
        refresh: 60_000,
    };

    let output = Output::new(
        "winit".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Smithay".into(),
            model: "Winit".into(),
        },
    );
    let _global = output.create_global::<DendriteState>(display_handle);
    output.change_current_state(
        Some(mode),
        Some(Transform::Flipped180),
        None,
        Some((0, 0).into()),
    );
    output.set_preferred(mode);

    state.space.map_output(&output, (0, 0));

    let mut damage_tracker = OutputDamageTracker::from_output(&output);

    unsafe {
        std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);
    }

    event_loop
        .handle()
        .insert_source(winit, move |event, _, data| {
            let display = &mut data.display_handle;
            let state = &mut data.state;

            match event {
                WinitEvent::Resized { size, .. } => {
                    output.change_current_state(
                        Some(Mode {
                            size,
                            refresh: 60_000,
                        }),
                        None,
                        None,
                        None,
                    );
                }
                WinitEvent::Input(event) => state.process_input_event(event),
                WinitEvent::Redraw => {
                    let size = backend.window_size();
                    let damage = Rectangle::from_size(size);

                    if state.dirty {
                        state.dirty = false;
                        for (i, elt) in state.layout.iter().cloned().enumerate() {
                            let is_active = state.active_pointer.map(|p| p == i).unwrap_or(false);
                            elt.set_activated(is_active);
                            state
                                .space
                                .map_element(elt, (0, (i as i32) * 100), is_active);
                        }
                        if let Some(index) = state.active_pointer {
                            if let Some(kbd) = state.seat.get_keyboard() {
                                let surface =
                                    state.layout[index].wl_surface().map(|s| s.into_owned());
                                kbd.set_focus(state, surface, SERIAL_COUNTER.next_serial());
                            }
                        }
                    }

                    {
                        let (renderer, mut framebuffer) = backend.bind().unwrap();

                        let render_elts: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = state
                            .space
                            .elements_for_output(state.space.outputs().next().unwrap())
                            .flat_map(|window| {
                                let Some((index_of_window, _w)) = state
                                    .layout
                                    .iter()
                                    .enumerate()
                                    .find(|(_i, w)| w.wl_surface() == window.wl_surface())
                                else {
                                    return window.render_elements(
                                        renderer,
                                        Point::new(0, 0),
                                        1.0.into(),
                                        1.0,
                                    );
                                };
                                window.render_elements(
                                    renderer,
                                    Point::new(0, index_of_window as i32 * 100)
                                        .to_physical_precise_round(1.0),
                                    1.0.into(),
                                    if state
                                        .active_pointer
                                        .map(|i| {
                                            state.layout[i].wl_surface() == window.wl_surface()
                                        })
                                        .unwrap_or(false)
                                    {
                                        1.0
                                    } else {
                                        0.9
                                    },
                                )
                            })
                            .collect();

                        smithay::desktop::space::render_output::<
                            _,
                            WaylandSurfaceRenderElement<GlesRenderer>,
                            Window,
                            _,
                        >(
                            &output,
                            renderer,
                            &mut framebuffer,
                            1.0,
                            0,
                            [],
                            &render_elts,
                            &mut damage_tracker,
                            [0.1, 0.1, 0.1, 1.0],
                        )
                        .unwrap();
                    }
                    backend.submit(Some(&[damage])).unwrap();

                    state.space.elements().for_each(|window| {
                        window.send_frame(
                            &output,
                            state.start_time.elapsed(),
                            Some(Duration::ZERO),
                            |_, _| Some(output.clone()),
                        )
                    });

                    // TODO: is this really the right spot for this?
                    state.wlr_layer_state.layer_surfaces().for_each(|layer| {
                        layer.send_configure();
                    });

                    state.space.refresh();
                    state.popups.cleanup();
                    let _ = display.flush_clients();

                    // Ask for redraw to schedule new frame.
                    backend.window().request_redraw();
                }
                WinitEvent::CloseRequested => {
                    state.loop_signal.stop();
                }
                _ => (),
            };
        })?;

    Ok(())
}
