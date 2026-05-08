-- heliOS schema · migration 0001 · H Origin
--
-- Direct port of D:/dev/H/packages/db/src/schema.sql
-- The H entity graph survives in heliOS as the canonical projection of the
-- system-events bus. Tables ported verbatim. New OS entities (processes,
-- files, network connections, applets, desktops) live in 0002.
--
-- Reference: docs/research/05-h-reuse-audit.md

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- ============================================================================
-- Projects
-- ============================================================================
CREATE TABLE IF NOT EXISTS projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  path TEXT NOT NULL,
  description TEXT,
  status TEXT NOT NULL DEFAULT 'active'
    CHECK(status IN ('active', 'paused', 'archived')),
  config_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ============================================================================
-- Agent Definitions (role schemas)
-- ============================================================================
CREATE TABLE IF NOT EXISTS agent_definitions (
  role TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT,
  system_prompt TEXT NOT NULL,
  capabilities_json TEXT NOT NULL DEFAULT '[]',
  llm_provider TEXT NOT NULL DEFAULT 'claude-code',
  model TEXT,
  max_concurrent_tasks INTEGER NOT NULL DEFAULT 1,
  temperature REAL NOT NULL DEFAULT 0.7,
  token_budget INTEGER NOT NULL DEFAULT 100000,
  max_turns INTEGER NOT NULL DEFAULT 50,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ============================================================================
-- Agent Instances (running agents — most are spawned Claude Code processes)
-- ============================================================================
CREATE TABLE IF NOT EXISTS agent_instances (
  id TEXT PRIMARY KEY,
  definition_role TEXT NOT NULL,
  project_id TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'spawning'
    CHECK(status IN ('spawning','idle','working','paused','terminated','error')),
  current_task_id TEXT,
  pid INTEGER,
  token_budget INTEGER NOT NULL DEFAULT 100000,
  turn_count INTEGER NOT NULL DEFAULT 0,
  spawned_at TEXT NOT NULL DEFAULT (datetime('now')),
  last_active_at TEXT NOT NULL DEFAULT (datetime('now')),
  terminated_at TEXT,
  error_message TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now')),
  FOREIGN KEY (definition_role) REFERENCES agent_definitions(role),
  FOREIGN KEY (project_id) REFERENCES projects(id)
);

-- ============================================================================
-- Tasks
-- ============================================================================
CREATE TABLE IF NOT EXISTS tasks (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  parent_task_id TEXT,
  title TEXT NOT NULL,
  description TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending'
    CHECK(status IN ('pending','assigned','in_progress','review',
                     'completed','failed','blocked','cancelled')),
  priority TEXT NOT NULL DEFAULT 'medium'
    CHECK(priority IN ('critical','high','medium','low')),
  required_role TEXT NOT NULL DEFAULT 'coder',
  assigned_agent_id TEXT,
  dependencies_json TEXT NOT NULL DEFAULT '[]',
  subtasks_json TEXT NOT NULL DEFAULT '[]',
  result_json TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now')),
  started_at TEXT,
  completed_at TEXT,
  FOREIGN KEY (project_id) REFERENCES projects(id),
  FOREIGN KEY (parent_task_id) REFERENCES tasks(id),
  FOREIGN KEY (assigned_agent_id) REFERENCES agent_instances(id)
);

-- ============================================================================
-- Events (append-only event store; system-events-bus also persists here)
-- ============================================================================
CREATE TABLE IF NOT EXISTS events (
  id TEXT PRIMARY KEY,
  type TEXT NOT NULL,
  timestamp TEXT NOT NULL DEFAULT (datetime('now')),
  project_id TEXT,
  agent_id TEXT,
  task_id TEXT,
  payload_json TEXT NOT NULL DEFAULT '{}',
  source TEXT NOT NULL,
  correlation_id TEXT,
  causation_id TEXT
);

-- ============================================================================
-- Memory Records
-- ============================================================================
CREATE TABLE IF NOT EXISTS memory_records (
  id TEXT PRIMARY KEY,
  project_id TEXT,
  agent_id TEXT,
  type TEXT NOT NULL
    CHECK(type IN ('fact','decision','pattern','preference','context','error_lesson')),
  content TEXT NOT NULL,
  tags_json TEXT NOT NULL DEFAULT '[]',
  importance REAL NOT NULL DEFAULT 0.5,
  access_count INTEGER NOT NULL DEFAULT 0,
  last_accessed_at TEXT NOT NULL DEFAULT (datetime('now')),
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  expires_at TEXT
);

-- ============================================================================
-- Conversations + Messages
-- ============================================================================
CREATE TABLE IF NOT EXISTS conversations (
  id TEXT PRIMARY KEY,
  project_id TEXT,
  agent_id TEXT,
  task_id TEXT,
  interface_source TEXT NOT NULL DEFAULT 'system'
    CHECK(interface_source IN ('telegram','api','cli','system','websocket','shell')),
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS messages (
  id TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL,
  role TEXT NOT NULL CHECK(role IN ('user','agent','system','tool')),
  agent_id TEXT,
  content TEXT NOT NULL,
  tool_calls_json TEXT,
  tool_results_json TEXT,
  token_count INTEGER,
  timestamp TEXT NOT NULL DEFAULT (datetime('now')),
  FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);

-- ============================================================================
-- Task Graphs (DAG decomposition)
-- ============================================================================
CREATE TABLE IF NOT EXISTS task_graphs (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  root_task_id TEXT,
  nodes_json TEXT NOT NULL DEFAULT '[]',
  strategy TEXT NOT NULL DEFAULT 'mixed'
    CHECK(strategy IN ('sequential', 'parallel', 'mixed')),
  status TEXT NOT NULL DEFAULT 'planning'
    CHECK(status IN ('planning', 'executing', 'completed', 'failed')),
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  completed_at TEXT,
  FOREIGN KEY (project_id) REFERENCES projects(id)
);

-- ============================================================================
-- Agent Checkpoints
-- ============================================================================
CREATE TABLE IF NOT EXISTS agent_checkpoints (
  id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  timestamp TEXT NOT NULL DEFAULT (datetime('now')),
  turn_count INTEGER NOT NULL DEFAULT 0,
  context_anchor_json TEXT NOT NULL DEFAULT '{}',
  recent_messages_json TEXT NOT NULL DEFAULT '[]',
  token_usage_json TEXT NOT NULL DEFAULT '{}',
  git_ref TEXT,
  FOREIGN KEY (agent_id) REFERENCES agent_instances(id),
  FOREIGN KEY (task_id) REFERENCES tasks(id)
);

-- ============================================================================
-- Blackboard
-- ============================================================================
CREATE TABLE IF NOT EXISTS blackboard_entries (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  agent_id TEXT NOT NULL,
  task_id TEXT,
  type TEXT NOT NULL
    CHECK(type IN ('hypothesis','decision','blocker','discovery','code_context','test_result','review_comment')),
  content TEXT NOT NULL,
  confidence REAL NOT NULL DEFAULT 0.5,
  resolved INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  FOREIGN KEY (project_id) REFERENCES projects(id)
);

-- ============================================================================
-- Episodes
-- ============================================================================
CREATE TABLE IF NOT EXISTS episodes (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  task_type TEXT NOT NULL,
  summary TEXT NOT NULL,
  outcome TEXT NOT NULL CHECK(outcome IN ('success', 'failure', 'partial')),
  lessons_json TEXT NOT NULL DEFAULT '[]',
  files_json TEXT NOT NULL DEFAULT '[]',
  token_cost INTEGER NOT NULL DEFAULT 0,
  duration_ms INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  FOREIGN KEY (project_id) REFERENCES projects(id)
);

-- ============================================================================
-- Trace Spans
-- ============================================================================
CREATE TABLE IF NOT EXISTS trace_spans (
  id TEXT PRIMARY KEY,
  trace_id TEXT NOT NULL,
  parent_span_id TEXT,
  agent_id TEXT,
  task_id TEXT,
  operation TEXT NOT NULL,
  start_time TEXT NOT NULL DEFAULT (datetime('now')),
  end_time TEXT,
  status TEXT NOT NULL DEFAULT 'ok' CHECK(status IN ('ok', 'error')),
  input_tokens INTEGER,
  output_tokens INTEGER,
  cost_usd REAL,
  model TEXT,
  tool_name TEXT,
  error_message TEXT
);

-- ============================================================================
-- Cost Records
-- ============================================================================
CREATE TABLE IF NOT EXISTS cost_records (
  id TEXT PRIMARY KEY,
  trace_id TEXT,
  agent_id TEXT,
  task_id TEXT,
  project_id TEXT,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  cost_usd REAL NOT NULL DEFAULT 0,
  timestamp TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ============================================================================
-- Tool Registry
-- ============================================================================
CREATE TABLE IF NOT EXISTS tool_registry (
  name TEXT PRIMARY KEY,
  description TEXT NOT NULL,
  input_schema_json TEXT NOT NULL,
  source TEXT NOT NULL DEFAULT 'builtin'
    CHECK(source IN ('builtin','mcp','plugin')),
  mcp_server_url TEXT,
  is_enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ============================================================================
-- Sessions
-- ============================================================================
CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  name TEXT,
  status TEXT NOT NULL DEFAULT 'active'
    CHECK(status IN ('active','paused','completed','abandoned')),
  started_at TEXT NOT NULL DEFAULT (datetime('now')),
  paused_at TEXT,
  resumed_at TEXT,
  completed_at TEXT,
  focus_description TEXT,
  config_json TEXT NOT NULL DEFAULT '{}',
  snapshot_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS session_projects (
  session_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  added_at TEXT NOT NULL DEFAULT (datetime('now')),
  is_primary INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (session_id, project_id),
  FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
  FOREIGN KEY (project_id) REFERENCES projects(id)
);

-- ============================================================================
-- Project Links
-- ============================================================================
CREATE TABLE IF NOT EXISTS project_links (
  id TEXT PRIMARY KEY,
  source_project_id TEXT NOT NULL,
  target_project_id TEXT NOT NULL,
  link_type TEXT NOT NULL DEFAULT 'related'
    CHECK(link_type IN ('related','depends_on','frontend_backend','monorepo_sibling','api_consumer')),
  description TEXT,
  config_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  FOREIGN KEY (source_project_id) REFERENCES projects(id),
  FOREIGN KEY (target_project_id) REFERENCES projects(id)
);

-- ============================================================================
-- Terminals (managed processes — H concept; see 0002 for the broader
-- 'processes' table that captures every PID, not just managed ones)
-- ============================================================================
CREATE TABLE IF NOT EXISTS terminals (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  agent_id TEXT,
  name TEXT NOT NULL,
  type TEXT NOT NULL DEFAULT 'shell'
    CHECK(type IN ('shell','claude_code_automated','claude_code_interactive','dev_server','watcher')),
  status TEXT NOT NULL DEFAULT 'starting'
    CHECK(status IN ('starting','running','stopped','crashed','completed')),
  pid INTEGER,
  command TEXT NOT NULL,
  args_json TEXT NOT NULL DEFAULT '[]',
  cwd TEXT NOT NULL,
  env_json TEXT NOT NULL DEFAULT '{}',
  exit_code INTEGER,
  started_at TEXT NOT NULL DEFAULT (datetime('now')),
  stopped_at TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now')),
  FOREIGN KEY (session_id) REFERENCES sessions(id),
  FOREIGN KEY (project_id) REFERENCES projects(id),
  FOREIGN KEY (agent_id) REFERENCES agent_instances(id)
);

-- ============================================================================
-- Agent Cards (A2A discovery)
-- ============================================================================
CREATE TABLE IF NOT EXISTS agent_cards (
  agent_id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT NOT NULL,
  project_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  capabilities_json TEXT NOT NULL DEFAULT '[]',
  skills_json TEXT NOT NULL DEFAULT '[]',
  endpoint TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'available'
    CHECK(status IN ('available','busy','offline')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now')),
  FOREIGN KEY (agent_id) REFERENCES agent_instances(id)
);

-- ============================================================================
-- A2A Messages
-- ============================================================================
CREATE TABLE IF NOT EXISTS a2a_messages (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  from_agent_id TEXT NOT NULL,
  to_agent_id TEXT,
  from_project_id TEXT NOT NULL,
  to_project_id TEXT,
  type TEXT NOT NULL DEFAULT 'message'
    CHECK(type IN ('message','task_request','task_response','artifact','query','notification','broadcast')),
  subject TEXT,
  body TEXT NOT NULL,
  artifacts_json TEXT NOT NULL DEFAULT '[]',
  correlation_id TEXT,
  in_reply_to TEXT,
  priority TEXT NOT NULL DEFAULT 'normal'
    CHECK(priority IN ('urgent','normal','low')),
  status TEXT NOT NULL DEFAULT 'pending'
    CHECK(status IN ('pending','delivered','read','processed','failed')),
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  delivered_at TEXT,
  processed_at TEXT,
  FOREIGN KEY (session_id) REFERENCES sessions(id),
  FOREIGN KEY (from_agent_id) REFERENCES agent_instances(id),
  FOREIGN KEY (in_reply_to) REFERENCES a2a_messages(id)
);

CREATE TABLE IF NOT EXISTS session_a2a_permissions (
  id TEXT PRIMARY KEY,
  from_session_id TEXT NOT NULL,
  to_session_id TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending'
    CHECK(status IN ('pending','granted','denied','revoked')),
  requested_by_agent_id TEXT,
  granted_at TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  UNIQUE(from_session_id, to_session_id)
);

-- ============================================================================
-- AutoDream runs
-- ============================================================================
CREATE TABLE IF NOT EXISTS autodream_runs (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  status TEXT NOT NULL
    CHECK(status IN ('running', 'completed', 'failed')),
  started_at TEXT NOT NULL DEFAULT (datetime('now')),
  finished_at TEXT,
  sessions_consolidated INTEGER NOT NULL DEFAULT 0,
  memories_extracted INTEGER NOT NULL DEFAULT 0,
  error_message TEXT
);

-- ============================================================================
-- Indexes
-- ============================================================================
CREATE INDEX IF NOT EXISTS idx_agents_project ON agent_instances(project_id);
CREATE INDEX IF NOT EXISTS idx_agents_status ON agent_instances(status);
CREATE INDEX IF NOT EXISTS idx_agents_pid ON agent_instances(pid);
CREATE INDEX IF NOT EXISTS idx_tasks_project ON tasks(project_id);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_assigned ON tasks(assigned_agent_id);
CREATE INDEX IF NOT EXISTS idx_tasks_parent ON tasks(parent_task_id);
CREATE INDEX IF NOT EXISTS idx_events_type ON events(type);
CREATE INDEX IF NOT EXISTS idx_events_project ON events(project_id);
CREATE INDEX IF NOT EXISTS idx_events_agent ON events(agent_id);
CREATE INDEX IF NOT EXISTS idx_events_task ON events(task_id);
CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
CREATE INDEX IF NOT EXISTS idx_events_correlation ON events(correlation_id);
CREATE INDEX IF NOT EXISTS idx_memory_project ON memory_records(project_id);
CREATE INDEX IF NOT EXISTS idx_memory_type ON memory_records(type);
CREATE INDEX IF NOT EXISTS idx_memory_importance ON memory_records(importance);
CREATE INDEX IF NOT EXISTS idx_messages_conversation ON messages(conversation_id);
CREATE INDEX IF NOT EXISTS idx_conversations_project ON conversations(project_id);
CREATE INDEX IF NOT EXISTS idx_task_graphs_project ON task_graphs(project_id);
CREATE INDEX IF NOT EXISTS idx_task_graphs_status ON task_graphs(status);
CREATE INDEX IF NOT EXISTS idx_checkpoints_agent ON agent_checkpoints(agent_id);
CREATE INDEX IF NOT EXISTS idx_checkpoints_task ON agent_checkpoints(task_id);
CREATE INDEX IF NOT EXISTS idx_blackboard_project ON blackboard_entries(project_id);
CREATE INDEX IF NOT EXISTS idx_blackboard_type ON blackboard_entries(type);
CREATE INDEX IF NOT EXISTS idx_blackboard_task ON blackboard_entries(task_id);
CREATE INDEX IF NOT EXISTS idx_episodes_project ON episodes(project_id);
CREATE INDEX IF NOT EXISTS idx_episodes_outcome ON episodes(outcome);
CREATE INDEX IF NOT EXISTS idx_traces_trace_id ON trace_spans(trace_id);
CREATE INDEX IF NOT EXISTS idx_traces_agent ON trace_spans(agent_id);
CREATE INDEX IF NOT EXISTS idx_traces_task ON trace_spans(task_id);
CREATE INDEX IF NOT EXISTS idx_cost_project ON cost_records(project_id);
CREATE INDEX IF NOT EXISTS idx_cost_agent ON cost_records(agent_id);
CREATE INDEX IF NOT EXISTS idx_cost_timestamp ON cost_records(timestamp);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
CREATE INDEX IF NOT EXISTS idx_session_projects_session ON session_projects(session_id);
CREATE INDEX IF NOT EXISTS idx_session_projects_project ON session_projects(project_id);
CREATE INDEX IF NOT EXISTS idx_project_links_source ON project_links(source_project_id);
CREATE INDEX IF NOT EXISTS idx_project_links_target ON project_links(target_project_id);
CREATE INDEX IF NOT EXISTS idx_terminals_session ON terminals(session_id);
CREATE INDEX IF NOT EXISTS idx_terminals_project ON terminals(project_id);
CREATE INDEX IF NOT EXISTS idx_terminals_agent ON terminals(agent_id);
CREATE INDEX IF NOT EXISTS idx_terminals_status ON terminals(status);
CREATE INDEX IF NOT EXISTS idx_agent_cards_session ON agent_cards(session_id);
CREATE INDEX IF NOT EXISTS idx_agent_cards_project ON agent_cards(project_id);
CREATE INDEX IF NOT EXISTS idx_agent_cards_status ON agent_cards(status);
CREATE INDEX IF NOT EXISTS idx_a2a_messages_session ON a2a_messages(session_id);
CREATE INDEX IF NOT EXISTS idx_a2a_messages_from ON a2a_messages(from_agent_id);
CREATE INDEX IF NOT EXISTS idx_a2a_messages_to ON a2a_messages(to_agent_id);
CREATE INDEX IF NOT EXISTS idx_a2a_messages_status ON a2a_messages(status);
CREATE INDEX IF NOT EXISTS idx_a2a_messages_correlation ON a2a_messages(correlation_id);
CREATE INDEX IF NOT EXISTS idx_a2a_perms_from ON session_a2a_permissions(from_session_id);
CREATE INDEX IF NOT EXISTS idx_a2a_perms_to ON session_a2a_permissions(to_session_id);
CREATE INDEX IF NOT EXISTS idx_a2a_perms_status ON session_a2a_permissions(status);
CREATE INDEX IF NOT EXISTS idx_autodream_project ON autodream_runs(project_id);
CREATE INDEX IF NOT EXISTS idx_autodream_status ON autodream_runs(status);
CREATE INDEX IF NOT EXISTS idx_autodream_started ON autodream_runs(started_at);
