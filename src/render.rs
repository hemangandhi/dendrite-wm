use smithay::backend::renderer::element::AsRenderElements;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::desktop::{Space, Window};
use smithay::utils::{Logical, Point, Rectangle, Size};

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
        scale: f64,
    ) -> Self {
        Self {
            space,
            output_elements,
            renderer,
            scale,
        }
    }

    pub fn render_or_map(&mut self, window: &Window, coords: Point<i32, Logical>, active: bool) {
        if active {
            self.output_elements.extend(window.render_elements(
                self.renderer,
                coords.to_physical_precise_round(self.scale),
                self.scale.into(),
                1.0,
            ))
        }
        // self.space.unmap_elem(window); // TODO: needed?
        self.space.map_element(window.clone(), coords, active);
    }
}
