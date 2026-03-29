mod connect;
mod conversation;
mod focus;
mod projects;
mod settings;
mod shared;
mod thread_detail;
mod threads;

use crate::backend::shepherd_runtime::ShepherdScopeActivity;
use crate::backend::{ShepherdChatMessage, ShepherdThread};

#[derive(Debug, Clone)]
pub struct ThreadPanelState {
    pub thread: ShepherdThread,
    pub history: Vec<ShepherdChatMessage>,
    pub activity: ShepherdScopeActivity,
}

pub use connect::render_connect_page;
pub use conversation::render_chat_panel;
pub use focus::{render_focus_document, render_project_focus_stage};
pub use projects::{
    render_new_project_page, render_project_page, ProjectCreateDraft, ProjectCreateReview,
};
pub use settings::{render_project_settings_page, render_settings_page};
pub use thread_detail::{render_thread_detail_main, render_thread_detail_page};
pub use threads::render_threads_panel;
