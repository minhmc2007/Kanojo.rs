# Kanojo.rs: Complete Architectural Master Specification

**`Kanojo.rs`** is an autonomous, system-aware companion daemon written in Rust. The architecture decouples background intelligence (`kanojod`) from user interfaces (Ratatui TUI, Telegram Gateway) using a client-server model over Unix Domain Sockets (UDS).

---

## 1. System Architecture & IPC Layer

```text
  [ Ratatui TUI ]  <----\
                         +---> [ Unix Domain Socket ] ---> [ kanojod Daemon Core ]
  [ Telegram Bot ] <----/       (/run/user/1000/kanojo.sock)      |
                                                                  +---> [ SQLite + sqlite-vec ]
                                                                  +---> [ LLM Router (Ollama/API) ]

```

### IPC Protocol Specification

* **Transport:** Unix Domain Socket (`tokio::net::UnixListener` / `interprocess`).
* **Protocol:** Frame-delimited JSON-RPC 2.0.
* **Brain Priority:** `kanojod` acts as the single source of truth for database access, state machine changes, and LLM inference. Frontends are strictly presentation layers.

#### Sample JSON-RPC Payload Structure

```json
// Client Request (TUI -> kanojod)
{
  "jsonrpc": "2.0",
  "method": "send_message",
  "params": {
    "channel": "terminal",
    "text": "Hey Aoi, what are you up to?"
  },
  "id": 1
}

// Daemon Streamed Response Frame (kanojod -> TUI)
{
  "jsonrpc": "2.0",
  "result": {
    "action_text": "She shifts in her chair, glancing at her secondary monitor.",
    "dialogue": "just fixing some compiler warnings in rust... u need something?",
    "inner_thought": "Hope he doesn't ask me to fix his borrowing errors again.",
    "emotion": "Casual",
    "typing_delay_ms": 950
  },
  "id": 1
}

```

---

## 2. Production Database Schema (SQLite + Vector Search)

```sql
PRAGMA foreign_keys = ON;

-- Global Configuration Key-Value Store
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Singleton Relationship Engine State
CREATE TABLE IF NOT EXISTS relationship (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    affection INTEGER NOT NULL DEFAULT 50 CHECK (affection BETWEEN 0 AND 100),
    intimacy INTEGER NOT NULL DEFAULT 0 CHECK (intimacy BETWEEN 0 AND 100),
    anger INTEGER NOT NULL DEFAULT 0 CHECK (anger BETWEEN 0 AND 100),
    current_emotion TEXT NOT NULL DEFAULT 'Casual'
);

-- Singleton Daemon Hardware & Monotonic Sleep State
CREATE TABLE IF NOT EXISTS daemon_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_bot_message_timestamp DATETIME,
    last_user_response_timestamp DATETIME,
    is_suspended BOOLEAN DEFAULT 0,
    is_afk_mode BOOLEAN DEFAULT 0,
    afk_reason TEXT
);

-- Physical Activity State
CREATE TABLE IF NOT EXISTS activity_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    current_activity TEXT DEFAULT 'Idle',
    estimated_delay_seconds INTEGER DEFAULT 0,
    typo_probability REAL DEFAULT 0.05
);

-- Complete Conversation History for Sliding Context Windows
CREATE TABLE IF NOT EXISTS message_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sender TEXT NOT NULL CHECK (sender IN ('user', 'companion', 'system')),
    channel TEXT NOT NULL DEFAULT 'terminal',
    action_text TEXT,
    dialogue TEXT NOT NULL,
    inner_thought TEXT,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Outbound Message Queue for Delayed Delivery
CREATE TABLE IF NOT EXISTS message_queue (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scheduled_delivery DATETIME NOT NULL,
    target_channel TEXT NOT NULL CHECK (target_channel IN ('terminal', 'telegram', 'desktop_notification')),
    content_json TEXT NOT NULL,
    status TEXT DEFAULT 'pending' CHECK (status IN ('pending', 'delivered', 'cancelled'))
);

-- Long-Term Memory Summaries
CREATE TABLE IF NOT EXISTS long_term_memory (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    summary TEXT NOT NULL,
    emotional_weight INTEGER NOT NULL DEFAULT 1
);

-- Semantic Embeddings Table (sqlite-vec)
CREATE VIRTUAL TABLE IF NOT EXISTS memory_embeddings USING vec0(
    memory_id INTEGER PRIMARY KEY,
    embedding float[384] -- Dimensions for all-MiniLM-L6-v2
);

```

---

## 3. Core Behavioral & State Engine Mechanics

* **Laptop Sleep & Suspend Handling:**
* **Linux Hook:** Listens directly to D-Bus signals (`org.freedesktop.login1.Manager` -> `PrepareForSleep`).
* **Behavior:** When `PrepareForSleep(true)` fires, `daemon_state.is_suspended` is set to `1`, pausing all ghosting penalty counters. Upon wake (`PrepareForSleep(false)`), `last_bot_message_timestamp` is offset by the total suspend duration.


* **Anger Decay & Reconciliation:**
* **Decay Function:** Anger automatically decays by $-5$ points/hour during periods of low activity or neutral interactions, shifting state: `Angry` $\rightarrow$ `Annoyed` $\rightarrow$ `Cold` $\rightarrow$ `Casual`.
* **Apology Path:** Prompts parse user intent; sincere apologies apply an immediate `anger -= 25` reduction.


* **Safety & Provider Constraint Routing:**
* **Cloud Routing (OpenAI/Anthropic):** Intimate states (`Horny`) trigger prompt-layer sanitization to ensure compliance with provider policies.
* **Local Routing (Ollama/vLLM):** If uncensored local weights are present, intimate context frames route directly to local inference to bypass cloud API restrictions.


* **Async Context Compaction:**
* Background worker runs when context usage hits 80%.
* Older messages ($1 \dots N-10$) are summarized into `long_term_memory`, converted into vector embeddings via `fastembed-rs`, and stored in `memory_embeddings`. The active window remains intact without triggering artificial state locks like `InShower`.



---

## 4. Cross-Platform Abstraction

To support multi-platform execution without code fragmentation, hardware and background runners implement standard trait interfaces:

```rust
#[async_trait]
pub trait ServiceManager {
    async fn install_daemon(&self) -> Result<()>;
    async fn start_daemon(&self) -> Result<()>;
    async fn stop_daemon(&self) -> Result<()>;
}

pub trait SystemObserver {
    fn get_active_media(&self) -> Option<MediaInfo>;
    fn register_sleep_hook(&self, tx: tokio::sync::mpsc::Sender<SleepEvent>);
}

```

* **Linux:** Systemd user unit + D-Bus MPRIS listener + `org.freedesktop.login1`.
* **macOS:** Launchd agent plist + CoreAudio / AppleScript observers.
* **Windows:** `windows-service` crate + Windows Media Manager API.

---

## 5. Development Phase Roadmap

| Phase | Core Focus | Technical Deliverables |
| --- | --- | --- |
| **Phase 1** | **Daemon & IPC Core** | `kanojod` binary, `tokio::net::UnixListener`, JSON-RPC schema, SQLite schema implementation. |
| **Phase 2** | **Sleep Hooks & Realism Engine** | D-Bus `PrepareForSleep` listener, delayed queue runner, typo engine transformer, anger decay loop. |
| **Phase 3** | **TUI & Typo Simulation** | Ratatui frontend over UDS, micro-batch character typing delays, custom action text highlights. |
| **Phase 4** | **Semantic RAG & Observers** | `sqlite-vec` integration, `sysinfo` process tracking, D-Bus MPRIS media listener, search integration. |
| **Phase 5** | **Compaction, Fallbacks & Gateways** | Background context summarizer, cloud-to-local LLM fallback router, Telegram gateway (`teloxide`), systemd packaging. |
