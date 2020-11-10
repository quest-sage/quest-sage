use std::sync::Arc;
use tokio::sync::RwLock;

use futures::future::{BoxFuture, FutureExt};
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

impl Widget {
    /// TODO remove this, just for debugging
    pub fn to_debug_string(&self) -> BoxFuture<String> {
        async move {
            let read = self.0.read().await;
            let mut children = String::new();
            for c in &read.children {
                children.push_str(&c.to_debug_string().await);
            }
            format!(
                "{{ {} {:#?}: {} }}",
                read.children.len(),
                read.layout,
                children
            )
        }
        .boxed()
    }
}

pub struct WidgetContents {
    element: Box<dyn UiElement>,
    /// This is the list of child widgets that will be laid out inside this widget in a non-overlapping way
    /// according to the flexbox style requirements.
    pub children: Vec<Widget>,
    /// The list of UI elements that will be rendered on sequential layers behind this one with the exact same
    /// layout. This is useful for creating backgrounds or highlights.
    backgrounds: Vec<Box<dyn UiElement>>,
    layout: Option<Layout>,
    style: Style,
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
}

impl Widget {
    /// Generates stretch node information for this node and children nodes.
    /// Returns the node for this widget, along with a map from child widgets to their information.
    fn generate_styles(&self) -> BoxFuture<WidgetStyle> {
        async move {
            let mut children = Vec::new();
            let style = self.0.read().await.get_style();
            let child_nodes = self.0.read().await.children.clone();
            for child in child_nodes {
                children.push(child.generate_styles().await);
            }

            WidgetStyle {
                widget: self.clone(),
                style,
                children,
            }
        }
        .boxed()
    }

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
        })))
    }

    /// Lays out this widget on a background thread. This does not block the caller.
    /// Call this on the root widget when you've changed something about this widget or a child widget.
    ///
    /// TODO: cancel the previous layout task if we've started another one. Maybe we should make some kind of primitive for this, it seems to be a common task.
    pub fn layout(&self, size: geometry::Size<Number>) {
        let clone = self.clone();
        tokio::task::spawn(async move {
            // Generate style information asynchronously, then process it synchronously.
            // Unfortunately, Stretch is !Send, so we can't just do this all at once.
            let styles: WidgetStyle = clone.generate_styles().await;

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
                let mut write = style.widget.0.write().await;
                write.layout = Some(layout);
            }
        });
    }

    pub fn generate_render_info(&self, offset: Point<f32>) -> BoxFuture<MultiRenderable> {
        async move {
            let read = self.0.read().await;
            if let Some(mut layout) = read.layout {
                let mut items = Vec::new();
                layout.location.x += offset.x;
                layout.location.y += offset.y;
                items.push(read.element.generate_render_info(&layout));
                for child in &read.children {
                    items.push(child.generate_render_info(layout.location).await);
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
        .boxed()
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
