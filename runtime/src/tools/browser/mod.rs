//! Browser automation tools backed by Chrome DevTools Protocol.
//!
//! The tools use an isolated, managed Chromium profile by default. They expose a
//! small browser-use-style action model: start/navigate/state/click/type/select/
//! scroll/back/screenshot/close.

mod cdp;
mod manager;
mod scripts;
mod session;
mod tool_impls;
mod util;

pub use manager::BrowserManager;
pub use tool_impls::{
    BrowserBackTool, BrowserClickTool, BrowserCloseTool, BrowserFindUrlTool, BrowserNavigateTool,
    BrowserScreenshotTool, BrowserScrollTool, BrowserSelectTool, BrowserStartTool,
    BrowserStateTool, BrowserTypeTool,
};
