//! Removing a library torrent (user delete, GC eviction): one path, under the
//! infohash lock, so its engine delete and its soft delete can't interleave
//! with a grab of the same release.

use iris_core::ids::TorrentId;
use iris_db::torrents::TorrentRow;
use sqlx::SqlitePool;

use crate::{Engine, EngineError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Removed {
    /// The engine still managed it.
    pub in_engine: bool,
    /// Bytes its exclusive files took on disk.
    pub freed_bytes: u64,
    /// The row was live until now (not removed meanwhile).
    pub row_was_live: bool,
}

/// Record the last upload delta (librqbit's counter dies with the torrent),
/// drop the torrent from the engine with the files no other torrent reads,
/// and soft-delete its row. A live row of the same name the engine doesn't
/// manage (it failed to restore) may share the files without the engine
/// knowing: they are all kept then.
///
/// # Errors
/// The engine refused the delete (the row is left live), or a DB write failed.
pub async fn remove_torrent(
    engine: &Engine,
    pool: &SqlitePool,
    row: &TorrentRow,
) -> anyhow::Result<Removed> {
    let held = engine.lock(&row.infohash).await;
    if let Some(snap) = engine.get_by_infohash(&row.infohash) {
        let _ =
            iris_db::torrents::reconcile_uploaded(pool, &row.infohash, snap.uploaded_bytes).await;
    }
    let keep_files = iris_db::torrents::list_active(pool)
        .await?
        .iter()
        .any(|t| t.infohash != row.infohash && t.name == row.name && !engine.contains(&t.infohash));
    let (in_engine, freed_bytes) = match engine
        .delete_by_infohash(&row.infohash, &held, keep_files)
        .await
    {
        Ok(freed) => (true, freed),
        Err(EngineError::NotFound) => (false, 0),
        Err(e) => return Err(anyhow::anyhow!("engine delete: {e}")),
    };
    let row_was_live = iris_db::torrents::soft_delete(pool, TorrentId::from(row.id)).await?;
    Ok(Removed {
        in_engine,
        freed_bytes,
        row_was_live,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use super::remove_torrent;
    use crate::Engine;

    /// A multi-file `.torrent` named `name`; `salt` (a `source` key) changes
    /// the infohash without changing the files, like one release on two
    /// trackers. Piece hashes are zeros: nothing is ever verified offline.
    pub(crate) fn torrent_bytes(name: &str, files: &[(&str, u64)], salt: &str) -> Vec<u8> {
        fn bstr(out: &mut Vec<u8>, s: &[u8]) {
            out.extend_from_slice(format!("{}:", s.len()).as_bytes());
            out.extend_from_slice(s);
        }
        const PIECE: u64 = 16_384;
        let total: u64 = files.iter().map(|(_, len)| len).sum();
        let pieces = usize::try_from(total.div_ceil(PIECE)).unwrap();
        let mut info = b"d".to_vec();
        bstr(&mut info, b"files");
        info.push(b'l');
        for (path, len) in files {
            info.extend_from_slice(format!("d6:lengthi{len}e4:pathl").as_bytes());
            for part in path.split('/') {
                bstr(&mut info, part.as_bytes());
            }
            info.extend_from_slice(b"ee");
        }
        info.push(b'e');
        bstr(&mut info, b"name");
        bstr(&mut info, name.as_bytes());
        info.extend_from_slice(format!("12:piece lengthi{PIECE}e").as_bytes());
        bstr(&mut info, b"pieces");
        bstr(&mut info, &vec![0u8; pieces * 20]);
        bstr(&mut info, b"source");
        bstr(&mut info, salt.as_bytes());
        info.push(b'e');
        let mut out = b"d4:info".to_vec();
        out.extend_from_slice(&info);
        out.push(b'e');
        out
    }

    pub(crate) fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("iris-removal-{}", uuid::Uuid::new_v4()))
    }

    pub(crate) fn write(dir: &Path, rel: &str, len: u64) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, vec![7u8; usize::try_from(len).unwrap()]).unwrap();
    }

    pub(crate) async fn grab(
        engine: &Arc<Engine>,
        pool: &sqlx::SqlitePool,
        bytes: Vec<u8>,
    ) -> iris_db::torrents::TorrentRow {
        let user = iris_db::test_support::make_user(pool).await;
        let added = engine.add_from_bytes(bytes).await.unwrap();
        iris_db::torrents::upsert(
            pool,
            iris_db::torrents::NewTorrent {
                infohash: added.snapshot.infohash.clone(),
                name: added.snapshot.name.clone().unwrap(),
                total_size_bytes: added.snapshot.total_size_bytes,
                source_provider: None,
                source_external_id: None,
                tracker_tmdb_id: None,
                added_by: user,
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn removing_one_of_two_copies_keeps_the_files_the_other_reads() {
        let dir = temp_dir();
        let engine = Engine::offline(dir.clone()).await.unwrap();
        let pool = iris_db::test_support::migrated_pool().await;
        let name = "Show.S01.1080p.WEB-GRP";
        write(&dir, &format!("{name}/Show.S01E01.mkv"), 40_000);
        write(&dir, &format!("{name}/Show.S01E02.mkv"), 30_000);
        write(&dir, &format!("{name}/extra.nfo"), 100);
        let both = [("Show.S01E01.mkv", 40_000), ("Show.S01E02.mkv", 30_000)];
        let a = grab(&engine, &pool, torrent_bytes(name, &both, "tracker-a")).await;
        let with_nfo = [both[0], both[1], ("extra.nfo", 100)];
        let b = grab(&engine, &pool, torrent_bytes(name, &with_nfo, "tracker-b")).await;
        assert_ne!(a.infohash, b.infohash);

        let first = remove_torrent(&engine, &pool, &a).await.unwrap();
        assert!(first.in_engine && first.row_was_live);
        assert_eq!(first.freed_bytes, 0, "every file of A is one B reads");
        assert!(dir.join(name).join("Show.S01E01.mkv").exists());
        assert!(!engine.contains(&a.infohash));

        let second = remove_torrent(&engine, &pool, &b).await.unwrap();
        assert_eq!(second.freed_bytes, 70_100);
        assert!(!dir.join(name).exists(), "the emptied folder goes too");
        assert!(dir.exists(), "never the download dir");

        let again = remove_torrent(&engine, &pool, &b).await.unwrap();
        assert!(!again.in_engine && !again.row_was_live);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_same_named_row_the_engine_lost_keeps_every_file() {
        let dir = temp_dir();
        let engine = Engine::offline(dir.clone()).await.unwrap();
        let pool = iris_db::test_support::migrated_pool().await;
        let name = "Movie.2021.1080p.WEB-GRP";
        // One file: librqbit writes it straight into the download dir.
        write(&dir, "Movie.2021.mkv", 20_000);
        let files = [("Movie.2021.mkv", 20_000)];
        let a = grab(&engine, &pool, torrent_bytes(name, &files, "a")).await;
        let user = iris_db::test_support::make_user(&pool).await;
        iris_db::torrents::upsert(
            &pool,
            iris_db::torrents::NewTorrent {
                infohash: "b".repeat(40),
                name: name.to_owned(),
                total_size_bytes: 20_000,
                source_provider: None,
                source_external_id: None,
                tracker_tmdb_id: None,
                added_by: user,
            },
        )
        .await
        .unwrap();
        let removed = remove_torrent(&engine, &pool, &a).await.unwrap();
        assert_eq!(removed.freed_bytes, 0);
        assert!(dir.join("Movie.2021.mkv").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
