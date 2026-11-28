pub const SCHEMA_VERSION: u32 = 4;

pub const SCHEMA: &str = r#"
-- Content Store: shared, source-grounded entities.
CREATE TABLE IF NOT EXISTS resources (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('pdf','image','audio','video','transcript','web')),
    title TEXT NOT NULL,
    sha256 TEXT NOT NULL UNIQUE,
    byte_size INTEGER NOT NULL CHECK (byte_size >= 0),
    page_count INTEGER CHECK (page_count IS NULL OR page_count >= 0),
    width INTEGER CHECK (width IS NULL OR width > 0),
    height INTEGER CHECK (height IS NULL OR height > 0),
    license TEXT,
    attribution_url TEXT,
    attribution_notice TEXT,
    sensitivity TEXT NOT NULL CHECK (sensitivity IN ('normal','restricted')),
    export_policy TEXT NOT NULL CHECK (export_policy IN ('allow','redact','owner_only')),
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS anchors (
    id TEXT PRIMARY KEY,
    resource_id TEXT NOT NULL REFERENCES resources(id),
    anchor_kind TEXT NOT NULL CHECK (anchor_kind IN ('pdf_page','rect','media_span')),
    pdf_page INTEGER CHECK (pdf_page IS NULL OR pdf_page >= 0),
    printed_page INTEGER CHECK (printed_page IS NULL OR printed_page >= 0),
    legend_pdf_page INTEGER CHECK (legend_pdf_page IS NULL OR legend_pdf_page >= 0),
    x REAL, y REAL, w REAL, h REAL,
    start_ms INTEGER, end_ms INTEGER,
    CHECK (anchor_kind != 'pdf_page' OR pdf_page IS NOT NULL),
    CHECK (anchor_kind != 'rect' OR (
        x IS NOT NULL AND y IS NOT NULL AND w IS NOT NULL AND h IS NOT NULL
        AND x >= 0.0 AND x <= 1.0 AND y >= 0.0 AND y <= 1.0
        AND w > 0.0 AND h > 0.0 AND x + w <= 1.0 AND y + h <= 1.0
    )),
    CHECK (anchor_kind != 'media_span' OR (
        start_ms IS NOT NULL AND end_ms IS NOT NULL AND end_ms > start_ms
    ))
);

CREATE TABLE IF NOT EXISTS knowledge_cards (
    id TEXT PRIMARY KEY,
    subject TEXT NOT NULL,
    topics_json TEXT NOT NULL DEFAULT '[]',
    title TEXT NOT NULL,
    blocks_json TEXT NOT NULL DEFAULT '[]',
    search_text TEXT NOT NULL DEFAULT '',
    anchor_ids_json TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL CHECK (status IN ('draft','staging','published','archived')),
    verification TEXT NOT NULL CHECK (verification IN ('unchecked','verified')),
    unsure_count INTEGER NOT NULL DEFAULT 0 CHECK (unsure_count >= 0),
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS notes (
    id TEXT PRIMARY KEY,
    subject TEXT NOT NULL,
    title TEXT NOT NULL,
    outline_json TEXT NOT NULL DEFAULT '[]',
    view_settings_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL CHECK (status IN ('draft','staging','published','archived')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS exam_sets (
    id TEXT PRIMARY KEY,
    subject TEXT NOT NULL,
    title TEXT NOT NULL,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('past_exam','question_bank','original','custom')),
    exam_year INTEGER,
    source_resource_id TEXT REFERENCES resources(id),
    expected_questions INTEGER CHECK (expected_questions IS NULL OR expected_questions >= 0),
    metadata_json TEXT NOT NULL DEFAULT '{}',
    is_complete INTEGER NOT NULL CHECK (is_complete IN (0,1))
);

CREATE TABLE IF NOT EXISTS question_groups (
    id TEXT PRIMARY KEY,
    exam_set_id TEXT REFERENCES exam_sets(id),
    title TEXT,
    shared_stem_blocks_json TEXT NOT NULL DEFAULT '[]'
);

-- Nested subject folders for the library view. A group owns a sorted set of
-- subject names; bundles inherit every subject of their descendants so a drill
-- can be scoped to one folder at any depth.
CREATE TABLE IF NOT EXISTS subject_groups (
    id TEXT PRIMARY KEY,
    parent_id TEXT REFERENCES subject_groups(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS subject_groups_parent ON subject_groups (parent_id, sort_order, name);

CREATE TABLE IF NOT EXISTS subject_group_subjects (
    group_id TEXT NOT NULL REFERENCES subject_groups(id) ON DELETE CASCADE,
    subject TEXT NOT NULL,
    PRIMARY KEY (group_id, subject)
);

CREATE INDEX IF NOT EXISTS subject_group_subjects_subject ON subject_group_subjects (subject);

CREATE TABLE IF NOT EXISTS question_cards (
    id TEXT PRIMARY KEY,
    exam_set_id TEXT REFERENCES exam_sets(id),
    question_group_id TEXT REFERENCES question_groups(id),
    question_no INTEGER CHECK (question_no IS NULL OR question_no >= 0),
    subject TEXT NOT NULL,
    topics_json TEXT NOT NULL DEFAULT '[]',
    stem_blocks_json TEXT NOT NULL DEFAULT '[]',
    explanation_blocks_json TEXT NOT NULL DEFAULT '[]',
    search_text TEXT NOT NULL DEFAULT '',
    anchor_ids_json TEXT NOT NULL DEFAULT '[]',
    question_type TEXT NOT NULL CHECK (question_type IN (
        'single_choice','multiple_choice','short_answer','vocab','image_qa',
        'image_occlusion','pin_identify','pin_locate'
    )),
    grading_mode TEXT NOT NULL CHECK (grading_mode IN ('exact','any_accepted','voided')),
    options_json TEXT NOT NULL DEFAULT '[]',
    answer_json TEXT NOT NULL DEFAULT '[]',
    accepted_answers_json TEXT NOT NULL DEFAULT '[]',
    answer_source_kind TEXT NOT NULL CHECK (answer_source_kind IN (
        'official','peer_note','duplicate_question','inferred','original_authored','unanswered'
    )),
    answer_basis TEXT,
    duplicate_question_id TEXT REFERENCES question_cards(id),
    status TEXT NOT NULL CHECK (status IN ('draft','staging','published','archived')),
    verification TEXT NOT NULL CHECK (verification IN ('unchecked','verified')),
    unsure_count INTEGER NOT NULL DEFAULT 0 CHECK (unsure_count >= 0),
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS edges (
    id TEXT PRIMARY KEY,
    edge_type TEXT NOT NULL CHECK (edge_type IN ('tests','confusable_with','prerequisite','variant_of')),
    from_kind TEXT NOT NULL CHECK (from_kind IN ('card','question','option')),
    from_id TEXT NOT NULL,
    from_option_key TEXT,
    from_key TEXT NOT NULL,
    to_kind TEXT NOT NULL CHECK (to_kind IN ('card','question','option')),
    to_id TEXT NOT NULL,
    to_option_key TEXT,
    to_key TEXT NOT NULL,
    created_at TEXT NOT NULL,
    CHECK (from_kind = 'option' OR from_option_key IS NULL),
    CHECK (to_kind = 'option' OR to_option_key IS NULL)
);

CREATE UNIQUE INDEX IF NOT EXISTS edges_identity ON edges (
    edge_type, from_key, to_key
);

CREATE INDEX IF NOT EXISTS edges_from_key ON edges (from_key);
CREATE INDEX IF NOT EXISTS edges_to_key ON edges (to_key);

CREATE TABLE IF NOT EXISTS proposals (
    id TEXT PRIMARY KEY,
    target_kind TEXT NOT NULL CHECK (target_kind IN (
        'resource','knowledge_card','note','exam_set','question_group','question_card','edge'
    )),
    target_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    submitted_by TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending','approved','rejected')),
    review_note TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    sync_cursor INTEGER NOT NULL DEFAULT 0
);

-- Study Store: per-user, append-only and deterministically derivable.
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS credentials (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('owner','member')),
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS sessions_user_id ON sessions (user_id);

CREATE TABLE IF NOT EXISTS invites (
    code_hash TEXT PRIMARY KEY,
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS attempt_logs (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    card_id TEXT NOT NULL,
    direction TEXT NOT NULL CHECK (direction IN ('default','forward','reverse')),
    session_mode TEXT NOT NULL CHECK (session_mode IN (
        'daily_due','full_exam','topic_drill','practical_exam','weakness_drill'
    )),
    submitted_payload TEXT NOT NULL DEFAULT '{}',
    is_correct INTEGER NOT NULL CHECK (is_correct IN (0,1)),
    duration_ms INTEGER NOT NULL CHECK (duration_ms >= 0),
    confidence TEXT NOT NULL CHECK (confidence IN ('certain','split','guessed')),
    error_tags_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    sync_cursor INTEGER NOT NULL DEFAULT 0,
    affects_scheduling INTEGER NOT NULL DEFAULT 1 CHECK (affects_scheduling IN (0,1))
);

CREATE INDEX IF NOT EXISTS attempt_logs_replay ON attempt_logs (user_id, card_id, direction, created_at);

CREATE INDEX IF NOT EXISTS attempt_logs_sync ON attempt_logs (user_id, sync_cursor, id);

CREATE TRIGGER IF NOT EXISTS attempt_logs_no_update
BEFORE UPDATE ON attempt_logs
BEGIN
    SELECT RAISE(ABORT, 'attempt logs are append-only');
END;

CREATE TRIGGER IF NOT EXISTS attempt_logs_no_delete
BEFORE DELETE ON attempt_logs
BEGIN
    SELECT RAISE(ABORT, 'attempt logs are append-only');
END;

CREATE TABLE IF NOT EXISTS card_scheduling (
    user_id TEXT NOT NULL REFERENCES users(id),
    card_id TEXT NOT NULL,
    direction TEXT NOT NULL CHECK (direction IN ('default','forward','reverse')),
    stability REAL NOT NULL CHECK (stability >= 0.0),
    difficulty REAL NOT NULL CHECK (difficulty >= 0.0),
    reps INTEGER NOT NULL CHECK (reps >= 0),
    lapses INTEGER NOT NULL CHECK (lapses >= 0),
    state TEXT NOT NULL CHECK (state IN ('new','learning','review','relearning')),
    last_review_at TEXT,
    due_at TEXT,
    desired_retention REAL NOT NULL CHECK (desired_retention > 0.0 AND desired_retention < 1.0),
    updated_at TEXT NOT NULL,
    PRIMARY KEY (user_id, card_id, direction)
);

CREATE TABLE IF NOT EXISTS user_card_meta (
    user_id TEXT NOT NULL REFERENCES users(id),
    card_id TEXT NOT NULL,
    starred INTEGER NOT NULL CHECK (starred IN (0,1)),
    personal_tags_json TEXT NOT NULL DEFAULT '[]',
    personal_note TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (user_id, card_id)
);

-- Device sync bookkeeping. These tables never touch the append-only attempt
-- log or the mutable personal rows: a failed request only leaves the work
-- marked as pending, so retrying a batch is always safe.
CREATE TABLE IF NOT EXISTS sync_state (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    remote_user_id TEXT NOT NULL,
    pull_cursor INTEGER NOT NULL DEFAULT 0 CHECK (pull_cursor >= 0),
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sync_attempt_uploads (
    attempt_id TEXT PRIMARY KEY REFERENCES attempt_logs(id),
    uploaded_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sync_meta_uploads (
    user_id TEXT NOT NULL REFERENCES users(id),
    card_id TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (user_id, card_id)
);

-- External plugin work queue. A row is the durable record of one heavy task
-- handed to an out-of-process worker: the plugin name and capability must
-- match a manifest the owner wrote in the server configuration file, and the
-- result is always a proposal, never a direct write. `attempts` counts started
-- runs, `cancel_requested` records a cancel that a running worker still has to
-- honour, and `proposal_id` is set only when the submission was accepted.
CREATE TABLE IF NOT EXISTS plugin_jobs (
    id TEXT PRIMARY KEY,
    plugin TEXT NOT NULL,
    capability TEXT NOT NULL,
    input_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN (
        'queued','running','succeeded','failed','cancelled'
    )),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    max_attempts INTEGER NOT NULL CHECK (max_attempts >= 1),
    cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK (cancel_requested IN (0,1)),
    progress REAL NOT NULL DEFAULT 0.0 CHECK (progress >= 0.0 AND progress <= 1.0),
    progress_message TEXT,
    proposal_id TEXT REFERENCES proposals(id),
    error_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT
);

CREATE INDEX IF NOT EXISTS plugin_jobs_queue ON plugin_jobs (status, created_at, id);
CREATE INDEX IF NOT EXISTS plugin_jobs_plugin ON plugin_jobs (plugin, status, created_at);

-- External-content FTS5 with the trigram tokenizer so CJK substring queries work.
-- Only readable text is indexed; topics are flattened by the writer.
CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_cards_fts USING fts5(
    title, subject, search_text,
    content='knowledge_cards', content_rowid='rowid', tokenize='trigram'
);

CREATE TRIGGER IF NOT EXISTS knowledge_cards_fts_insert AFTER INSERT ON knowledge_cards BEGIN
    INSERT INTO knowledge_cards_fts(rowid, title, subject, search_text)
    VALUES (new.rowid, new.title, new.subject, new.search_text);
END;

CREATE TRIGGER IF NOT EXISTS knowledge_cards_fts_delete AFTER DELETE ON knowledge_cards BEGIN
    INSERT INTO knowledge_cards_fts(knowledge_cards_fts, rowid, title, subject, search_text)
    VALUES ('delete', old.rowid, old.title, old.subject, old.search_text);
END;

CREATE TRIGGER IF NOT EXISTS knowledge_cards_fts_update AFTER UPDATE ON knowledge_cards BEGIN
    INSERT INTO knowledge_cards_fts(knowledge_cards_fts, rowid, title, subject, search_text)
    VALUES ('delete', old.rowid, old.title, old.subject, old.search_text);
    INSERT INTO knowledge_cards_fts(rowid, title, subject, search_text)
    VALUES (new.rowid, new.title, new.subject, new.search_text);
END;

CREATE VIRTUAL TABLE IF NOT EXISTS question_cards_fts USING fts5(
    subject, search_text,
    content='question_cards', content_rowid='rowid', tokenize='trigram'
);

CREATE TRIGGER IF NOT EXISTS question_cards_fts_insert AFTER INSERT ON question_cards BEGIN
    INSERT INTO question_cards_fts(rowid, subject, search_text)
    VALUES (new.rowid, new.subject, new.search_text);
END;

CREATE TRIGGER IF NOT EXISTS question_cards_fts_delete AFTER DELETE ON question_cards BEGIN
    INSERT INTO question_cards_fts(question_cards_fts, rowid, subject, search_text)
    VALUES ('delete', old.rowid, old.subject, old.search_text);
END;

CREATE TRIGGER IF NOT EXISTS question_cards_fts_update AFTER UPDATE ON question_cards BEGIN
    INSERT INTO question_cards_fts(question_cards_fts, rowid, subject, search_text)
    VALUES ('delete', old.rowid, old.subject, old.search_text);
    INSERT INTO question_cards_fts(rowid, subject, search_text)
    VALUES (new.rowid, new.subject, new.search_text);
END;
"#;
