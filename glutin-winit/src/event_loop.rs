use raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle};
use winit::error::OsError;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes};

use crate::private::Sealed;

/// Even though [ActiveEventLoop] is the recommended way for using the event
/// loop we still want to have support for [EventLoop], for now.
pub trait GlutinEventLoop: Sealed {
    fn create_window(&self, window_attributes: WindowAttributes) -> Result<Window, OsError>;

    fn glutin_display_handle(&self) -> Result<DisplayHandle<'_>, HandleError>;
}

impl Sealed for ActiveEventLoop {}

impl GlutinEventLoop for ActiveEventLoop {
    fn create_window(&self, window_attributes: WindowAttributes) -> Result<Window, OsError> {
        self.create_window(window_attributes)
    }

    fn glutin_display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.display_handle()
    }
}

impl<T> Sealed for EventLoop<T> {}

impl<T> GlutinEventLoop for EventLoop<T> {
    #[allow(deprecated)]
    fn create_window(&self, window_attributes: WindowAttributes) -> Result<Window, OsError> {
        self.create_window(window_attributes)
    }

    fn glutin_display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.display_handle()
    }
}
