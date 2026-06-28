pub const SQLITE_SCHEMA: &str = r#"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    display_name TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'user')),
    created_at_epoch_seconds INTEGER NOT NULL,
    updated_at_epoch_seconds INTEGER NOT NULL,
    disabled_at_epoch_seconds INTEGER
);

CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    token_hash TEXT NOT NULL UNIQUE,
    user_id INTEGER NOT NULL REFERENCES users(id),
    expires_at_epoch_seconds INTEGER NOT NULL,
    created_at_epoch_seconds INTEGER NOT NULL,
    last_seen_at_epoch_seconds INTEGER NOT NULL,
    revoked_at_epoch_seconds INTEGER
);

CREATE TABLE IF NOT EXISTS invite_codes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code_hash TEXT NOT NULL UNIQUE,
    created_by_user_id INTEGER NOT NULL REFERENCES users(id),
    expires_at_epoch_seconds INTEGER NOT NULL,
    used_by_user_id INTEGER REFERENCES users(id),
    used_at_epoch_seconds INTEGER,
    created_at_epoch_seconds INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS persistent_rooms (
    room_id TEXT PRIMARY KEY,
    owner_user_id INTEGER NOT NULL REFERENCES users(id),
    created_at_epoch_seconds INTEGER NOT NULL,
    last_active_at_epoch_seconds INTEGER NOT NULL,
    closed_at_epoch_seconds INTEGER
);

CREATE INDEX IF NOT EXISTS idx_sessions_token_hash ON sessions(token_hash);
CREATE INDEX IF NOT EXISTS idx_invite_codes_code_hash ON invite_codes(code_hash);
CREATE INDEX IF NOT EXISTS idx_persistent_rooms_open ON persistent_rooms(closed_at_epoch_seconds);
"#;
