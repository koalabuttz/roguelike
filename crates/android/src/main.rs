//! Desktop entry point for development and testing.
//!
//! Opens a winit window with mouse-click-as-touch input.
//! The same game logic runs identically on Android via android_main.

fn main() {
    let event_loop = winit::event_loop::EventLoop::new().expect("failed to create event loop");
    roguelike_android::run(event_loop);
}
