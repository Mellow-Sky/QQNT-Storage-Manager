BEGIN IMMEDIATE;
CREATE TABLE file_removals(root TEXT NOT NULL,path TEXT NOT NULL,fingerprint TEXT NOT NULL,removed_at INTEGER NOT NULL,PRIMARY KEY(root,path,fingerprint));
PRAGMA user_version=4;
COMMIT;
