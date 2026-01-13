-- hirsel run state schema

CREATE TABLE IF NOT EXISTS state (
    id INTEGER PRIMARY KEY CHECK (id = 1),  -- singleton row
    status TEXT NOT NULL DEFAULT 'idle',     -- idle|working|paused|runaway|timed_out|eval|eval_failed|waiting|done|delivered|merged
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    request TEXT,                            -- current waiting message
    project_path TEXT,                       -- path to git project root
    unread_count INTEGER DEFAULT 0,          -- unread messages from workers
    human_in_the_loop INTEGER DEFAULT 1,     -- 1=HITL (wait for user), 0=YOLO (auto-respond)
    summary TEXT,                            -- run summary generated at completion
    waiting_reason TEXT,                     -- separate from request/spec
    worker_scale TEXT,                       -- for autoscaling: "3", "1-5", "2+"
    time_limit_minutes INTEGER,
    started_at TEXT,
    last_time_notification_pct INTEGER,
    iteration_count INTEGER DEFAULT 0,
    max_iterations INTEGER,
    learnings_processed_at TEXT              -- for improve feature
);

CREATE TABLE IF NOT EXISTS workers (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,               -- greek name (achilles, hector, etc.)
    pid INTEGER,                             -- worker process id
    session_id TEXT,                         -- claude session id for resume/metrics
    session_started_at TEXT,                 -- when current session started (for inbox)
    status TEXT NOT NULL DEFAULT 'idle',     -- idle|working|waiting|awaiting|paused|done|error
    work_dir TEXT,                           -- worktree path
    waiting_thread TEXT,                     -- thread name when status=waiting
    needs_restart INTEGER DEFAULT 0,         -- for fresh context per task
    location TEXT DEFAULT 'local',           -- local|remote
    last_heartbeat TEXT,                     -- for remote worker support
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS history (
    id INTEGER PRIMARY KEY,
    timestamp TEXT NOT NULL,
    action TEXT NOT NULL,
    detail TEXT
);

CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,                     -- readable slug: setup_user_model
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'todo',     -- todo|doing|done
    created_at TEXT NOT NULL,
    completed_at TEXT,
    claimed_by TEXT,                         -- worker name who claimed
    claimed_at TEXT,                         -- when claimed
    tokens_used INTEGER,                     -- tokens consumed during task (for sizing analysis)
    parent_id TEXT,                          -- for hierarchical tasks
    blocked_by TEXT,                         -- comma-separated task IDs
    pending_done_at TEXT                     -- for two-phase task completion
);

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY,
    thread TEXT NOT NULL,                    -- chat thread name (user, group, learnings, etc.)
    sender TEXT NOT NULL,                    -- worker name, "user", or "hirsel"
    content TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    waiting INTEGER DEFAULT 0               -- 1 if this was a blocking message
);

CREATE TABLE IF NOT EXISTS message_reads (
    worker_name TEXT NOT NULL,
    thread TEXT NOT NULL,
    last_read_id INTEGER NOT NULL,           -- last message id read by this worker in this thread
    PRIMARY KEY (worker_name, thread)
);

CREATE INDEX IF NOT EXISTS idx_messages_thread ON messages(thread);
CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON messages(timestamp);

CREATE TABLE IF NOT EXISTS evals (
    id INTEGER PRIMARY KEY,
    branch TEXT NOT NULL,
    eval_name TEXT,
    status TEXT NOT NULL DEFAULT 'running',  -- running|passed|failed
    feedback TEXT,
    log_file TEXT,
    started_at TEXT NOT NULL,
    finished_at TEXT
);

CREATE TABLE IF NOT EXISTS amendments (
    id INTEGER PRIMARY KEY,
    message TEXT NOT NULL,                   -- commit-like message describing the change
    timestamp TEXT NOT NULL,
    author TEXT NOT NULL DEFAULT 'user',     -- who made the amendment
    spec_hash TEXT NOT NULL                  -- hash of spec.md after this amendment
);
