use std::collections::HashMap;
use std::sync::{Mutex as StdMutex, OnceLock};

use lash::LashRuntime;
use tokio_util::sync::CancellationToken;

use super::types::ShepherdScope;

pub(super) struct ShepherdSession {
    pub scope: ShepherdScope,
    pub active_turn: Option<CancellationToken>,
    pub runtime: Option<LashRuntime>,
}

static ACTIVE_SESSIONS: OnceLock<StdMutex<HashMap<String, ShepherdSession>>> = OnceLock::new();

pub(super) fn sessions() -> &'static StdMutex<HashMap<String, ShepherdSession>> {
    ACTIVE_SESSIONS.get_or_init(|| StdMutex::new(HashMap::new()))
}
