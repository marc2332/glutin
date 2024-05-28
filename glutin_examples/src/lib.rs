use std::error::Error;
use std::ffi::{CStr, CString};
use std::num::NonZeroU32;
use std::ops::Deref;

use gl::types::GLfloat;
use raw_window_handle::HasWindowHandle;
use winit::application::ApplicationHandler;
use winit::event::{KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use glutin::config::{Config, ConfigTemplateBuilder};
use glutin::context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{Surface, SwapInterval, WindowSurface};

use glutin_winit::{self, DisplayBuilder, GlWindow};

pub mod gl {
    #![allow(clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));

    pub use Gles2 as Gl;
}

pub fn main(event_loop: EventLoop<()>) -> Result<(), Box<dyn Error>> {
    let mut app = Application::default();

    event_loop.run_app(&mut app)?;

    Ok(())
}

#[derive(Default)]
struct Application {
    // Renderer must be dropped the first as it uses the GL states from below.
    renderer: Option<Renderer>,
    gl_context: Option<PossiblyCurrentContext>,
    gl_surface: Option<Surface<WindowSurface>>,
    gl_config: Option<Config>,
    // Window must be dropped at the end.
    window: Option<Window>,
}

impl Application {
    fn create_window(&mut self, active_event_loop: &ActiveEventLoop) {
        // Only Windows requires the window to be present before creating the display.
        // Other platforms don't really need one.
        let window_attributes = Window::default_attributes()
            .with_transparent(true)
            .with_title("Glutin triangle gradient example (press Escape to exit)");

        // The template will match only the configurations supporting rendering
        // to windows.
        //
        // XXX We force transparency only on macOS, given that EGL on X11 doesn't
        // have it, but we still want to show window. The macOS situation is like
        // that, because we can query only one config at a time on it, but all
        // normal platforms will return multiple configs, so we can find the config
        // with transparency ourselves inside the `reduce`.
        let template =
            ConfigTemplateBuilder::new().with_alpha_size(8).with_transparency(cfg!(cgl_backend));

        let display_builder = DisplayBuilder::new().with_window_attributes(Some(window_attributes));

        let (window, gl_config) = display_builder
            .build(active_event_loop, template, gl_config_picker)
            .expect("Failed to create Window.");

        println!("Picked a config with {} samples", gl_config.num_samples());

        let raw_window_handle = window
            .as_ref()
            .and_then(|window| window.window_handle().ok())
            .map(|handle| handle.as_raw());

        // XXX The display could be obtained from any object created by it, so we can
        // query it from the config.
        let gl_display = gl_config.display();

        // The context creation part.
        let context_attributes = ContextAttributesBuilder::new().build(raw_window_handle);

        // Since glutin by default tries to create OpenGL core context, which may not be
        // present we should try gles.
        let fallback_context_attributes = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::Gles(None))
            .build(raw_window_handle);

        // There are also some old devices that support neither modern OpenGL nor GLES.
        // To support these we can try and create a 2.1 context.
        let legacy_context_attributes = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(2, 1))))
            .build(raw_window_handle);

        let not_current_gl_context = unsafe {
            gl_display.create_context(&gl_config, &context_attributes).unwrap_or_else(|_| {
                gl_display.create_context(&gl_config, &fallback_context_attributes).unwrap_or_else(
                    |_| {
                        gl_display
                            .create_context(&gl_config, &legacy_context_attributes)
                            .expect("failed to create context")
                    },
                )
            })
        };

        self.window = window;
        self.gl_config = Some(gl_config);
        self.gl_context = Some(not_current_gl_context.treat_as_possibly_current());
    }
}

impl ApplicationHandler<()> for Application {
    fn resumed(&mut self, active_event_loop: &ActiveEventLoop) {
        #[cfg(android_platform)]
        println!("Android window available");

        let (gl_config, window) =
            if let Some(state) = self.gl_config.as_ref().zip(self.window.as_ref()) {
                state
            } else {
                self.create_window(active_event_loop);

                (self.gl_config.as_ref().unwrap(), self.window.as_ref().unwrap())
            };

        let attrs = window.build_surface_attributes(Default::default()).unwrap();
        let gl_surface =
            unsafe { gl_config.display().create_window_surface(gl_config, &attrs).unwrap() };

        let gl_context = self.gl_context.as_ref().unwrap();

        // Make it current.
        gl_context.make_current(&gl_surface).unwrap();

        // The context needs to be current for the Renderer to set up shaders and
        // buffers. It also performs function loading, which needs a current context on
        // WGL.
        self.renderer.get_or_insert_with(|| Renderer::new(&gl_config.display()));

        // Try setting vsync.
        if let Err(res) = gl_surface
            .set_swap_interval(gl_context, SwapInterval::Wait(NonZeroU32::new(1).unwrap()))
        {
            eprintln!("Error setting vsync: {res:?}");
        }

        self.gl_surface = Some(gl_surface);
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        // This event is only raised on Android, where the backing NativeWindow for a GL
        // Surface can appear and disappear at any moment.
        println!("Android window removed");

        // Destroy the GL Surface and un-current the GL Context before ndk-glue releases
        // the window back to the system.
        let gl_context = self.gl_context.take().unwrap();
        gl_context.make_not_current().unwrap();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::Resized(size) if size.width != 0 && size.height != 0 => {
                // Some platforms like EGL require resizing GL surface to update the size
                // Notable platforms here are Wayland and macOS, other don't require it
                // and the function is no-op, but it's wise to resize it for portability
                // reasons.
                if let Some((gl_context, gl_surface)) =
                    &self.gl_context.as_ref().zip(self.gl_surface.as_ref())
                {
                    gl_surface.resize(
                        gl_context,
                        NonZeroU32::new(size.width).unwrap(),
                        NonZeroU32::new(size.height).unwrap(),
                    );
                    let renderer = self.renderer.as_ref().unwrap();
                    renderer.resize(size.width as i32, size.height as i32);
                }
            },
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key: Key::Named(NamedKey::Escape), .. },
                ..
            } => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                if let Some(((gl_context, gl_surface), window)) = &self
                    .gl_context
                    .as_ref()
                    .zip(self.gl_surface.as_ref())
                    .zip(self.window.as_ref())
                {
                    let renderer = self.renderer.as_ref().unwrap();
                    renderer.draw();
                    window.request_redraw();

                    gl_surface.swap_buffers(gl_context).unwrap();
                }
            },
            _ => (),
        }
    }
}

// Find the config with the maximum number of samples, so our triangle will be
// smooth.
pub fn gl_config_picker(configs: Box<dyn Iterator<Item = Config> + '_>) -> Config {
    configs
        .reduce(|accum, config| {
            let transparency_check = config.supports_transparency().unwrap_or(false)
                & !accum.supports_transparency().unwrap_or(false);

            if transparency_check || config.num_samples() > accum.num_samples() {
                config
            } else {
                accum
            }
        })
        .unwrap()
}

pub struct Renderer {
    program: gl::types::GLuint,
    vao: gl::types::GLuint,
    vbo: gl::types::GLuint,
    gl: gl::Gl,
}

impl Renderer {
    pub fn new<D: GlDisplay>(gl_display: &D) -> Self {
        unsafe {
            let gl = gl::Gl::load_with(|symbol| {
                let symbol = CString::new(symbol).unwrap();
                gl_display.get_proc_address(symbol.as_c_str()).cast()
            });

            if let Some(renderer) = get_gl_string(&gl, gl::RENDERER) {
                println!("Running on {}", renderer.to_string_lossy());
            }
            if let Some(version) = get_gl_string(&gl, gl::VERSION) {
                println!("OpenGL Version {}", version.to_string_lossy());
            }

            if let Some(shaders_version) = get_gl_string(&gl, gl::SHADING_LANGUAGE_VERSION) {
                println!("Shaders version on {}", shaders_version.to_string_lossy());
            }

            let vertex_shader = create_shader(&gl, gl::VERTEX_SHADER, VERTEX_SHADER_SOURCE);
            let fragment_shader = create_shader(&gl, gl::FRAGMENT_SHADER, FRAGMENT_SHADER_SOURCE);

            let program = gl.CreateProgram();

            gl.AttachShader(program, vertex_shader);
            gl.AttachShader(program, fragment_shader);

            gl.LinkProgram(program);

            gl.UseProgram(program);

            gl.DeleteShader(vertex_shader);
            gl.DeleteShader(fragment_shader);

            let mut vao = std::mem::zeroed();
            gl.GenVertexArrays(1, &mut vao);
            gl.BindVertexArray(vao);

            let mut vbo = std::mem::zeroed();
            gl.GenBuffers(1, &mut vbo);
            gl.BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl.BufferData(
                gl::ARRAY_BUFFER,
                (VERTEX_DATA.len() * std::mem::size_of::<f32>()) as gl::types::GLsizeiptr,
                VERTEX_DATA.as_ptr() as *const _,
                gl::STATIC_DRAW,
            );

            let pos_attrib = gl.GetAttribLocation(program, b"position\0".as_ptr() as *const _);
            let color_attrib = gl.GetAttribLocation(program, b"color\0".as_ptr() as *const _);
            gl.VertexAttribPointer(
                pos_attrib as gl::types::GLuint,
                2,
                gl::FLOAT,
                0,
                5 * std::mem::size_of::<f32>() as gl::types::GLsizei,
                std::ptr::null(),
            );
            gl.VertexAttribPointer(
                color_attrib as gl::types::GLuint,
                3,
                gl::FLOAT,
                0,
                5 * std::mem::size_of::<f32>() as gl::types::GLsizei,
                (2 * std::mem::size_of::<f32>()) as *const () as *const _,
            );
            gl.EnableVertexAttribArray(pos_attrib as gl::types::GLuint);
            gl.EnableVertexAttribArray(color_attrib as gl::types::GLuint);

            Self { program, vao, vbo, gl }
        }
    }

    pub fn draw(&self) {
        self.draw_with_clear_color(0.1, 0.1, 0.1, 0.9)
    }

    pub fn draw_with_clear_color(
        &self,
        red: GLfloat,
        green: GLfloat,
        blue: GLfloat,
        alpha: GLfloat,
    ) {
        unsafe {
            self.gl.UseProgram(self.program);

            self.gl.BindVertexArray(self.vao);
            self.gl.BindBuffer(gl::ARRAY_BUFFER, self.vbo);

            self.gl.ClearColor(red, green, blue, alpha);
            self.gl.Clear(gl::COLOR_BUFFER_BIT);
            self.gl.DrawArrays(gl::TRIANGLES, 0, 3);
        }
    }

    pub fn resize(&self, width: i32, height: i32) {
        unsafe {
            self.gl.Viewport(0, 0, width, height);
        }
    }
}

impl Deref for Renderer {
    type Target = gl::Gl;

    fn deref(&self) -> &Self::Target {
        &self.gl
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            self.gl.DeleteProgram(self.program);
            self.gl.DeleteBuffers(1, &self.vbo);
            self.gl.DeleteVertexArrays(1, &self.vao);
        }
    }
}

unsafe fn create_shader(
    gl: &gl::Gl,
    shader: gl::types::GLenum,
    source: &[u8],
) -> gl::types::GLuint {
    let shader = gl.CreateShader(shader);
    gl.ShaderSource(shader, 1, [source.as_ptr().cast()].as_ptr(), std::ptr::null());
    gl.CompileShader(shader);
    shader
}

fn get_gl_string(gl: &gl::Gl, variant: gl::types::GLenum) -> Option<&'static CStr> {
    unsafe {
        let s = gl.GetString(variant);
        (!s.is_null()).then(|| CStr::from_ptr(s.cast()))
    }
}

#[rustfmt::skip]
static VERTEX_DATA: [f32; 15] = [
    -0.5, -0.5,  1.0,  0.0,  0.0,
     0.0,  0.5,  0.0,  1.0,  0.0,
     0.5, -0.5,  0.0,  0.0,  1.0,
];

const VERTEX_SHADER_SOURCE: &[u8] = b"
#version 100
precision mediump float;

attribute vec2 position;
attribute vec3 color;

varying vec3 v_color;

void main() {
    gl_Position = vec4(position, 0.0, 1.0);
    v_color = color;
}
\0";

const FRAGMENT_SHADER_SOURCE: &[u8] = b"
#version 100
precision mediump float;

varying vec3 v_color;

void main() {
    gl_FragColor = vec4(v_color, 1.0);
}
\0";
