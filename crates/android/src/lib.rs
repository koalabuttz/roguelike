//! Android/desktop frontend for the roguelike dungeon crawler.
//!
//! Phase 1: softbuffer pixel buffer rendering with colored rectangles,
//! tap-to-move input. Runs on desktop (winit window + mouse clicks)
//! and Android (NativeActivity + touch).
//!
//! # Desktop usage
//! ```sh
//! cargo run -p roguelike-android
//! ```
//!
//! # Android usage
//! ```sh
//! cargo apk2 build --release
//! ```

pub mod input;
pub mod render;

use std::num::NonZeroU32;
use std::sync::Arc;

use roguelike_core::data;
use roguelike_core::game_step::create_game;
use roguelike_core::rules::balance;

use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::EventLoop;
use winit::window::Window;

/// Active rendering surface, created on Resumed and dropped on Suspended.
struct RenderState {
    window: Arc<Window>,
    // Context must outlive Surface on some platforms.
    _context: Context<Arc<Window>>,
    surface: Surface<Arc<Window>, Arc<Window>>,
}

/// Application state machine.
struct App {
    game: Box<dyn roguelike_core::game_step::GameStep>,
    render: Option<RenderState>,
    /// Last known cursor position (desktop mouse input).
    last_cursor: Option<(f64, f64)>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.render.is_some() {
            return;
        }

        let attrs = Window::default_attributes()
            .with_title("Roguelike")
            .with_inner_size(PhysicalSize::new(960u32, 528u32));

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );

        let context = Context::new(window.clone()).expect("failed to create softbuffer context");
        let surface =
            Surface::new(&context, window.clone()).expect("failed to create softbuffer surface");

        self.render = Some(RenderState {
            window,
            _context: context,
            surface,
        });
        self.render.as_ref().unwrap().window.request_redraw();
    }

    fn suspended(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        // Drop the surface so Android can reclaim the native window.
        self.render = None;
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::RedrawRequested => {
                let Some(rs) = self.render.as_mut() else {
                    return;
                };
                let size = rs.window.inner_size();
                if size.width == 0 || size.height == 0 {
                    return;
                }

                rs.surface
                    .resize(
                        NonZeroU32::new(size.width).unwrap(),
                        NonZeroU32::new(size.height).unwrap(),
                    )
                    .expect("failed to resize surface");

                let mut buffer = rs.surface.buffer_mut().expect("failed to get buffer");
                render::render_frame(&mut buffer, size.width, size.height, self.game.as_ref());
                buffer.present().expect("failed to present buffer");
            }

            // Desktop: track cursor position for click-to-move.
            WindowEvent::CursorMoved { position, .. } => {
                self.last_cursor = Some((position.x, position.y));
            }

            // Desktop: translate mouse clicks to touch-style tap input.
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some((cx, cy)) = self.last_cursor {
                    self.handle_tap(cx, cy);
                }
            }

            // Android: handle touch events.
            WindowEvent::Touch(touch) => {
                if touch.phase == winit::event::TouchPhase::Started {
                    self.handle_tap(touch.location.x, touch.location.y);
                }
            }

            _ => {}
        }
    }
}

impl App {
    fn handle_tap(&mut self, x: f64, y: f64) {
        let Some(rs) = self.render.as_ref() else {
            return;
        };
        let size = rs.window.inner_size();
        if let Some(cmd) =
            input::touch_to_command(x, y, size.width, size.height, self.game.as_ref())
        {
            self.game.step_view(cmd);
            rs.window.request_redraw();
        }
    }
}

/// Run the game. Called from both desktop main() and android_main().
pub fn run(event_loop: EventLoop<()>) {
    let game_data = data::defaults();
    let game = create_game(
        100_000,
        balance::STANDARD_MAP_WIDTH as i32,
        balance::STANDARD_MAP_HEIGHT as i32,
        None,
        game_data,
    )
    .expect("failed to create game");

    let mut app = App {
        game,
        render: None,
        last_cursor: None,
    };

    event_loop.run_app(&mut app).expect("event loop failed");
}
