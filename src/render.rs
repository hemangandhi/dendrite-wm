use smithay::backend::renderer::element::AsRenderElements;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::desktop::{Space, Window};
use smithay::utils::{Logical, Point, Rectangle, Size};
use smithay::wayland::seat::WaylandFocus;

pub struct RenderData<'a> {
    space: &'a mut Space<Window>,
    output_elements: &'a mut Vec<WaylandSurfaceRenderElement<GlesRenderer>>,
    renderer: &'a mut GlesRenderer,
    scale: f64,
}

impl<'a> RenderData<'a> {
    pub fn new(
        space: &'a mut Space<Window>,
        output_elements: &'a mut Vec<WaylandSurfaceRenderElement<GlesRenderer>>,
        renderer: &'a mut GlesRenderer,
    ) -> Self {
        let scale = space
            .outputs()
            .next()
            .unwrap()
            .current_scale()
            .fractional_scale();
        Self {
            space,
            output_elements,
            renderer,
            scale,
        }
    }

    pub fn unmap(&mut self, window: &Window) {
        self.space.unmap_elem(window);
    }

    pub fn render_or_map(&mut self, window: &Window, coords: Point<i32, Logical>, active: bool) {
        self.space.map_element(window.clone(), coords, active);
        if active {
            self.output_elements.extend(window.render_elements(
                self.renderer,
                coords.to_physical_precise_round(self.scale),
                self.scale.into(),
                1.0,
            ))
        }
    }
}

pub trait RenderableElement {
    type SurfaceType;
    type TopLevelSurfaceType;

    fn send_close(&self);
    fn unmap<'a>(&self, renderer: &mut RenderData<'a>);
    fn render_or_map<'a>(
        &self,
        renderer: &mut RenderData<'a>,
        coords: Point<i32, Logical>,
        active: bool,
        z_index: u8,
    );
    fn contains_surface(&self, s: &Self::SurfaceType) -> bool;
    fn matches(&self, other: &Self) -> bool;

    fn from_toplevel(size: Size<i32, Logical>, t: Self::TopLevelSurfaceType) -> Self;

    fn to_surface(&self) -> Option<Self::SurfaceType>;
}

impl RenderableElement for Window {
    type SurfaceType = smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
    type TopLevelSurfaceType = smithay::wayland::shell::xdg::ToplevelSurface;

    fn send_close(&self) {
        if let Some(tl) = self.toplevel() {
            tl.send_close();
        } else {
            tracing::warn!("Couldn't get toplevel to send_close!");
        }
    }

    fn unmap<'a>(&self, renderer: &mut RenderData<'a>) {
        renderer.unmap(self);
    }

    fn render_or_map<'a>(
        &self,
        renderer: &mut RenderData<'a>,
        coords: Point<i32, Logical>,
        active: bool,
        z_index: u8,
    ) {
        self.override_z_index(z_index);
        renderer.space.map_element(self.clone(), coords, active);
        if active {
            renderer.output_elements.extend(self.render_elements(
                renderer.renderer,
                coords.to_physical_precise_round(renderer.scale),
                renderer.scale.into(),
                1.0,
            ));
        }
    }

    fn contains_surface(&self, other: &Self::SurfaceType) -> bool {
        self.wl_surface().map(|s| *s == *other).unwrap_or(false)
    }

    fn matches(&self, other: &Self) -> bool {
        other
            .wl_surface()
            .map(|s| self.contains_surface(&*s))
            .unwrap_or(false)
    }

    fn from_toplevel(size: Size<i32, Logical>, surface: Self::TopLevelSurfaceType) -> Self {
        surface.with_pending_state(|p| {
            p.bounds = Some(size);
            p.size = Some(size)
        });
        surface.send_pending_configure();
        return Window::new_wayland_window(surface);
    }

    fn to_surface(&self) -> Option<Self::SurfaceType> {
        self.wl_surface().map(move |c| (*c).clone())
    }
}

#[cfg(test)]
pub mod test_render {
    use std::cell::RefCell;

    use smithay::utils::{Logical, Point, Size};

    #[derive(Clone, Default, Eq, PartialEq)]
    pub struct MappingData {
        pub coords: Point<i32, Logical>,
        pub active: bool,
        pub z_index: u8,
    }

    #[derive(Clone, Default, Eq, PartialEq)]
    pub struct TestRenderElement {
        pub id: u32,
        pub got_close: RefCell<bool>,
        pub was_unmapped: RefCell<bool>,
        pub mapped_locs: RefCell<Vec<MappingData>>,
        pub size: Size<i32, Logical>,
    }

    impl TestRenderElement {
        pub fn with_id(id: u32) -> Self {
            Self {
                id,
                ..Default::default()
            }
        }
    }

    impl super::RenderableElement for TestRenderElement {
        type SurfaceType = Self;
        type TopLevelSurfaceType = Self;

        fn send_close(&self) {
            self.got_close.swap(&true.into());
        }

        fn unmap<'a>(&self, _renderer: &mut super::RenderData<'a>) {
            self.was_unmapped.swap(&true.into());
        }

        fn render_or_map<'a>(
            &self,
            _renderer: &mut super::RenderData<'a>,
            coords: Point<i32, Logical>,
            active: bool,
            z_index: u8,
        ) {
            self.mapped_locs.borrow_mut().push(MappingData {
                coords,
                active,
                z_index,
            });
        }

        fn contains_surface(&self, s: &Self::SurfaceType) -> bool {
            self.id == s.id
        }

        fn matches(&self, other: &Self) -> bool {
            self.id == other.id
        }

        fn from_toplevel(size: Size<i32, Logical>, t: Self::TopLevelSurfaceType) -> Self {
            Self { size: size, ..t }
        }

        fn to_surface(&self) -> Option<Self::SurfaceType> {
            Some(self.clone())
        }
    }
}
