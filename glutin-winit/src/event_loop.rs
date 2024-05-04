use raw_window_handle::{HasRawDisplayHandle, RawDisplayHandle};
use winit::error::OsError;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes};

use crate::private::Sealed;

/// Even though [ActiveEventLoop] is the recommended way for using the event
/// loop we still want to have support for [EventLoop], for now.
pub trait GlutinEventLoop: Sealed {
    fn create_window(&self, window_attributes: WindowAttributes) -> Result<Window, OsError>;

    fn display_handle(&self) -> RawDisplayHandle;
}

impl Sealed for ActiveEventLoop {}

impl GlutinEventLoop for ActiveEventLoop {
    fn create_window(&self, window_attributes: WindowAttributes) -> Result<Window, OsError> {
        self.create_window(window_attributes)
    }

    fn display_handle(&self) -> RawDisplayHandle {
        self.raw_display_handle()
    }
}

impl<T> Sealed for EventLoop<T> {}

impl<T> GlutinEventLoop for EventLoop<T> {
    #[allow(deprecated)]
    fn create_window(&self, window_attributes: WindowAttributes) -> Result<Window, OsError> {
        self.create_window(window_attributes)
    }

    fn display_handle(&self) -> RawDisplayHandle {
        self.raw_display_handle()
    }
}
