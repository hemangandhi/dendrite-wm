use std::borrow::Cow;
use std::time::Duration;

use smithay::wayland::seat::WaylandFocus;
use smithay::{
    backend::{
        renderer::{
            damage::OutputDamageTracker, element::surface::WaylandSurfaceRenderElement,
            gles::GlesRenderer,
        },
        winit::{self, WinitEvent},
    },
    desktop::Window,
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::{Rectangle, SERIAL_COUNTER, Size, Transform},
};

use crate::render::RenderData;
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
                    let scale = 1.0 / output.current_scale().fractional_scale();
                    state.layout.resize_output(Size::new(
                        ((size.w as f64) * scale) as i32,
                        ((size.h as f64) * scale) as i32,
                    ));
                }
                WinitEvent::Input(event) => state.process_input_event(event),
                WinitEvent::Redraw => {
                    let size = backend.window_size();
                    let damage = Rectangle::from_size(size);

                    if let Some(kbd) = state.seat.get_keyboard() {
                        let s = state
                            .layout
                            .get_focused_window()
                            .and_then(|w| w.wl_surface())
                            .map(Cow::into_owned);
                        kbd.set_focus(state, s, SERIAL_COUNTER.next_serial());
                    }

                    {
                        let (renderer, mut framebuffer) = backend.bind().unwrap();

                        let mut render_elts = vec![];
                        let mut render_data =
                            RenderData::new(&mut state.space, &mut render_elts, renderer);
                        state.layout.render_to_space(&mut render_data);
                        let output = state.space.outputs().next().unwrap();
                        smithay::desktop::space::render_output::<
                            _,
                            WaylandSurfaceRenderElement<GlesRenderer>,
                            Window,
                            _,
                        >(
                            &output,
                            renderer,
                            &mut framebuffer,
                            0.9,
                            0,
                            [&state.space],
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
