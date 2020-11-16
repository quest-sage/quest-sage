use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use qs_common::assets::Asset;
use stretch::{geometry::Size, result::Layout, style::Dimension};
use winit::event::{ElementState, MouseButton};

use crate::graphics::{MultiRenderable, Renderable, Texture, Vertex};

use super::{Colour, UiElement};

pub struct Button {
    style: ButtonStyle,
    state: ButtonState,
    on_click: Box<dyn Fn() + Send + Sync + 'static>,
    disabled: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub struct ButtonStyle {
    /// The texture to be rendered when the button is not being held.
    pub released_texture: Asset<Texture>,
    /// The texture to be rendered when the mouse is hovering over the button.
    pub hovered_texture: Asset<Texture>,
    /// The texture to be rendered when the mouse is currently pressed on the button.
    pub pressed_texture: Asset<Texture>,
    /// The texture to be rendered when the button is disabled, i.e. not clickable.
    pub disabled_texture: Asset<Texture>,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum ButtonState {
    Released,
    Hovered,
    Pressed,
    PressedNotHovered,
}

impl Button {
    pub fn new(style: ButtonStyle, on_click: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            style,
            state: ButtonState::Released,
            on_click: Box::new(on_click),
            disabled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// If `disabled` is ever set to `true`, the button will not be clickable.
    pub fn new_disableable(
        style: ButtonStyle,
        on_click: impl Fn() + Send + Sync + 'static,
        disabled: Arc<AtomicBool>,
    ) -> Self {
        Self {
            style,
            state: ButtonState::Released,
            on_click: Box::new(on_click),
            disabled,
        }
    }
}

impl UiElement for Button {
    fn get_size(&self) -> Size<Dimension> {
        Size {
            width: Dimension::Auto,
            height: Dimension::Auto,
        }
    }

    fn generate_render_info(&self, layout: &Layout) -> MultiRenderable {
        let disabled = self.disabled.load(Ordering::Relaxed);

        let color = Colour::WHITE.into();
        MultiRenderable::Image {
            texture: if disabled {
                self.style.disabled_texture.clone()
            } else {
                match self.state {
                    ButtonState::Released => self.style.released_texture.clone(),
                    ButtonState::Hovered => self.style.hovered_texture.clone(),
                    ButtonState::Pressed => self.style.pressed_texture.clone(),
                    ButtonState::PressedNotHovered => self.style.pressed_texture.clone(),
                }
            },
            renderables: vec![Renderable::Quadrilateral(
                Vertex {
                    position: [layout.location.x, -layout.location.y, 0.0],
                    color,
                    tex_coords: [0.0, 0.0],
                },
                Vertex {
                    position: [
                        layout.location.x + layout.size.width,
                        -layout.location.y,
                        0.0,
                    ],
                    color,
                    tex_coords: [1.0, 0.0],
                },
                Vertex {
                    position: [
                        layout.location.x + layout.size.width,
                        -layout.location.y - layout.size.height,
                        0.0,
                    ],
                    color,
                    tex_coords: [1.0, 1.0],
                },
                Vertex {
                    position: [
                        layout.location.x,
                        -layout.location.y - layout.size.height,
                        0.0,
                    ],
                    color,
                    tex_coords: [0.0, 1.0],
                },
            )],
        }
    }

    fn process_mouse_input(&mut self, button: MouseButton, state: ElementState) -> bool {
        let disabled = self.disabled.load(Ordering::Relaxed);

        if let MouseButton::Left = button {
            match state {
                ElementState::Pressed => {
                    if self.state == ButtonState::Hovered {
                        if !disabled {
                            self.state = ButtonState::Pressed;
                        }
                        true
                    } else {
                        false
                    }
                }
                ElementState::Released => {
                    if self.state == ButtonState::Pressed {
                        self.state = ButtonState::Hovered;
                        if !disabled {
                            let on_click = &self.on_click;
                            on_click();
                        }
                        true
                    } else if self.state == ButtonState::PressedNotHovered {
                        self.state = ButtonState::Released;
                        false
                    } else {
                        false
                    }
                }
            }
        } else {
            false
        }
    }

    fn mouse_enter(&mut self) {
        if self.state == ButtonState::Released {
            self.state = ButtonState::Hovered;
        } else if self.state == ButtonState::PressedNotHovered {
            self.state = ButtonState::Pressed;
        }
    }

    fn mouse_leave(&mut self) {
        if self.state == ButtonState::Hovered {
            self.state = ButtonState::Released;
        } else if self.state == ButtonState::Pressed {
            self.state = ButtonState::PressedNotHovered;
        }
    }
}
