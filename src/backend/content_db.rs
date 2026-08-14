
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use super::display_path;

pub struct ContentDbEntry {
    pub path: String,
    pub hash_hex: String,
}

pub struct ContentDatabase {
    pub path: PathBuf,
    pub version_id: i64,
}

pub fn build_content_database(
    db_path: &Path,
    blob_cache_dir: &Path,
    entries: &[ContentDbEntry],
    manifest_hash_hex: &str,
    fork_id: Option<&str>,
    build_version: Option<&str>,
    engine_version: &str,
) -> Result<ContentDatabase> {
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", display_path(parent)))?;
    }

    if db_path.exists() {
        fs::remove_file(db_path)
            .with_context(|| format!("removing stale {}", display_path(db_path)))?;
    }

    let conn = Connection::open(db_path)
        .with_context(|| format!("opening content db {}", display_path(db_path)))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .context("enabling WAL mode on content db")?;

    create_schema(&conn)?;

    let manifest_hash_bytes = hex::decode(manifest_hash_hex)
        .with_context(|| format!("invalid manifest hash {manifest_hash_hex:?}"))?;

    conn.execute(
        "INSERT INTO ContentVersion (Hash, ForkId, ForkVersion, LastUsed, ZipHash)
         VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP, NULL)",
        params![
            manifest_hash_bytes,
            fork_id.unwrap_or(""),
            build_version.unwrap_or(""),
        ],
    )
    .context("inserting ContentVersion")?;
    let version_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO ContentEngineDependency (VersionId, ModuleName, ModuleVersion)
         VALUES (?1, ?2, ?3)",
        params![version_id, "Robust", engine_version],
    )
    .context("inserting engine dependency")?;

    let mut seen_hashes: Vec<String> = Vec::new();
    let mut seen_set: std::collections::HashSet<String> = Default::default();
    for entry in entries {
        if seen_set.insert(entry.hash_hex.clone()) {
            seen_hashes.push(entry.hash_hex.clone());
        }
    }

    let build = || -> Result<()> {
        conn.execute("BEGIN TRANSACTION", [])
            .context("beginning content db transaction")?;

        let mut content_ids: std::collections::HashMap<String, i64> = Default::default();
        for hash_hex in &seen_hashes {
            let blob_path = blob_cache_dir.join(hash_hex);
            let data = fs::read(&blob_path)
                .with_context(|| format!("reading content blob {}", display_path(&blob_path)))?;

            conn.execute(
                "INSERT INTO Content (Hash, Size, Compression, Data) VALUES (?1, ?2, 0, ?3)",
                params![decoded_bytes(hash_hex)?, data.len() as i64, data],
            )
            .with_context(|| format!("inserting content blob {hash_hex}"))?;

            content_ids.insert(hash_hex.clone(), conn.last_insert_rowid());
        }

        {
            let mut stmt = conn
                .prepare(
                    "INSERT INTO ContentManifest (VersionId, Path, ContentId)
                     VALUES (?1, ?2, ?3)",
                )
                .context("preparing ContentManifest insert")?;

            for entry in entries {
                let content_id = content_ids
                    .get(&entry.hash_hex)
                    .copied()
                    .context("missing content id for manifest path")?;

                stmt.execute(params![version_id, entry.path, content_id])
                    .with_context(|| format!("inserting manifest path {}", entry.path))?;
            }
        }

        conn.execute("COMMIT", [])
            .context("committing content db transaction")?;
        Ok(())
    };

    if let Err(err) = build() {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err);
    }

    Ok(ContentDatabase {
        path: db_path.to_path_buf(),
        version_id,
    })
}

fn decoded_bytes(hash_hex: &str) -> Result<Vec<u8>> {
    hex::decode(hash_hex).with_context(|| format!("invalid hex hash {hash_hex:?}"))
}

fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE ContentVersion(
            Id INTEGER PRIMARY KEY AUTOINCREMENT,
            Hash BLOB NOT NULL,
            ForkId TEXT NULL,
            ForkVersion TEXT NULL,
            LastUsed DATE NOT NULL,
            ZipHash BLOB NULL
        );

        CREATE TABLE Content(
            Id INTEGER PRIMARY KEY AUTOINCREMENT,
            Hash BLOB NOT NULL UNIQUE,
            Size INTEGER NOT NULL,
            Compression INTEGER NOT NULL,
            Data BLOB NOT NULL,
            CONSTRAINT UncompressedSameSize CHECK(Compression != 0 OR length(Data) = Size)
        );

        CREATE TABLE ContentManifest(
            Id INTEGER PRIMARY KEY,
            VersionId INTEGER NOT NULL REFERENCES ContentVersion(Id) ON DELETE CASCADE,
            Path TEXT NOT NULL,
            ContentId INTEGER NOT NULL REFERENCES Content(Id) ON DELETE RESTRICT,
            CONSTRAINT NotDirectory CHECK (Path NOT LIKE '%/')
        );

        CREATE UNIQUE INDEX ContentManifestUniqueIndex ON ContentManifest(VersionId, Path);
        CREATE INDEX ContentManifest_ContentId ON ContentManifest(ContentId);

        CREATE TABLE ContentEngineDependency(
            Id INTEGER PRIMARY KEY,
            VersionId INTEGER NOT NULL REFERENCES ContentVersion(Id) ON DELETE CASCADE,
            ModuleName TEXT NOT NULL,
            ModuleVersion TEXT NOT NULL
        );

        CREATE UNIQUE INDEX ContentEngineModuleUniqueIndex ON ContentEngineDependency(VersionId, ModuleName);

        CREATE TABLE RunningClient(
            ProcessId INTEGER PRIMARY KEY NOT NULL,
            MainModule TEXT NOT NULL,
            UsedVersion INTEGER NOT NULL REFERENCES ContentVersion(Id) ON DELETE RESTRICT
        );

        CREATE TABLE InterruptedDownload(
            Id INTEGER PRIMARY KEY AUTOINCREMENT,
            Added DATE NOT NULL
        );

        CREATE TABLE InterruptedDownloadContent(
            Id INTEGER PRIMARY KEY AUTOINCREMENT,
            InterruptedDownloadId INTEGER NOT NULL,
            ContentId INTEGER NOT NULL UNIQUE
        );
        "#,
    )
    .context("creating content db schema")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_usable_database() {
        let tmp = std::env::temp_dir().join(format!("content-db-test-{}", std::process::id()));
        let cache_dir = tmp.join("cache");
        fs::create_dir_all(&cache_dir).unwrap();

        let data = b"hello content db";
        let hash = crate::backend::content::hash_bytes_hex(data);
        fs::write(cache_dir.join(&hash), data).unwrap();

        let entries = vec![
            ContentDbEntry {
                path: "manifest.yml".into(),
                hash_hex: hash.clone(),
            },
            ContentDbEntry {
                path: "Content.Client.dll".into(),
                hash_hex: hash.clone(),
            },
        ];

        let db = build_content_database(
            &tmp.join("content.db"),
            &cache_dir,
            &entries,
            &crate::backend::content::hash_bytes_hex(b"manifest"),
            Some("fork"),
            Some("1.0"),
            "264.0.2",
        )
        .unwrap();

        let conn = Connection::open(&db.path).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ContentManifest WHERE VersionId = ?1",
                params![db.version_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

        let row: (i64, String) = conn
            .query_row(
                "SELECT cm.VersionId, cm.Path FROM ContentManifest cm WHERE cm.Path = ?1",
                params!["Content.Client.dll"],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row.0, db.version_id);
        assert_eq!(row.1, "Content.Client.dll");
    }
}
