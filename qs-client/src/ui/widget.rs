use qs_common::assets::Asset;
use std::sync::{Arc, RwLock, atomic::AtomicBool, atomic::Ordering, Weak};

use stretch::{
    geometry, geometry::Point, geometry::Size, node::Node, node::Stretch, number::Number,
    result::Layout, style::Dimension, style::Style,
};

use crate::graphics::*;

/// A UI element is an item in a UI that has a size and can be rendered.
pub trait UiElement: Send + Sync {
    /// When laying out this UI element inside a widget, what should its size be?
    /// This is allowed to be asynchronous; for example, a text asset must wait
    /// for the font to load before this can be calculated.
    fn get_size(&self) -> Size<Dimension>;

    /// Generates information about how to render this widget, based on the calculated layout info.
    /// Asynchronous, asset-based information must be called on a background task and just used here.
    fn generate_render_info(&self, layout: &Layout) -> MultiRenderable;
}

impl UiElement for () {
    fn get_size(&self) -> Size<Dimension> {
        Size {
            width: Dimension::Auto,
            height: Dimension::Auto,
        }
    }

    fn generate_render_info(&self, _layout: &Layout) -> MultiRenderable {
        MultiRenderable::Nothing
    }
}

/// A widget is some UI element together with a list of children that can be laid out according to flexbox rules.
/// You can clone the widget to get another reference to the same widget.
#[derive(Clone)]
pub struct Widget(pub Arc<RwLock<WidgetContents>>);

pub struct WidgetContents {
    element: Box<dyn UiElement>,
    /// This is the list of child widgets that will be laid out inside this widget in a non-overlapping way
    /// according to the flexbox style requirements.
    children: Vec<Widget>,
    /// The list of UI elements that will be rendered on sequential layers behind this one with the exact same
    /// layout. This is useful for creating backgrounds or highlights.
    backgrounds: Vec<Box<dyn UiElement>>,
    layout: Option<Layout>,
    style: Style,

    /// When we want to update the UI's layout (e.g. after changing some setting like text contents),
    /// we will set this value to true. If the `Weak` cannot be upgraded, then the UI has been dropped, or
    /// this widget has not been added to a UI yet.
    force_layout_signal: Weak<AtomicBool>,
}

/// Temporarily contains style information about a widget so we can lay it out.
struct WidgetStyle {
    widget: Widget,
    style: Style,
    children: Vec<WidgetStyle>,
}

impl WidgetContents {
    fn get_style(&self) -> Style {
        Style {
            size: self.element.get_size(),
            ..self.style
        }
    }

    /// Request that the UI updates the layout next time we render it.
    pub fn force_layout(&self) {
        if let Some(signal) = self.force_layout_signal.upgrade() {
            signal.store(true, Ordering::Relaxed);
        }
        // Otherwise, the widget was not part of a UI, or the UI containing this widget was dropped
    }

    pub fn add_child(&mut self, widget: Widget) {
        widget.update_force_layout_signal(Weak::clone(&self.force_layout_signal));
        self.children.push(widget);
        self.force_layout();
    }

    pub fn clear_children(&mut self) {
        self.children.clear();
        self.force_layout();
    }
}

impl Widget {
    pub fn new(
        element: impl UiElement + 'static,
        children: Vec<Widget>,
        backgrounds: Vec<Box<dyn UiElement>>,
        style: Style,
    ) -> Self {
        Self(Arc::new(RwLock::new(WidgetContents {
            element: Box::new(element),
            children,
            backgrounds,
            layout: None,
            style,
            force_layout_signal: Weak::new(),
        })))
    }

    fn update_force_layout_signal(&self, force_layout_signal: Weak<AtomicBool>) {
        let mut write = self.0.write().unwrap();
        for child in &write.children {
            child.update_force_layout_signal(Weak::clone(&force_layout_signal));
        }
        write.force_layout_signal = force_layout_signal;
    }

    /// Generates stretch node information for this node and children nodes.
    /// Returns the node for this widget, along with a map from child widgets to their information.
    fn generate_styles(&self) -> WidgetStyle {
        let mut children = Vec::new();
        let read = self.0.read().unwrap();
        let style = read.get_style();
        let child_nodes = read.children.clone();
        for child in child_nodes {
            children.push(child.generate_styles());
        }

        WidgetStyle {
            widget: self.clone(),
            style,
            children,
        }
    }

    /// Generates a `MultiRenderable` so that we can render this widget.
    ///
    /// Y coordinates are typically reversed in this method; the flexbox library expects Y to increase in the downwards direction
    /// but our render expects Y to increase in the upwards direction.
    ///
    /// If render_debug is a texture, additional lines will be drawn using this texture for debug information for each
    /// child widget.
    fn generate_render_info(
        &self,
        offset: Point<f32>,
        debug_line_texture: Option<Asset<Texture>>,
    ) -> MultiRenderable {
        let read = self.0.read().unwrap();
        if let Some(mut layout) = read.layout {
            let mut items = Vec::new();
            layout.location.x += offset.x;
            layout.location.y += offset.y;
            items.push(read.element.generate_render_info(&layout));
            for child in &read.children {
                items.push(
                    child
                        .generate_render_info(layout.location, debug_line_texture.clone()),
                );
            }

            if let Some(debug_line_texture) = debug_line_texture {
                let (x0, y0) = (layout.location.x, -layout.location.y);
                let (x1, y1) = (
                    layout.location.x + layout.size.width,
                    -layout.location.y - layout.size.height,
                );
                const SIZE: f32 = 1.0;
                // Create four lines of the given thickness (`SIZE`) to surround the widget.
                let color = super::Colour {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                }
                .into();
                let tex_coords = [0.0, 0.0];
                items.push(MultiRenderable::Image {
                    texture: debug_line_texture,
                    renderables: vec![
                        Renderable::Quadrilateral(
                            Vertex {
                                position: [x0, y0, 0.0],
                                color,
                                tex_coords,
                            },
                            Vertex {
                                position: [x0 + SIZE, y0, 0.0],
                                color,
                                tex_coords,
                            },
                            Vertex {
                                position: [x0 + SIZE, y1, 0.0],
                                color,
                                tex_coords,
                            },
                            Vertex {
                                position: [x0, y1, 0.0],
                                color,
                                tex_coords,
                            },
                        ),
                        Renderable::Quadrilateral(
                            Vertex {
                                position: [x1, y0, 0.0],
                                color,
                                tex_coords,
                            },
                            Vertex {
                                position: [x1 - SIZE, y0, 0.0],
                                color,
                                tex_coords,
                            },
                            Vertex {
                                position: [x1 - SIZE, y1, 0.0],
                                color,
                                tex_coords,
                            },
                            Vertex {
                                position: [x1, y1, 0.0],
                                color,
                                tex_coords,
                            },
                        ),
                        Renderable::Quadrilateral(
                            Vertex {
                                position: [x0, y0, 0.0],
                                color,
                                tex_coords,
                            },
                            Vertex {
                                position: [x0, y0 + SIZE, 0.0],
                                color,
                                tex_coords,
                            },
                            Vertex {
                                position: [x1, y0 + SIZE, 0.0],
                                color,
                                tex_coords,
                            },
                            Vertex {
                                position: [x1, y0, 0.0],
                                color,
                                tex_coords,
                            },
                        ),
                        Renderable::Quadrilateral(
                            Vertex {
                                position: [x0, y1, 0.0],
                                color,
                                tex_coords,
                            },
                            Vertex {
                                position: [x0, y1 - SIZE, 0.0],
                                color,
                                tex_coords,
                            },
                            Vertex {
                                position: [x1, y1 - SIZE, 0.0],
                                color,
                                tex_coords,
                            },
                            Vertex {
                                position: [x1, y1, 0.0],
                                color,
                                tex_coords,
                            },
                        ),
                    ],
                })
            }

            let renderable = MultiRenderable::Adjacent(items);

            if read.backgrounds.is_empty() {
                renderable
            } else {
                let mut layers = Vec::new();
                for background in &read.backgrounds {
                    layers.push(background.generate_render_info(&layout));
                }
                layers.push(renderable);
                MultiRenderable::Layered(layers)
            }
        } else {
            MultiRenderable::Nothing
        }
    }
}

/// Represents an entire user interface. Holds a root widget.
pub struct UI {
    root: Widget,
    size: Size<Number>,
    /// When a child widget calls `force_layout`, it updates this value to true.
    /// This forces the UI to recalculate its layout before its next render.
    force_layout: Arc<AtomicBool>,
}

impl UI {
    pub fn new(root: Widget, size: Size<Number>) -> Self {
        let force_layout = Arc::new(AtomicBool::new(true));
        root.update_force_layout_signal(Arc::downgrade(&force_layout));
        Self {
            root,
            size,
            force_layout,
        }
    }

    pub fn update_size(&mut self, size: Size<Number>) {
        self.size = size;
        self.force_layout.store(true, Ordering::Relaxed);
    }

    /// Generates a `MultiRenderable` so that we can render this UI.
    ///
    /// Y coordinates are typically reversed in this method; the flexbox library expects Y to increase in the downwards direction
    /// but our render expects Y to increase in the upwards direction.
    ///
    /// If render_debug is a texture, additional lines will be drawn using this texture for debug information for each
    /// child widget.
    ///
    /// If `force_layout` has been called by a child UI element, the UI layout will be recalculated first.
    pub fn generate_render_info(
        &self,
        offset: Point<f32>,
        debug_line_texture: Option<Asset<Texture>>,
    ) -> MultiRenderable {
        self.layout(self.size);
        self.root.generate_render_info(offset, debug_line_texture)
    }

    /// Lays out this UI according to flexbox rules.
    /// This is called when we want to render this UI but the layout has been invalidated by
    /// changing some content in a child widget or UI element.
    fn layout(&self, size: geometry::Size<Number>) {
        let styles: WidgetStyle = self.root.generate_styles();

        let layouts: Vec<_> = {
            let mut stretch = Stretch::new();
            let (node, nodes) = generate_nodes(&mut stretch, &styles);
            stretch
                .compute_layout(node, size)
                .expect("could not layout");
            nodes
                .into_iter()
                .map(|(style, node)| {
                    (style, *stretch.layout(node).expect("could not get layout"))
                })
                .collect()
        };

        for (style, layout) in layouts {
            let mut write = style.widget.0.write().unwrap();
            write.layout = Some(layout);
        }
    }
}

/// Returns the node corresponding to this widget, along with a vector containing all child widget styles and their nodes.
/// This vector notably includes the current node that was returned as the first return value.
fn generate_nodes<'a>(
    stretch: &mut Stretch,
    widget_style: &'a WidgetStyle,
) -> (Node, Vec<(&'a WidgetStyle, Node)>) {
    let mut children = Vec::new();
    let mut child_nodes = Vec::new();
    for child in &widget_style.children {
        let (node, mut new_child_nodes) = generate_nodes(stretch, child);
        children.push(node);
        child_nodes.append(&mut new_child_nodes);
    }
    let node = stretch
        .new_node(widget_style.style, children)
        .expect("could not add node");
    child_nodes.push((widget_style, node));
    (node, child_nodes)
}
