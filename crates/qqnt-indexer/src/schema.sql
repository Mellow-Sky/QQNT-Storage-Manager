CREATE TABLE accounts(root TEXT PRIMARY KEY, adapter_version TEXT NOT NULL);
CREATE TABLE scan_sessions(id TEXT PRIMARY KEY,root TEXT NOT NULL REFERENCES accounts(root),started INTEGER NOT NULL,finished INTEGER,status TEXT NOT NULL,detection TEXT NOT NULL,summary TEXT);
CREATE TABLE indexed_files(session_id TEXT NOT NULL REFERENCES scan_sessions(id),path TEXT NOT NULL,kind TEXT NOT NULL,risk TEXT NOT NULL,size INTEGER NOT NULL,modified INTEGER,payload TEXT NOT NULL,PRIMARY KEY(session_id,path));
CREATE INDEX files_page ON indexed_files(session_id,kind,size DESC);
CREATE INDEX files_time ON indexed_files(session_id,modified);
CREATE TABLE classification_results(session_id TEXT NOT NULL,path TEXT NOT NULL,risk TEXT NOT NULL,evidence TEXT NOT NULL,PRIMARY KEY(session_id,path),FOREIGN KEY(session_id,path) REFERENCES indexed_files(session_id,path));
-- Reserved for later phases. Phase 1 has no mutation or cleanup commands.
CREATE TABLE cleanup_plans(id TEXT PRIMARY KEY,created INTEGER NOT NULL,payload TEXT NOT NULL);
CREATE TABLE cleanup_operations(id TEXT PRIMARY KEY,plan_id TEXT NOT NULL REFERENCES cleanup_plans(id),status TEXT NOT NULL,payload TEXT NOT NULL);
CREATE TABLE protected_items(root TEXT NOT NULL,path TEXT NOT NULL,PRIMARY KEY(root,path));
CREATE TABLE app_settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
PRAGMA user_version=1;
