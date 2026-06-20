use crate::error::AgentError;
use crate::tools::browser::manager::{BrowserManager, SharedSession};

mod find_url;
mod input;
mod lifecycle;
mod navigation;
mod page;
mod state;

pub use find_url::BrowserFindUrlTool;
pub use input::{BrowserClickTool, BrowserSelectTool, BrowserTypeTool};
pub use lifecycle::{BrowserCloseTool, BrowserStartTool};
pub use navigation::BrowserNavigateTool;
pub use page::{BrowserBackTool, BrowserScreenshotTool, BrowserScrollTool};
pub use state::BrowserStateTool;

pub(super) async fn require_session(
    manager: &BrowserManager,
    key: &str,
    tool: &str,
) -> Result<SharedSession, AgentError> {
    manager.get(key).await.ok_or_else(|| AgentError::ToolExecution {
        tool: tool.into(),
        message: "no browser session found; call BrowserNavigate or BrowserStart first".into(),
    })
}
