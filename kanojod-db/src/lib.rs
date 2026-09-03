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
            CREATE TABLE IF NOT EXISTS config (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS relationship (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                affection INTEGER NOT NULL DEFAULT 50 CHECK (affection BETWEEN 0 AND 100),
                intimacy INTEGER NOT NULL DEFAULT 0 CHECK (intimacy BETWEEN 0 AND 100),
                anger INTEGER NOT NULL DEFAULT 0 CHECK (anger BETWEEN 0 AND 100),
                current_emotion TEXT NOT NULL DEFAULT 'Casual'
            );

            CREATE TABLE IF NOT EXISTS daemon_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                last_bot_message_timestamp DATETIME,
                last_user_response_timestamp DATETIME,
                is_suspended BOOLEAN DEFAULT 0,
                is_afk_mode BOOLEAN DEFAULT 0,
                afk_reason TEXT
            );

            CREATE TABLE IF NOT EXISTS activity_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                current_activity TEXT DEFAULT 'Idle',
                estimated_delay_seconds INTEGER DEFAULT 0,
                typo_probability REAL DEFAULT 0.05
            );

            CREATE TABLE IF NOT EXISTS message_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sender TEXT NOT NULL CHECK (sender IN ('user', 'companion', 'system')),
                channel TEXT NOT NULL DEFAULT 'terminal',
                action_text TEXT,
                dialogue TEXT NOT NULL,
                inner_thought TEXT,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS message_queue (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scheduled_delivery DATETIME NOT NULL,
                target_channel TEXT NOT NULL CHECK (target_channel IN ('terminal', 'telegram', 'desktop_notification')),
                content_json TEXT NOT NULL,
                status TEXT DEFAULT 'pending' CHECK (status IN ('pending', 'delivered', 'cancelled'))
            );

            CREATE TABLE IF NOT EXISTS long_term_memory (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                summary TEXT NOT NULL,
                emotional_weight INTEGER NOT NULL DEFAULT 1
            );
            "#,
        )?;

        // Singleton rows
        self.conn
            .execute("INSERT OR IGNORE INTO relationship (id) VALUES (1)", [])?;
        self.conn
            .execute("INSERT OR IGNORE INTO daemon_state (id) VALUES (1)", [])?;
        self.conn
            .execute("INSERT OR IGNORE INTO activity_state (id) VALUES (1)", [])?;

        Ok(())
    }

    // --- config ---
    pub fn get_config(&self, key: &str) -> SqlResult<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM config WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn set_config(&self, key: &str, value: &str) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    // --- relationship ---
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

    // --- daemon_state ---
    pub fn is_suspended(&self) -> SqlResult<bool> {
        self.conn.query_row(
            "SELECT is_suspended FROM daemon_state WHERE id = 1",
            [],
            |row| row.get(0),
        )
    }

    pub fn set_suspended(&self, suspended: bool) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE daemon_state SET is_suspended = ?1 WHERE id = 1",
            params![suspended],
        )?;
        Ok(())
    }

    pub fn update_last_user_response(&self) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE daemon_state SET last_user_response_timestamp = CURRENT_TIMESTAMP WHERE id = 1",
            [],
        )?;
        Ok(())
    }

    pub fn update_last_bot_message(&self) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE daemon_state SET last_bot_message_timestamp = CURRENT_TIMESTAMP WHERE id = 1",
            [],
        )?;
        Ok(())
    }

    // --- activity_state ---
    pub fn get_activity(&self) -> SqlResult<String> {
        self.conn.query_row(
            "SELECT current_activity FROM activity_state WHERE id = 1",
            [],
            |row| row.get(0),
        )
    }

    pub fn set_activity(&self, activity: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE activity_state SET current_activity = ?1 WHERE id = 1",
            params![activity],
        )?;
        Ok(())
    }

    pub fn get_typo_probability(&self) -> SqlResult<f64> {
        self.conn.query_row(
            "SELECT typo_probability FROM activity_state WHERE id = 1",
            [],
            |row| row.get(0),
        )
    }

    pub fn set_typo_probability(&self, prob: f64) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE activity_state SET typo_probability = ?1 WHERE id = 1",
            params![prob],
        )?;
        Ok(())
    }

    // --- messages ---
    pub fn insert_message(
        &self,
        sender: &str,
        channel: &str,
        dialogue: &str,
        action_text: Option<&str>,
        inner_thought: Option<&str>,
    ) -> SqlResult<i64> {
        self.conn.execute(
            "INSERT INTO message_history (sender, channel, dialogue, action_text, inner_thought) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![sender, channel, dialogue, action_text, inner_thought],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn recent_messages(&self, limit: usize) -> SqlResult<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT sender, channel, dialogue FROM message_history ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(rows)
    }

    // --- emotional engine ---
    pub fn detect_apology(text: &str) -> bool {
        let lower = text.to_lowercase();
        ["sorry", "was busy", "my fault", "apologize", "forgive me", "my bad"]
            .iter()
            .any(|token| lower.contains(token))
    }

    pub fn detect_warmth(text: &str) -> bool {
        let lower = text.to_lowercase();
        ["love", "miss", "care", "missed", "adore", "hug", "kiss", "sweet", "wonderful", "amazing", "beautiful", "happy", "glad"]
            .iter()
            .any(|token| lower.contains(token))
    }

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

    pub fn decay_anger(&self, amount: i32) -> SqlResult<i32> {
        let current = self.get_anger()?;
        let new_anger = (current - amount).max(0);
        self.set_anger(new_anger)?;
        Ok(new_anger)
    }

    pub fn process_incoming_message(&self, text: &str) -> SqlResult<String> {
        self.update_last_user_response()?;

        if Self::detect_apology(text) {
            let anger = self.get_anger()?;
            self.set_anger((anger - 25).max(0))?;
            let affection = self.get_affection()?;
            self.set_affection((affection + 5).min(100))?;
        }

        // Warm messages raise affection slightly when anger is low
        if Self::detect_warmth(text) {
            let anger = self.get_anger()?;
            if anger < 30 {
                let affection = self.get_affection()?;
                self.set_affection((affection + 3).min(100))?;
            }
        }

        let anger = self.get_anger()?;
        let affection = self.get_affection()?;
        let emotion = Self::compute_emotion(anger, affection);
        self.set_emotion(emotion)?;
        Ok(emotion.to_string())
    }
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Database {
        let db = Database::open(":memory:").unwrap();
        db.init_schema().unwrap();
        db
    }

    #[test]
    fn test_schema() {
        let db = test_db();
        let tables: Vec<String> = db
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(tables.contains(&"config".to_string()));
        assert!(tables.contains(&"relationship".to_string()));
        assert!(tables.contains(&"daemon_state".to_string()));
        assert!(tables.contains(&"activity_state".to_string()));
        assert!(tables.contains(&"message_history".to_string()));
        assert!(tables.contains(&"message_queue".to_string()));
        assert!(tables.contains(&"long_term_memory".to_string()));
    }

    #[test]
    fn test_config_roundtrip() {
        let db = test_db();
        assert_eq!(db.get_config("key").unwrap(), None);
        db.set_config("key", "value").unwrap();
        assert_eq!(db.get_config("key").unwrap(), Some("value".into()));
    }

    #[test]
    fn test_emotion_defaults() {
        let db = test_db();
        assert_eq!(db.get_emotion().unwrap(), "Casual");
        assert_eq!(db.get_anger().unwrap(), 0);
        assert_eq!(db.get_affection().unwrap(), 50);
        assert_eq!(db.get_intimacy().unwrap(), 0);
    }

    #[test]
    fn test_emotion_transitions() {
        assert_eq!(Database::compute_emotion(90, 50), "Furious");
        assert_eq!(Database::compute_emotion(60, 50), "Angry");
        assert_eq!(Database::compute_emotion(30, 50), "Annoyed");
        assert_eq!(Database::compute_emotion(0, 90), "InLove");
        assert_eq!(Database::compute_emotion(0, 70), "Affectionate");
        assert_eq!(Database::compute_emotion(0, 40), "Casual");
        assert_eq!(Database::compute_emotion(0, 10), "Distant");
    }

    #[test]
    fn test_anger_decay() {
        let db = test_db();
        db.set_anger(30).unwrap();
        assert_eq!(db.decay_anger(10).unwrap(), 20);
        assert_eq!(db.decay_anger(50).unwrap(), 0);
    }

    #[test]
    fn test_apology_detection() {
        assert!(Database::detect_apology("I'm sorry"));
        assert!(Database::detect_apology("was busy all day"));
        assert!(Database::detect_apology("my fault"));
        assert!(!Database::detect_apology("hello there"));
    }

    #[test]
    fn test_warmth_detection() {
        assert!(Database::detect_warmth("I love you"));
        assert!(Database::detect_warmth("I missed you today"));
        assert!(Database::detect_warmth("you are wonderful"));
        assert!(!Database::detect_warmth("you are stupid"));
    }

    #[test]
    fn test_warmth_increases_affection() {
        let db = test_db();
        db.set_anger(0).unwrap();
        db.set_affection(15).unwrap();
        db.process_incoming_message("I missed you today").unwrap();
        assert!(db.get_affection().unwrap() > 15);
    }

    #[test]
    fn test_process_message_with_apology() {
        let db = test_db();
        db.set_anger(50).unwrap();
        db.set_affection(40).unwrap();
        let emotion = db.process_incoming_message("I'm sorry I was late").unwrap();
        assert_eq!(db.get_anger().unwrap(), 25);
        assert_eq!(db.get_affection().unwrap(), 45);
        assert_eq!(emotion, "Annoyed");
    }

    #[test]
    fn test_insert_and_recent_messages() {
        let db = test_db();
        db.insert_message("user", "terminal", "hello", None, None)
            .unwrap();
        db.insert_message("companion", "terminal", "hey!", None, None)
            .unwrap();
        let msgs = db.recent_messages(10).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].0, "companion");
    }

    #[test]
    fn test_activity_state() {
        let db = test_db();
        assert_eq!(db.get_activity().unwrap(), "Idle");
        db.set_activity("Cooking").unwrap();
        assert_eq!(db.get_activity().unwrap(), "Cooking");
    }

    #[test]
    fn test_daemon_state() {
        let db = test_db();
        assert!(!db.is_suspended().unwrap());
        db.set_suspended(true).unwrap();
        assert!(db.is_suspended().unwrap());
    }
}
