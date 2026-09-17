#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::sync::Arc;
use rqrr::PreparedImage;
use image::{GrayImage, RgbaImage};
use screenshots::Screen;
use pixels::{Pixels, SurfaceTexture};
use winit::{
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Fullscreen, Window, WindowId},
    application::ApplicationHandler,
};
use arboard::Clipboard;

fn capture_screen() -> Result<RgbaImage, Box<dyn std::error::Error>> {
    let screen = Screen::all()?
        .into_iter()
        .next()
        .ok_or("No screens found")?;

    let image = screen.capture()?;

    let width = image.width();
    let height = image.height();
    let data = image.into_raw();

    RgbaImage::from_raw(width, height, data)
        .ok_or_else(|| "Invalid screenshot buffer".into())
}

fn crop_selection(image: &RgbaImage, rect: (u32, u32, u32, u32)) -> RgbaImage {
    let (x, y, width, height) = rect;

    image::imageops::crop_imm(image, x, y, width, height).to_image()
}

fn save_selection(
    image: &RgbaImage,
    rect: (u32, u32, u32, u32),
) -> Result<(), image::ImageError> {
    let (x, y, width, height) = rect;

    let cropped = image::imageops::crop_imm(
        image,
        x,
        y,
        width,
        height,
    )
    .to_image();

    cropped.save("qr_capture.png")?;

    Ok(())
}

fn decode_qr(image: &RgbaImage) -> Result<String, String> {
    let gray: GrayImage = image::imageops::grayscale(image);

    let mut prepared = PreparedImage::prepare(gray);

    let grids = prepared.detect_grids();

    if grids.is_empty() {
        return Err("No QR code found".into());
    }

    for grid in grids {
        match grid.decode() {
            Ok((_, content)) => return Ok(content),
            Err(_) => continue,
        }
    }

    Err("Failed to decode QR code".into())
}

fn darken_image(mut image: RgbaImage) -> RgbaImage {
    for pixel in image.pixels_mut() {
        pixel[0] = (pixel[0] as f32 * 0.35) as u8;
        pixel[1] = (pixel[1] as f32 * 0.35) as u8;
        pixel[2] = (pixel[2] as f32 * 0.35) as u8;
    }

    image
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionState {
    Idle,
    Selecting,
    Complete,
}

struct App {
    original: RgbaImage,
    darkened: RgbaImage,

    cursor_x: f32,
    cursor_y: f32,

    start_x: f32,
    start_y: f32,

    end_x: f32,
    end_y: f32,

    state: SelectionState,
}

impl App {
    fn new(original: RgbaImage) -> Self {
        let darkened = darken_image(original.clone());

        Self {
            original,
            darkened,

            cursor_x: 0.0,
            cursor_y: 0.0,

            start_x: 0.0,
            start_y: 0.0,

            end_x: 0.0,
            end_y: 0.0,

            state: SelectionState::Idle,
        }
    }

    fn selection_rect(&self) -> (u32, u32, u32, u32) {
        let x1 = self.start_x.min(self.end_x);
        let y1 = self.start_y.min(self.end_y);

        let x2 = self.start_x.max(self.end_x);
        let y2 = self.start_y.max(self.end_y);

        (
            x1 as u32,
            y1 as u32,
            (x2 - x1) as u32,
            (y2 - y1) as u32,
        )
    }

    fn has_valid_selection(&self) -> bool {
        let (_, _, width, height) = self.selection_rect();

        width >= 5 && height >= 5
    }
}

fn copy_image_to_frame(image: &RgbaImage, frame: &mut [u8]) {
    frame.copy_from_slice(image.as_raw());
}

struct Overlay {
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    app: App,
}

impl Overlay {
    fn render(&mut self) {
        let pixels = match self.pixels.as_mut() {
            Some(pixels) => pixels,
            None => return,
        };

        let frame = pixels.frame_mut();

        copy_image_to_frame(&self.app.darkened, frame);

        if self.app.state == SelectionState::Selecting || self.app.state == SelectionState::Complete
        {
            let (x, y, width, height) = self.app.selection_rect();

            let screen_width = self.app.original.width();

            for row in 0..height {
                let src_start = ((y + row) * screen_width + x) as usize * 4;

                let src_end = src_start + (width as usize * 4);

                let dst_start = src_start;

                let dst_end = dst_start + (width as usize * 4);

                if src_end <= self.app.original.as_raw().len() && dst_end <= frame.len() {
                    frame[dst_start..dst_end]
                        .copy_from_slice(&self.app.original.as_raw()[src_start..src_end]);
                }
            }
        }

        if let Err(error) = pixels.render() {
            eprintln!("Render error: {error}");
        }
    }
}

impl ApplicationHandler for Overlay {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let width = self.app.original.width();
        let height = self.app.original.height();

        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_visible(false)
                        .with_decorations(false)
                        .with_resizable(false)
                        .with_fullscreen(Some(Fullscreen::Borderless(None))),
                )
                .unwrap(),
        );

        let surface_texture = SurfaceTexture::new(width, height, window.clone());

        let pixels = Pixels::new(width, height, surface_texture).unwrap();

        self.pixels = Some(pixels);
        self.window = Some(window.clone());

        {
            let pixels = self.pixels.as_mut().unwrap();
            let frame = pixels.frame_mut();

            copy_image_to_frame(&self.app.darkened, frame);

            pixels.render().unwrap();
        }

        window.set_visible(true);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::Resized(size) => {
                if let Some(pixels) = self.pixels.as_mut() {
                    if let Err(error) = pixels.resize_surface(size.width, size.height) {
                        eprintln!("Failed to resize surface: {error}");
                    }
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.app.cursor_x = position.x as f32;
                self.app.cursor_y = position.y as f32;

                if self.app.state == SelectionState::Selecting {
                    self.app.end_x = self.app.cursor_x;
                    self.app.end_y = self.app.cursor_y;

                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }

            WindowEvent::MouseInput {
                state,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                match state {
                    winit::event::ElementState::Pressed => {
                        self.app.start_x = self.app.cursor_x;
                        self.app.start_y = self.app.cursor_y;

                        self.app.end_x = self.app.cursor_x;
                        self.app.end_y = self.app.cursor_y;

                        self.app.state = SelectionState::Selecting;

                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }

                    winit::event::ElementState::Released => {
                        if self.app.state != SelectionState::Selecting {
                            return;
                        }

                        self.app.end_x = self.app.cursor_x;
                        self.app.end_y = self.app.cursor_y;

                        if !self.app.has_valid_selection() {
                            self.app.state = SelectionState::Idle;
                            return;
                        }

                        let rect = self.app.selection_rect();

                        println!(
                            "Selection: x={}, y={}, width={}, height={}",
                            rect.0, rect.1, rect.2, rect.3
                        );

                        match save_selection(&self.app.original, rect) {
                            Ok(()) => println!("Saved crop to qr_capture.png"),
                            Err(error) => eprintln!("Failed to save crop: {error}"),
                        }

                        let cropped = crop_selection(&self.app.original, rect);

                        match decode_qr(&cropped) {
                            Ok(content) => {
                                match Clipboard::new() {
                                    Ok(mut clipboard) => {
                                        match clipboard.set_text(&content) {
                                            Ok(()) => {
                                                println!("Copied QR code to clipboard");
                                            }
                                            Err(error) => {
                                                eprintln!("Failed to copy QR code: {error}");
                                            }
                                        }
                                    }

                                    Err(error) => {
                                        eprintln!("Failed to access clipboard: {error}");
                                    }
                                }
                            }

                            Err(error) => {
                                println!("QR decode failed: {error}");
                            }
                        }

                        event_loop.exit();
                    }
                }
            },

            WindowEvent::RedrawRequested => {
                self.render();
            }

            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let screenshot = capture_screen()?;
    let app = App::new(screenshot);

    let event_loop = EventLoop::new()?;

    let mut overlay = Overlay {
        window: None,
        pixels: None,
        app,
    };

    event_loop.run_app(&mut overlay)?;

    Ok(())
}
