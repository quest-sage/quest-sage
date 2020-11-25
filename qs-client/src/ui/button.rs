use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use stretch::{geometry::Size, result::Layout, style::Dimension};
use winit::event::{ElementState, MouseButton};

use crate::graphics::{MultiRenderable, NinePatch};

use super::{Colour, MouseInputProcessResult, UiElement};

pub struct Button {
    style: ButtonStyle,
    state: ButtonState,
    on_click: Box<dyn Fn() + Send + Sync + 'static>,
    disabled: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub struct ButtonStyle {
    /// The texture to be rendered when the button is not being held.
    pub released_texture: NinePatch,
    /// The texture to be rendered when the mouse is hovering over the button.
    pub hovered_texture: NinePatch,
    /// The texture to be rendered when the mouse is currently pressed on the button.
    pub pressed_texture: NinePatch,
    /// The texture to be rendered when the button is disabled, i.e. not clickable.
    pub disabled_texture: NinePatch,
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

        let nine_patch = if disabled {
            &self.style.disabled_texture
        } else {
            match self.state {
                ButtonState::Released => &self.style.released_texture,
                ButtonState::Hovered => &self.style.hovered_texture,
                ButtonState::Pressed => &self.style.pressed_texture,
                ButtonState::PressedNotHovered => &self.style.pressed_texture,
            }
        };
        nine_patch.generate_render_info(
            Colour::WHITE,
            layout.location.x,
            -layout.location.y - layout.size.height,
            layout.size.width,
            layout.size.height,
        )
    }

    fn process_mouse_input(&mut self, button: MouseButton, state: ElementState) -> MouseInputProcessResult {
        let disabled = self.disabled.load(Ordering::Relaxed);

        // The button takes keyboard focus so that other UI elements, for instance fields, are required to give up their focus
        // when the button is clicked.
        if let MouseButton::Left = button {
            match state {
                ElementState::Pressed => {
                    if self.state == ButtonState::Hovered {
                        if !disabled {
                            self.state = ButtonState::Pressed;
                        }
                        MouseInputProcessResult::TakeKeyboardFocus
                    } else {
                        MouseInputProcessResult::NotProcessed
                    }
                }
                ElementState::Released => {
                    if self.state == ButtonState::Pressed {
                        self.state = ButtonState::Hovered;
                        if !disabled {
                            let on_click = &self.on_click;
                            on_click();
                        }
                        MouseInputProcessResult::TakeKeyboardFocus
                    } else if self.state == ButtonState::PressedNotHovered {
                        self.state = ButtonState::Released;
                        MouseInputProcessResult::NotProcessed
                    } else {
                        MouseInputProcessResult::NotProcessed
                    }
                }
            }
        } else {
            MouseInputProcessResult::NotProcessed
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
