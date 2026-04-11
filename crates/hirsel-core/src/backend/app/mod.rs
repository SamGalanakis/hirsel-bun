pub mod projects;

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
pub use projects::{create_project, get_project_surface, list_projects, update_project_settings};
