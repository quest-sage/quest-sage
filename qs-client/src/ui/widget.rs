use std::sync::Arc;
use tokio::sync::RwLock;

use futures::future::{BoxFuture, FutureExt};
use stretch::{
    geometry, geometry::Size, node::Node, node::Stretch, number::Number, result::Layout,
    style::Dimension, style::Style,
};

use crate::graphics::*;

/// A UI element is an item in a UI that has a size and can be rendered.
#[async_trait::async_trait]
pub trait UiElement: Send + Sync {
    /// When laying out this UI element inside a widget, what should its size be?
    /// This is allowed to be asynchronous; for example, a text asset must wait
    /// for the font to load before this can be calculated.
    async fn get_size(&self) -> Size<Dimension>;

    /// Generates information about how to render this widget, based on the calculated layout info.
    /// Asynchronous, asset-based information must be called on a background task and just used here.
    fn generate_render_info(&self, layout: &Layout) -> MultiRenderable;
}

/// A widget is some UI element together with a list of children that can be laid out according to flexbox rules.
/// You can clone the widget to get another reference to the same widget.
#[derive(Clone)]
pub struct Widget(Arc<RwLock<WidgetContents>>);

struct WidgetContents {
    element: Box<dyn UiElement>,
    children: Vec<Widget>,
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
    async fn get_style(&self) -> Style {
        Style {
            size: self.element.get_size().await,
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
            let style = self.0.read().await.get_style().await;
            let child_nodes = self.0.read().await.children.clone();
            for child in child_nodes {
                children.push(child.generate_styles().await);
            }
            
            WidgetStyle {
                widget: self.clone(),
                style,
                children,
            }
        }.boxed()
    }

    pub fn new(element: impl UiElement + 'static, children: Vec<Widget>, style: Style) -> Self {
        Self(Arc::new(RwLock::new(WidgetContents {
            element: Box::new(element),
            children,
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
                stretch.compute_layout(node, size).expect("could not layout");
                nodes.into_iter().map(|(style, node)| (style, *stretch.layout(node).expect("could not get layout"))).collect()
            };

            for (style, layout) in layouts {
                style.widget.0.write().await.layout = Some(layout);
                // TODO let the widget respond to this layout change.
            }
        });
    }

    pub async fn generate_render_info(&self) -> MultiRenderable {
        let read = self.0.read().await;
        if let Some(layout) = read.layout {
            read.element.generate_render_info(&layout)
        } else {
            MultiRenderable::Nothing
        }
    }
}

/// Returns the node corresponding to this widget, along with a vector containing all child widget styles and their nodes.
/// This vector notably includes the current node that was returned as the first return value.
fn generate_nodes<'a>(stretch: &mut Stretch, widget_style: &'a WidgetStyle) -> (Node, Vec<(&'a WidgetStyle, Node)>) {
    let mut children = Vec::new();
    let mut child_nodes = Vec::new();
    for child in &widget_style.children {
        let (node, mut new_child_nodes) = generate_nodes(stretch, child);
        children.push(node);
        child_nodes.append(&mut new_child_nodes);
    }
    let node = stretch.new_node(widget_style.style, children).expect("could not add node");
    child_nodes.push((widget_style, node));
    (node, child_nodes)
}
