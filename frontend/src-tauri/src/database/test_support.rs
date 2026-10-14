//! Shared fixtures and in-memory SQLite helpers for database tests.
use crate::api::TranscriptSegment;
use crate::database::repositories::speaker::{MeetingSpeaker, NewSpeaker, SpeakerWrite, SpeakersRepository};
use crate::database::repositories::transcript::TranscriptsRepository;
use chrono::Utc;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::str::FromStr;

pub struct SeedRow {
    pub id: &'static str,
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub speaker: Option<&'static str>,
    pub text: &'static str,
}

/// In-memory database with every real migration applied.
/// A single connection keeps the in-memory database alive for the pool's lifetime.
pub async fn migrated_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("open in-memory sqlite");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    pool
}

pub async fn seed_meeting(pool: &SqlitePool, meeting_id: &str, rows: &[SeedRow]) {
    let now = chrono::Utc::now();
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_path) VALUES (?, ?, ?, ?, NULL)")
        .bind(meeting_id)
        .bind("Test meeting")
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .expect("insert meeting");
    let mut conn = pool.acquire().await.expect("acquire connection");
    for row in rows {
        let segment = TranscriptSegment {
            id: row.id.to_string(),
            text: row.text.to_string(),
            timestamp: "2026-09-27T10:00:00Z".to_string(),
            audio_start_time: row.start,
            audio_end_time: row.end,
            duration: match (row.start, row.end) {
                (Some(s), Some(e)) => Some(e - s),
                _ => None,
            },
            speaker: row.speaker.map(str::to_string),
        };
        TranscriptsRepository::insert_row(&mut conn, row.id, meeting_id, &segment)
            .await
            .expect("insert transcript");
    }
}

/// Replaces the meeting's speakers.
pub async fn write_speakers(pool: &SqlitePool, meeting_id: &str, speakers: Vec<NewSpeaker>) {
    let mut conn = pool.acquire().await.expect("acquire connection");
    SpeakersRepository::replace_for_meeting(&mut conn, meeting_id, &SpeakerWrite { speakers, ..Default::default() })
        .await
        .expect("write speakers");
}

/// Creates the meeting without rows and writes its speakers.
pub async fn seed_speakers(pool: &SqlitePool, meeting_id: &str, speakers: Vec<NewSpeaker>) {
    seed_meeting(pool, meeting_id, &[]).await;
    write_speakers(pool, meeting_id, speakers).await;
}

/// The stored speaker with this key.
pub async fn stored_speaker(pool: &SqlitePool, meeting_id: &str, key: &str) -> MeetingSpeaker {
    SpeakersRepository::list(pool, meeting_id)
        .await
        .expect("list speakers")
        .into_iter()
        .find(|s| s.speaker_key == key)
        .expect("speaker exists")
}

/// The meeting's stored speakers by key.
pub async fn speakers_by_key(pool: &SqlitePool, meeting_id: &str) -> HashMap<String, MeetingSpeaker> {
    SpeakersRepository::list(pool, meeting_id)
        .await
        .expect("list speakers")
        .into_iter()
        .map(|s| (s.speaker_key.clone(), s))
        .collect()
}

pub async fn people_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM people").fetch_one(pool).await.expect("count people")
}

pub async fn seed_person(pool: &SqlitePool, id: &str, name: &str) {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO people (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(id)
        .bind(name)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .expect("insert person");
}

/// A fresh in-memory database with every migration applied. One connection
/// with no idle timeout, because each in-memory connection is its own database.
pub async fn test_pool() -> SqlitePool {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .unwrap()
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

pub async fn insert_meeting(pool: &SqlitePool, id: &str, title: &str, folder_path: Option<&str>) {
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO meetings (id, title, created_at, updated_at, folder_path) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(title)
    .bind(now)
    .bind(now)
    .bind(folder_path)
    .execute(pool)
    .await
    .unwrap();
}

pub async fn insert_transcript(pool: &SqlitePool, meeting_id: &str, text: &str) {
    sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)")
        .bind(format!("transcript-{}", uuid::Uuid::new_v4()))
        .bind(meeting_id)
        .bind(text)
        .bind("00:00:01")
        .execute(pool)
        .await
        .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migration_creates_organization_schema() {
        let pool = test_pool().await;
        insert_meeting(&pool, "meeting-1", "Standup", None).await;

        let (folder_id, deleted_at): (Option<String>, Option<String>) =
            sqlx::query_as("SELECT folder_id, deleted_at FROM meetings WHERE id = 'meeting-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(folder_id, None);
        assert_eq!(deleted_at, None);

        for table in ["folders", "tags", "meeting_tags"] {
            let count: (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM {}", table))
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(count.0, 0, "{} should start empty", table);
        }
    }

    #[tokio::test]
    async fn live_folder_names_are_unique_case_insensitively() {
        let pool = test_pool().await;
        let insert = |id: &'static str, name: &'static str| {
            let pool = pool.clone();
            async move {
                sqlx::query(
                    "INSERT INTO folders (id, name, created_at, updated_at) VALUES (?, ?, 't', 't')",
                )
                .bind(id)
                .bind(name)
                .execute(&pool)
                .await
            }
        };

        insert("folder-a", "Work").await.unwrap();
        assert!(insert("folder-b", "work").await.is_err());

        sqlx::query("UPDATE folders SET deleted_at = 't' WHERE id = 'folder-a'")
            .execute(&pool)
            .await
            .unwrap();
        insert("folder-b", "work").await.unwrap();
    }
}
