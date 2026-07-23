//! Durable kernel state: the SQLite registry (plan v3 Phase B).
//!
//! The directory layout under `.ivk/workspaces/` remains the physical source
//! of *files*; SQLite is the source of *state*. Every multi-step operation
//! journals its intent here first, so a SIGKILL at any point leaves a row in
//! an in-flight state (`creating` / `removing`) that `ivk doctor` can report
//! and `ivk doctor --repair` can roll back or complete.
//!
//! Compatibility: `sync_from_disk` backfills rows from the pre-DB directory
//! layout and from `.ivk/changesets/*.json`, so repos created by v0.0.x work
//! unchanged. The JSON changeset files continue to be written as artifacts;
//! the DB is authoritative for queries.
//!
//! Concurrency: WAL mode + a busy timeout make N parallel `ivk new`
//! processes safe — writes are row-scoped and take milliseconds.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug)]
pub struct RegistryError(pub String);

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "registry error: {}", self.0)
    }
}

impl std::error::Error for RegistryError {}

impl From<rusqlite::Error> for RegistryError {
    fn from(e: rusqlite::Error) -> Self {
        RegistryError(e.to_string())
    }
}

type Result<T> = std::result::Result<T, RegistryError>;

/// Lifecycle state of a workspace row. `Creating` and `Removing` are
/// in-flight journal states; anything found in them at rest is evidence of
/// an interrupted operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceState {
    Creating,
    Ready,
    Removing,
}

impl WorkspaceState {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkspaceState::Creating => "creating",
            WorkspaceState::Ready => "ready",
            WorkspaceState::Removing => "removing",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "creating" => Some(WorkspaceState::Creating),
            "ready" => Some(WorkspaceState::Ready),
            "removing" => Some(WorkspaceState::Removing),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceRecord {
    pub name: String,
    pub state: WorkspaceState,
    pub base_snapshot: Option<String>,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
}

/// An operation that journaled its intent and has not confirmed completion.
/// Found at rest, it means the process died mid-operation.
#[derive(Debug, Clone)]
pub struct PendingOp {
    pub id: i64,
    /// Operation kind; currently `"ch-new"`.
    pub kind: String,
    pub workspace_name: String,
    /// State the operation started from (for `ch-new`: the worktree HEAD
    /// before committing — HEAD advanced past it means the commit landed).
    pub base_snapshot: Option<String>,
    pub started_at_unix: u64,
}

#[derive(Debug, Clone)]
pub struct ChangesetRecord {
    pub id: String,
    pub workspace_name: String,
    pub base_snapshot: String,
    pub result_snapshot: String,
    pub touched_paths: Vec<String>,
    pub created_at_unix: u64,
    pub exported_branch: Option<String>,
    pub exported_at_unix: Option<u64>,
}

/// One recorded conflict check: "does this changeset merge cleanly onto
/// this target?". A check is a fact about a *specific* target snapshot —
/// when the target ref moves, the fact goes stale and a re-check writes a
/// new row (one row per (changeset, target snapshot) pair).
#[derive(Debug, Clone)]
pub struct ChangesetCheckRecord {
    pub changeset_id: String,
    /// The revision the caller asked to check against (e.g. `"main"`).
    pub target_ref: String,
    /// What `target_ref` resolved to at check time.
    pub target_snapshot: String,
    pub clean: bool,
    pub conflict_paths: Vec<String>,
    pub checked_at_unix: u64,
}

/// A path that keeps showing up across changesets — the registry-level
/// early warning for contention and megafile growth: many independent
/// changes to one file means either the file is too big or task boundaries
/// keep crossing it.
#[derive(Debug, Clone)]
pub struct HotspotRecord {
    pub path: String,
    /// Distinct changesets that touched the path.
    pub changeset_count: u64,
    /// Distinct workspaces those changesets came from.
    pub workspace_count: u64,
}

/// What `sync_from_disk` imported.
#[derive(Debug, Default, Clone, Copy)]
pub struct SyncReport {
    pub imported_workspaces: usize,
    pub imported_changesets: usize,
}

/// Outcome of `begin_create`: whether this call inserted the journal row.
/// `AlreadyTracked` means a row existed (e.g. a stale row for a removed
/// directory, or a backfilled one) — a failure rollback must then leave the
/// row alone instead of deleting state it does not own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeginCreate {
    Started,
    AlreadyTracked,
}

pub struct Registry {
    conn: Connection,
}

const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS workspaces (
  name          TEXT PRIMARY KEY,
  state         TEXT NOT NULL CHECK (state IN ('creating','ready','removing')),
  base_snapshot TEXT,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS changesets (
  id              TEXT PRIMARY KEY,
  workspace_name  TEXT NOT NULL,
  base_snapshot   TEXT NOT NULL,
  result_snapshot TEXT NOT NULL,
  touched_paths   TEXT NOT NULL,
  created_at      INTEGER NOT NULL,
  exported_branch TEXT,
  exported_at     INTEGER
);
CREATE TABLE IF NOT EXISTS pending_ops (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  kind           TEXT NOT NULL,
  workspace_name TEXT NOT NULL,
  base_snapshot  TEXT,
  started_at     INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS changeset_checks (
  changeset_id    TEXT NOT NULL,
  target_ref      TEXT NOT NULL,
  target_snapshot TEXT NOT NULL,
  clean           INTEGER NOT NULL,
  conflict_paths  TEXT NOT NULL,
  checked_at      INTEGER NOT NULL,
  PRIMARY KEY (changeset_id, target_snapshot)
);
INSERT OR IGNORE INTO meta(key, value) VALUES ('schema_version', '1');
";

fn is_busy(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(f, _)
            if f.code == rusqlite::ErrorCode::DatabaseBusy
                || f.code == rusqlite::ErrorCode::DatabaseLocked
    )
}

// SQLite integers are i64; rusqlite deliberately does not implement ToSql
// for u64. Store as i64, expose as u64 on the record types.
fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Registry {
    /// Where the database lives for a repo root.
    pub fn db_path(repo_root: &Path) -> PathBuf {
        repo_root.join(".ivk").join("db.sqlite")
    }

    /// Open (creating if needed) the registry for `repo_root`. Creates
    /// `.ivk/` when absent — use [`Registry::open_if_present`] on read-only
    /// paths that must not initialize anything.
    pub fn open_at_root(repo_root: &Path) -> Result<Self> {
        let ivk_dir = repo_root.join(".ivk");
        fs::create_dir_all(&ivk_dir)
            .map_err(|e| RegistryError(format!("cannot create {}: {}", ivk_dir.display(), e)))?;
        Self::open_file(&Self::db_path(repo_root))
    }

    /// Open the registry only if `.ivk/` already exists; `Ok(None)` otherwise.
    pub fn open_if_present(repo_root: &Path) -> Result<Option<Self>> {
        if !repo_root.join(".ivk").is_dir() {
            return Ok(None);
        }
        Self::open_file(&Self::db_path(repo_root)).map(Some)
    }

    fn open_file(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| RegistryError(format!("cannot open {}: {}", path.display(), e)))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // WAL survives reopen; setting it every time is a cheap no-op after
        // the first. NORMAL synchronous is the documented WAL pairing.
        // The mode switch on a brand-new db needs an exclusive lock and can
        // return SQLITE_BUSY outside the busy handler when N processes race
        // the first open (observed at ~30 parallel `ivk new`) — retry.
        let mut attempts = 0u32;
        loop {
            match conn.pragma_update(None, "journal_mode", "WAL") {
                Ok(()) => break,
                Err(e) if attempts < 40 && is_busy(&e) => {
                    attempts += 1;
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                Err(e) => return Err(e.into()),
            }
        }
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Backfill rows from the on-disk layout so pre-DB repos (v0.0.x) and
    /// externally created state stay visible. Idempotent: existing rows are
    /// never modified (`INSERT OR IGNORE`), so in-flight journal states are
    /// preserved across syncs.
    pub fn sync_from_disk(&self, repo_root: &Path) -> Result<SyncReport> {
        let mut report = SyncReport::default();
        let now = now_unix();

        let ws_dir = repo_root.join(".ivk").join("workspaces");
        if let Ok(entries) = fs::read_dir(&ws_dir) {
            for entry in entries.flatten() {
                if !entry.path().is_dir() {
                    continue;
                }
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                let n = self.conn.execute(
                    "INSERT OR IGNORE INTO workspaces(name, state, base_snapshot, created_at, updated_at)
                     VALUES (?1, 'ready', NULL, ?2, ?2)",
                    params![name, now],
                )?;
                report.imported_workspaces += n;
            }
        }

        let ch_dir = repo_root.join(".ivk").join("changesets");
        if let Ok(entries) = fs::read_dir(&ch_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                let Ok(body) = fs::read_to_string(&path) else {
                    continue;
                };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) else {
                    continue;
                };
                let get = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
                let (Some(id), Some(ws), Some(base), Some(result)) = (
                    get("id"),
                    get("workspace_name"),
                    get("base_snapshot"),
                    get("result_snapshot"),
                ) else {
                    continue;
                };
                let touched = v
                    .get("touched_paths")
                    .cloned()
                    .unwrap_or_else(|| serde_json::Value::Array(vec![]));
                let created = v
                    .get("created_at_unix")
                    .and_then(|x| x.as_i64())
                    .unwrap_or(now);
                let n = self.conn.execute(
                    "INSERT OR IGNORE INTO changesets
                       (id, workspace_name, base_snapshot, result_snapshot, touched_paths, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![id, ws, base, result, touched.to_string(), created],
                )?;
                report.imported_changesets += n;
            }
        }

        Ok(report)
    }

    // ---------- workspace journal ----------

    /// Journal the intent to create workspace `name`. Call before touching
    /// the filesystem; call [`Registry::mark_ready`] after materialization
    /// succeeds, or [`Registry::delete_workspace_row`] to roll back — but
    /// only when this returned [`BeginCreate::Started`].
    pub fn begin_create(&self, name: &str, base_snapshot: Option<&str>) -> Result<BeginCreate> {
        let now = now_unix();
        let n = self.conn.execute(
            "INSERT INTO workspaces(name, state, base_snapshot, created_at, updated_at)
             VALUES (?1, 'creating', ?2, ?3, ?3)
             ON CONFLICT(name) DO NOTHING",
            params![name, base_snapshot, now],
        )?;
        Ok(if n == 1 {
            BeginCreate::Started
        } else {
            BeginCreate::AlreadyTracked
        })
    }

    pub fn mark_ready(&self, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE workspaces SET state = 'ready', updated_at = ?2 WHERE name = ?1",
            params![name, now_unix()],
        )?;
        Ok(())
    }

    /// Journal the intent to remove workspace `name`. A row left in
    /// `removing` means the removal was interrupted; repair completes it.
    pub fn begin_remove(&self, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE workspaces SET state = 'removing', updated_at = ?2 WHERE name = ?1",
            params![name, now_unix()],
        )?;
        Ok(())
    }

    pub fn finish_remove(&self, name: &str) -> Result<()> {
        self.delete_workspace_row(name)
    }

    pub fn delete_workspace_row(&self, name: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM workspaces WHERE name = ?1", params![name])?;
        Ok(())
    }

    pub fn workspace(&self, name: &str) -> Result<Option<WorkspaceRecord>> {
        self.conn
            .query_row(
                "SELECT name, state, base_snapshot, created_at, updated_at
                 FROM workspaces WHERE name = ?1",
                params![name],
                row_to_workspace,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn workspaces(&self) -> Result<Vec<WorkspaceRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, state, base_snapshot, created_at, updated_at
             FROM workspaces ORDER BY name",
        )?;
        let rows = stmt.query_map([], row_to_workspace)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ---------- operation journal ----------

    /// Journal an operation's intent. Call [`Registry::finish_op`] once the
    /// operation's effects are durably recorded; a row left behind is picked
    /// up by `doctor --repair`.
    pub fn begin_op(
        &self,
        kind: &str,
        workspace: &str,
        base_snapshot: Option<&str>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO pending_ops(kind, workspace_name, base_snapshot, started_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![kind, workspace, base_snapshot, now_unix()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn finish_op(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM pending_ops WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn pending_ops(&self) -> Result<Vec<PendingOp>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, workspace_name, base_snapshot, started_at
             FROM pending_ops ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(PendingOp {
                id: row.get(0)?,
                kind: row.get(1)?,
                workspace_name: row.get(2)?,
                base_snapshot: row.get(3)?,
                started_at_unix: row.get::<_, i64>(4)? as u64,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ---------- changesets ----------

    pub fn record_changeset(&self, c: &ChangesetRecord) -> Result<()> {
        let touched = serde_json::to_string(&c.touched_paths)
            .map_err(|e| RegistryError(format!("cannot serialize touched_paths: {}", e)))?;
        self.conn.execute(
            "INSERT OR REPLACE INTO changesets
               (id, workspace_name, base_snapshot, result_snapshot, touched_paths, created_at,
                exported_branch, exported_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                c.id,
                c.workspace_name,
                c.base_snapshot,
                c.result_snapshot,
                touched,
                c.created_at_unix as i64,
                c.exported_branch,
                c.exported_at_unix.map(|v| v as i64),
            ],
        )?;
        Ok(())
    }

    pub fn mark_exported(&self, id: &str, branch: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE changesets SET exported_branch = ?2, exported_at = ?3 WHERE id = ?1",
            params![id, branch, now_unix()],
        )?;
        Ok(())
    }

    pub fn changeset(&self, id: &str) -> Result<Option<ChangesetRecord>> {
        self.conn
            .query_row(
                "SELECT id, workspace_name, base_snapshot, result_snapshot, touched_paths,
                        created_at, exported_branch, exported_at
                 FROM changesets WHERE id = ?1",
                params![id],
                row_to_changeset,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn changesets(&self) -> Result<Vec<ChangesetRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, workspace_name, base_snapshot, result_snapshot, touched_paths,
                    created_at, exported_branch, exported_at
             FROM changesets ORDER BY created_at DESC, id",
        )?;
        let rows = stmt.query_map([], row_to_changeset)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ---------- changeset checks ----------

    /// Record a conflict check. Re-checking the same (changeset, target
    /// snapshot) pair replaces the row — the merge result is deterministic,
    /// only `checked_at` moves.
    pub fn record_check(&self, c: &ChangesetCheckRecord) -> Result<()> {
        let paths = serde_json::to_string(&c.conflict_paths)
            .map_err(|e| RegistryError(format!("cannot serialize conflict_paths: {}", e)))?;
        self.conn.execute(
            "INSERT OR REPLACE INTO changeset_checks
               (changeset_id, target_ref, target_snapshot, clean, conflict_paths, checked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                c.changeset_id,
                c.target_ref,
                c.target_snapshot,
                c.clean,
                paths,
                c.checked_at_unix as i64,
            ],
        )?;
        Ok(())
    }

    /// Paths touched by at least `min_changesets` distinct changesets,
    /// hottest first. Uses SQLite's `json_each` over the stored
    /// `touched_paths` arrays — one query, no table scan in Rust.
    pub fn hotspots(&self, min_changesets: u32, limit: u32) -> Result<Vec<HotspotRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT je.value, COUNT(DISTINCT c.id), COUNT(DISTINCT c.workspace_name)
             FROM changesets c, json_each(c.touched_paths) je
             GROUP BY je.value
             HAVING COUNT(DISTINCT c.id) >= ?1
             ORDER BY COUNT(DISTINCT c.id) DESC, je.value
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![min_changesets, limit], |row| {
            Ok(HotspotRecord {
                path: row.get(0)?,
                changeset_count: row.get::<_, i64>(1)? as u64,
                workspace_count: row.get::<_, i64>(2)? as u64,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// The most recent check recorded for `changeset_id`, if any.
    pub fn latest_check(&self, changeset_id: &str) -> Result<Option<ChangesetCheckRecord>> {
        self.conn
            .query_row(
                "SELECT changeset_id, target_ref, target_snapshot, clean, conflict_paths, checked_at
                 FROM changeset_checks WHERE changeset_id = ?1
                 ORDER BY checked_at DESC, target_snapshot LIMIT 1",
                params![changeset_id],
                row_to_check,
            )
            .optional()
            .map_err(Into::into)
    }
}

fn row_to_workspace(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceRecord> {
    let state_s: String = row.get(1)?;
    Ok(WorkspaceRecord {
        name: row.get(0)?,
        // CHECK constraint guarantees a known value; default defensively.
        state: WorkspaceState::from_str(&state_s).unwrap_or(WorkspaceState::Ready),
        base_snapshot: row.get(2)?,
        created_at_unix: row.get::<_, i64>(3)? as u64,
        updated_at_unix: row.get::<_, i64>(4)? as u64,
    })
}

fn row_to_check(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChangesetCheckRecord> {
    let paths_s: String = row.get(4)?;
    Ok(ChangesetCheckRecord {
        changeset_id: row.get(0)?,
        target_ref: row.get(1)?,
        target_snapshot: row.get(2)?,
        clean: row.get(3)?,
        conflict_paths: serde_json::from_str(&paths_s).unwrap_or_default(),
        checked_at_unix: row.get::<_, i64>(5)? as u64,
    })
}

fn row_to_changeset(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChangesetRecord> {
    let touched_s: String = row.get(4)?;
    Ok(ChangesetRecord {
        id: row.get(0)?,
        workspace_name: row.get(1)?,
        base_snapshot: row.get(2)?,
        result_snapshot: row.get(3)?,
        touched_paths: serde_json::from_str(&touched_s).unwrap_or_default(),
        created_at_unix: row.get::<_, i64>(5)? as u64,
        exported_branch: row.get(6)?,
        exported_at_unix: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
    })
}
