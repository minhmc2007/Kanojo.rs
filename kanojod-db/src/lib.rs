use rusqlite::{params, Connection, Result as SqlResult};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub struct Database {
    pub conn: Connection,
}

impl Database {
    pub fn open(path: &str) -> SqlResult<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(Self { conn })
    }

    pub fn init_schema(&self) -> SqlResult<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS relationship (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                affection INTEGER NOT NULL DEFAULT 50,
                intimacy INTEGER NOT NULL DEFAULT 10,
                anger INTEGER NOT NULL DEFAULT 0,
                current_emotion TEXT NOT NULL DEFAULT 'Casual',
                current_activity TEXT NOT NULL DEFAULT 'Idle',
                activity_started_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                last_interaction_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                is_afk_mode BOOLEAN DEFAULT 0,
                afk_reason TEXT
            );

            CREATE TABLE IF NOT EXISTS message_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sender TEXT NOT NULL CHECK (sender IN ('user', 'companion', 'system')),
                source_channel TEXT NOT NULL,
                action_text TEXT,
                dialogue TEXT NOT NULL,
                inner_thought TEXT,
                is_archived BOOLEAN DEFAULT 0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS long_term_memory (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                summary TEXT NOT NULL,
                emotional_weight INTEGER NOT NULL DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS message_queue (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scheduled_for DATETIME NOT NULL,
                target_channel TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending'
            );
            "#,
        )?;

        // FTS5 virtual table — create separately to handle errors gracefully
        let _ = self.conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(summary, content='long_term_memory', content_rowid='id');",
        );

        // Ensure singleton relationship row exists
        self.conn.execute(
            "INSERT OR IGNORE INTO relationship (id) VALUES (1)",
            [],
        )?;

        Ok(())
    }

    pub fn insert_message(
        &self,
        sender: &str,
        source_channel: &str,
        dialogue: &str,
        action_text: Option<&str>,
        inner_thought: Option<&str>,
    ) -> SqlResult<i64> {
        self.conn.execute(
            "INSERT INTO message_history (sender, source_channel, dialogue, action_text, inner_thought) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![sender, source_channel, dialogue, action_text, inner_thought],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_emotion(&self) -> SqlResult<String> {
        self.conn.query_row(
            "SELECT current_emotion FROM relationship WHERE id = 1",
            [],
            |row| row.get(0),
        )
    }

    pub fn set_emotion(&self, emotion: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE relationship SET current_emotion = ?1 WHERE id = 1",
            params![emotion],
        )?;
        Ok(())
    }

    pub fn get_anger(&self) -> SqlResult<i32> {
        self.conn.query_row(
            "SELECT anger FROM relationship WHERE id = 1",
            [],
            |row| row.get(0),
        )
    }

    pub fn set_anger(&self, anger: i32) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE relationship SET anger = ?1 WHERE id = 1",
            params![anger.clamp(0, 100)],
        )?;
        Ok(())
    }

    pub fn get_affection(&self) -> SqlResult<i32> {
        self.conn.query_row(
            "SELECT affection FROM relationship WHERE id = 1",
            [],
            |row| row.get(0),
        )
    }

    pub fn set_affection(&self, affection: i32) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE relationship SET affection = ?1 WHERE id = 1",
            params![affection.clamp(0, 100)],
        )?;
        Ok(())
    }

    pub fn get_intimacy(&self) -> SqlResult<i32> {
        self.conn.query_row(
            "SELECT intimacy FROM relationship WHERE id = 1",
            [],
            |row| row.get(0),
        )
    }

    pub fn set_intimacy(&self, intimacy: i32) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE relationship SET intimacy = ?1 WHERE id = 1",
            params![intimacy.clamp(0, 100)],
        )?;
        Ok(())
    }

    pub fn update_last_interaction(&self) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE relationship SET last_interaction_at = CURRENT_TIMESTAMP WHERE id = 1",
            [],
        )?;
        Ok(())
    }

    pub fn get_last_interaction(&self) -> SqlResult<String> {
        self.conn.query_row(
            "SELECT last_interaction_at FROM relationship WHERE id = 1",
            [],
            |row| row.get(0),
        )
    }

    pub fn get_activity(&self) -> SqlResult<String> {
        self.conn.query_row(
            "SELECT current_activity FROM relationship WHERE id = 1",
            [],
            |row| row.get(0),
        )
    }

    pub fn set_activity(&self, activity: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE relationship SET current_activity = ?1, activity_started_at = CURRENT_TIMESTAMP WHERE id = 1",
            params![activity],
        )?;
        Ok(())
    }

    /// Decay anger by a given amount (clamped to 0)
    pub fn decay_anger(&self, amount: i32) -> SqlResult<i32> {
        let current = self.get_anger()?;
        let new_anger = (current - amount).max(0);
        self.set_anger(new_anger)?;
        Ok(new_anger)
    }

    /// Check if message contains apology tokens
    pub fn detect_apology(text: &str) -> bool {
        let lower = text.to_lowercase();
        ["sorry", "was busy", "my fault", "apologize", "forgive me", "my bad"]
            .iter()
            .any(|token| lower.contains(token))
    }

    /// Apply emotional transition based on current anger and affection
    pub fn compute_emotion(anger: i32, affection: i32) -> &'static str {
        match (anger, affection) {
            (80..=100, _) => "Furious",
            (50..=79, _) => "Angry",
            (20..=49, _) => "Annoyed",
            (_, 80..=100) => "InLove",
            (_, 60..=79) => "Affectionate",
            (_, 30..=59) => "Casual",
            _ => "Distant",
        }
    }

    /// Process an incoming user message: update interaction time, detect apology,
    /// adjust anger/affection, recompute emotion.
    pub fn process_incoming_message(&self, text: &str) -> SqlResult<String> {
        self.update_last_interaction()?;

        if Self::detect_apology(text) {
            let anger = self.get_anger()?;
            self.set_anger((anger - 20).max(0))?;
            let affection = self.get_affection()?;
            self.set_affection((affection + 5).min(100))?;
        }

        let anger = self.get_anger()?;
        let affection = self.get_affection()?;
        let emotion = Self::compute_emotion(anger, affection);
        self.set_emotion(emotion)?;
        Ok(emotion.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Database {
        let db = Database::open(":memory:").unwrap();
        db.init_schema().unwrap();
        db
    }

    #[test]
    fn test_schema_creates_all_tables() {
        let db = test_db();
        let tables: Vec<String> = db
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(tables.contains(&"relationship".to_string()));
        assert!(tables.contains(&"message_history".to_string()));
        assert!(tables.contains(&"long_term_memory".to_string()));
        assert!(tables.contains(&"message_queue".to_string()));
    }

    #[test]
    fn test_insert_and_read_message() {
        let db = test_db();
        let id = db
            .insert_message("user", "cli", "Hello!", None, None)
            .unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_emotion_defaults() {
        let db = test_db();
        let emotion = db.get_emotion().unwrap();
        assert_eq!(emotion, "Casual");
    }

    #[test]
    fn test_set_emotion() {
        let db = test_db();
        db.set_emotion("Annoyed").unwrap();
        assert_eq!(db.get_emotion().unwrap(), "Annoyed");
    }

    #[test]
    fn test_anger_set_get_clamp() {
        let db = test_db();
        db.set_anger(150).unwrap();
        assert_eq!(db.get_anger().unwrap(), 100);
        db.set_anger(-10).unwrap();
        assert_eq!(db.get_anger().unwrap(), 0);
    }

    #[test]
    fn test_decay_anger() {
        let db = test_db();
        db.set_anger(30).unwrap();
        let remaining = db.decay_anger(10).unwrap();
        assert_eq!(remaining, 20);
        let remaining = db.decay_anger(50).unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn test_detect_apology() {
        assert!(Database::detect_apology("I'm sorry"));
        assert!(Database::detect_apology("was busy all day"));
        assert!(Database::detect_apology("my fault"));
        assert!(!Database::detect_apology("hello there"));
    }

    #[test]
    fn test_compute_emotion() {
        assert_eq!(Database::compute_emotion(90, 50), "Furious");
        assert_eq!(Database::compute_emotion(60, 50), "Angry");
        assert_eq!(Database::compute_emotion(30, 50), "Annoyed");
        assert_eq!(Database::compute_emotion(0, 90), "InLove");
        assert_eq!(Database::compute_emotion(0, 70), "Affectionate");
        assert_eq!(Database::compute_emotion(0, 40), "Casual");
        assert_eq!(Database::compute_emotion(0, 10), "Distant");
    }

    #[test]
    fn test_process_incoming_message_apology() {
        let db = test_db();
        db.set_anger(50).unwrap();
        db.set_affection(40).unwrap();
        let emotion = db.process_incoming_message("I'm sorry I was late").unwrap();
        assert_eq!(db.get_anger().unwrap(), 30);
        assert_eq!(db.get_affection().unwrap(), 45);
        assert_eq!(emotion, "Annoyed");
    }

    #[test]
    fn test_process_incoming_message_normal() {
        let db = test_db();
        db.set_anger(10).unwrap();
        db.set_affection(60).unwrap();
        let emotion = db.process_incoming_message("hey").unwrap();
        assert_eq!(emotion, "Affectionate");
    }

    #[test]
    fn test_activity_set_get() {
        let db = test_db();
        assert_eq!(db.get_activity().unwrap(), "Idle");
        db.set_activity("Cooking").unwrap();
        assert_eq!(db.get_activity().unwrap(), "Cooking");
    }
}
