-- heliOS schema · migration 0002 · OS Entities
--
-- Adds the entity kinds that exist only in the OS context: every running
-- process, every observed file, every network connection, every applet
-- instance, and the canvas itself (desktops + per-entity placement).
--
-- These are the rows the canvas compositor draws. Combined with 0001 they
-- form the full entity graph: H concepts (projects, agents, tasks, memory)
-- + OS concepts (processes, files, net, applets, desktops) under one
-- relational namespace, all keyed by project_id where applicable.

-- ============================================================================
-- Processes — every PID observed by the events bus
--
-- Populated by the system-events bus from sched_process_exec / exit eBPF
-- probes plus /proc enrichment. Lifetime mirrors the OS PID. Long-lived
-- managed processes (terminals, dev servers, agent CC instances) are also
-- referenced from 0001's `terminals` / `agent_instances` tables; this is
-- the universal table.
-- ============================================================================
CREATE TABLE IF NOT EXISTS processes (
  pid INTEGER PRIMARY KEY,
  ppid INTEGER,
  cmdline TEXT NOT NULL DEFAULT '',
  exe TEXT,
  comm TEXT NOT NULL DEFAULT '',
  uid INTEGER NOT NULL DEFAULT 0,
  gid INTEGER NOT NULL DEFAULT 0,
  cgroup TEXT,
  systemd_unit TEXT,
  project_id TEXT,
  agent_id TEXT,
  status TEXT NOT NULL DEFAULT 'running'
    CHECK(status IN ('running','sleeping','zombie','stopped','dead')),
  rss_kb INTEGER,
  cpu_percent REAL,
  started_at TEXT NOT NULL DEFAULT (datetime('now')),
  exited_at TEXT,
  exit_code INTEGER,
  FOREIGN KEY (project_id) REFERENCES projects(id),
  FOREIGN KEY (agent_id) REFERENCES agent_instances(id)
);

-- ============================================================================
-- Files — observed by fanotify or eBPF LSM hooks; not every file on disk,
-- only those that have been read/written/created during the session and
-- entities the user or agent is touching. Stale files age out.
-- ============================================================================
CREATE TABLE IF NOT EXISTS files (
  id TEXT PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  kind TEXT NOT NULL DEFAULT 'unknown'
    CHECK(kind IN ('source','config','binary','document','image','video','archive','log','data','unknown')),
  size_bytes INTEGER,
  mtime TEXT,
  project_id TEXT,
  tags_json TEXT NOT NULL DEFAULT '[]',
  embedding_id TEXT,
  last_accessed_at TEXT NOT NULL DEFAULT (datetime('now')),
  access_count INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  FOREIGN KEY (project_id) REFERENCES projects(id)
);

-- ============================================================================
-- Network connections — sockets observed by sock_diag / eBPF tcp probes.
-- Only outbound and listening sockets we care about. Lifetime = TCP
-- connection lifetime.
-- ============================================================================
CREATE TABLE IF NOT EXISTS network_connections (
  id TEXT PRIMARY KEY,
  pid INTEGER,
  protocol TEXT NOT NULL CHECK(protocol IN ('tcp','udp','unix')),
  direction TEXT NOT NULL CHECK(direction IN ('inbound','outbound','listen')),
  local_addr TEXT NOT NULL,
  local_port INTEGER,
  remote_addr TEXT,
  remote_port INTEGER,
  state TEXT NOT NULL DEFAULT 'established'
    CHECK(state IN ('established','listen','time_wait','close_wait','closed','syn_sent','syn_recv')),
  bytes_sent INTEGER NOT NULL DEFAULT 0,
  bytes_received INTEGER NOT NULL DEFAULT 0,
  established_at TEXT NOT NULL DEFAULT (datetime('now')),
  closed_at TEXT,
  FOREIGN KEY (pid) REFERENCES processes(pid)
);

-- ============================================================================
-- Applets — installed and running WASM applets
-- ============================================================================
CREATE TABLE IF NOT EXISTS applets (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT,
  source TEXT NOT NULL DEFAULT 'installed'
    CHECK(source IN ('installed','agent_emitted_ephemeral','agent_emitted_persistent')),
  manifest_json TEXT NOT NULL DEFAULT '{}',
  wasm_path TEXT NOT NULL,
  wit_world TEXT NOT NULL DEFAULT 'host:ui/canvas',
  capabilities_json TEXT NOT NULL DEFAULT '[]',
  signature TEXT,
  status TEXT NOT NULL DEFAULT 'idle'
    CHECK(status IN ('idle','running','suspended','crashed','revoked')),
  installed_at TEXT NOT NULL DEFAULT (datetime('now')),
  last_run_at TEXT,
  author_agent_id TEXT,
  project_id TEXT,
  FOREIGN KEY (author_agent_id) REFERENCES agent_instances(id),
  FOREIGN KEY (project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS applet_instances (
  id TEXT PRIMARY KEY,
  applet_id TEXT NOT NULL,
  state_json TEXT NOT NULL DEFAULT '{}',
  bound_entity_kind TEXT,
  bound_entity_id TEXT,
  started_at TEXT NOT NULL DEFAULT (datetime('now')),
  stopped_at TEXT,
  FOREIGN KEY (applet_id) REFERENCES applets(id)
);

-- ============================================================================
-- Desktops — the canvas is partitioned into named workspaces, each itself
-- an entity. Pan-between-desktops is the navigation primitive. A desktop
-- *contains* canvas_entities.
-- ============================================================================
CREATE TABLE IF NOT EXISTS desktops (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  project_id TEXT,
  origin_x REAL NOT NULL DEFAULT 0,
  origin_y REAL NOT NULL DEFAULT 0,
  default_zoom REAL NOT NULL DEFAULT 1.0,
  background_json TEXT NOT NULL DEFAULT '{}',
  is_active INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now')),
  FOREIGN KEY (project_id) REFERENCES projects(id)
);

-- ============================================================================
-- Canvas entities — the unifying placement table. Every visible thing on
-- the canvas (process, file, applet, agent, terminal, etc.) is referenced
-- by a row here with (x, y, scale, rotation, z, desktop_id). The compositor
-- queries this table to know what to draw.
--
-- entity_kind discriminates which table holds the underlying entity:
--   'process'      -> processes.pid (entity_id = pid as text)
--   'file'         -> files.id
--   'applet'       -> applet_instances.id
--   'agent'        -> agent_instances.id
--   'terminal'     -> terminals.id
--   'task'         -> tasks.id
--   'project'      -> projects.id
--   'connection'   -> network_connections.id
--   'desktop'      -> desktops.id  (a desktop nested inside another desktop)
-- ============================================================================
CREATE TABLE IF NOT EXISTS canvas_entities (
  id TEXT PRIMARY KEY,
  desktop_id TEXT NOT NULL,
  entity_kind TEXT NOT NULL
    CHECK(entity_kind IN ('process','file','applet','agent','terminal','task','project','connection','desktop')),
  entity_id TEXT NOT NULL,
  x REAL NOT NULL DEFAULT 0,
  y REAL NOT NULL DEFAULT 0,
  scale REAL NOT NULL DEFAULT 1.0,
  rotation REAL NOT NULL DEFAULT 0,
  z INTEGER NOT NULL DEFAULT 0,
  width REAL,
  height REAL,
  pinned INTEGER NOT NULL DEFAULT 0,
  visible INTEGER NOT NULL DEFAULT 1,
  relevance REAL NOT NULL DEFAULT 0.5,
  attached_applet_ids_json TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now')),
  UNIQUE(desktop_id, entity_kind, entity_id),
  FOREIGN KEY (desktop_id) REFERENCES desktops(id) ON DELETE CASCADE
);

-- ============================================================================
-- Indexes
-- ============================================================================
CREATE INDEX IF NOT EXISTS idx_processes_ppid ON processes(ppid);
CREATE INDEX IF NOT EXISTS idx_processes_project ON processes(project_id);
CREATE INDEX IF NOT EXISTS idx_processes_agent ON processes(agent_id);
CREATE INDEX IF NOT EXISTS idx_processes_status ON processes(status);
CREATE INDEX IF NOT EXISTS idx_processes_cgroup ON processes(cgroup);
CREATE INDEX IF NOT EXISTS idx_processes_systemd_unit ON processes(systemd_unit);

CREATE INDEX IF NOT EXISTS idx_files_kind ON files(kind);
CREATE INDEX IF NOT EXISTS idx_files_project ON files(project_id);
CREATE INDEX IF NOT EXISTS idx_files_last_accessed ON files(last_accessed_at);

CREATE INDEX IF NOT EXISTS idx_netconn_pid ON network_connections(pid);
CREATE INDEX IF NOT EXISTS idx_netconn_state ON network_connections(state);
CREATE INDEX IF NOT EXISTS idx_netconn_remote ON network_connections(remote_addr, remote_port);

CREATE INDEX IF NOT EXISTS idx_applets_status ON applets(status);
CREATE INDEX IF NOT EXISTS idx_applets_source ON applets(source);
CREATE INDEX IF NOT EXISTS idx_applets_project ON applets(project_id);
CREATE INDEX IF NOT EXISTS idx_applet_instances_applet ON applet_instances(applet_id);

CREATE INDEX IF NOT EXISTS idx_desktops_project ON desktops(project_id);
CREATE INDEX IF NOT EXISTS idx_desktops_active ON desktops(is_active);

CREATE INDEX IF NOT EXISTS idx_canvas_desktop ON canvas_entities(desktop_id);
CREATE INDEX IF NOT EXISTS idx_canvas_entity ON canvas_entities(entity_kind, entity_id);
CREATE INDEX IF NOT EXISTS idx_canvas_visible ON canvas_entities(desktop_id, visible);
CREATE INDEX IF NOT EXISTS idx_canvas_relevance ON canvas_entities(relevance);

-- ============================================================================
-- FTS5 — fast text search over the things we'll search most: memory,
-- messages, files, blackboard. Other tables can be added in later
-- migrations.
-- ============================================================================
CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
  content,
  tags,
  content=memory_records,
  content_rowid=rowid
);

CREATE VIRTUAL TABLE IF NOT EXISTS file_fts USING fts5(
  path,
  tags,
  content=files,
  content_rowid=rowid
);

CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(
  content,
  content=messages,
  content_rowid=rowid
);

-- Trigger maintenance for FTS indexes
CREATE TRIGGER IF NOT EXISTS memory_fts_insert AFTER INSERT ON memory_records BEGIN
  INSERT INTO memory_fts(rowid, content, tags) VALUES (new.rowid, new.content, new.tags_json);
END;

CREATE TRIGGER IF NOT EXISTS memory_fts_delete AFTER DELETE ON memory_records BEGIN
  INSERT INTO memory_fts(memory_fts, rowid, content, tags) VALUES('delete', old.rowid, old.content, old.tags_json);
END;

CREATE TRIGGER IF NOT EXISTS memory_fts_update AFTER UPDATE ON memory_records BEGIN
  INSERT INTO memory_fts(memory_fts, rowid, content, tags) VALUES('delete', old.rowid, old.content, old.tags_json);
  INSERT INTO memory_fts(rowid, content, tags) VALUES (new.rowid, new.content, new.tags_json);
END;

CREATE TRIGGER IF NOT EXISTS file_fts_insert AFTER INSERT ON files BEGIN
  INSERT INTO file_fts(rowid, path, tags) VALUES (new.rowid, new.path, new.tags_json);
END;

CREATE TRIGGER IF NOT EXISTS message_fts_insert AFTER INSERT ON messages BEGIN
  INSERT INTO message_fts(rowid, content) VALUES (new.rowid, new.content);
END;
