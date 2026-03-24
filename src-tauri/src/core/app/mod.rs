pub mod concerns;
pub mod delivery;
pub mod events;
pub mod projects;
pub mod routes;
pub mod types;
pub mod workers;
pub mod worktree;

pub trait ResultExt<T, E: ToString> {
    fn str_err(self) -> Result<T, String>;
    fn context(self, msg: &str) -> Result<T, String>;
}

impl<T, E: ToString> ResultExt<T, E> for Result<T, E> {
    fn str_err(self) -> Result<T, String> {
        self.map_err(|e| e.to_string())
    }

    fn context(self, msg: &str) -> Result<T, String> {
        self.map_err(|e| format!("{}: {}", msg, e.to_string()))
    }
}

pub use concerns::get_all_unread_notifications;
pub use delivery::{
    abandon_board_delivery, complete_board_delivery, get_board_versions,
    get_current_board_delivery, get_delivery_attempts, get_latest_board_version,
    retry_board_delivery, start_board_delivery, validate_delivery_target,
};
pub use events::get_route_worker_events;
pub use projects::{
    create_project, get_project_surface, list_projects, update_project_description,
    update_project_name, ProjectLifecycleService,
};
pub use routes::{
    archive_route, create_route, get_active_route, get_route, set_active_route,
    update_route_settings,
};
pub use workers::get_route_workers;
pub use worktree::{
    archive_work_item, assign_work_item, create_work_item, get_route_work_tree, reopen_work_item,
    split_work_item, SplitWorkItemRequest,
};
