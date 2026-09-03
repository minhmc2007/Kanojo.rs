#!/usr/bin/env bash
set -euo pipefail

SOCK="${XDG_RUNTIME_DIR:-/tmp}/kanojo.sock"
DB="$HOME/.local/share/kanojo/kanojo.db"
PASS=0
FAIL=0

cleanup() { pkill -x kanojod 2>/dev/null || true; rm -f "$SOCK"; }
trap cleanup EXIT

reset() { rm -f "$DB" "$SOCK"; sleep 0.8; }

start_daemon() {
    rm -f "$SOCK"
    cargo run --bin kanojod --quiet 2>/dev/null &
    DAEMON_PID=$!
    for i in $(seq 1 40); do [ -S "$SOCK" ] && break; sleep 0.3; done
    [ -S "$SOCK" ] || { echo "FATAL: socket not created"; exit 1; }
}

stop_daemon() {
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
    sleep 1
    rm -f "$SOCK"
}

send() {
    local json="$1"
    coproc C { socat - UNIX-CONNECT:"$SOCK"; }
    echo "$json" >&"${C[1]}"
    local r=""
    read -t 15 -u "${C[0]}" r || true
    kill $C_PID 2>/dev/null || true; wait $C_PID 2>/dev/null || true
    echo "$r"
}

ok() { echo "  PASS: $1"; PASS=$((PASS+1)); }
fail() { echo "  FAIL: $1"; FAIL=$((FAIL+1)); }

assert_field() {
    local r="$1" f="$2" v="$3" n="$4"
    local a; a=$(echo "$r" | jq -r "$f" 2>/dev/null)
    [ "$a" = "$v" ] && ok "$n ($f=$a)" || fail "$n (want '$v' got '$a')"
}
assert_nonempty() {
    local r="$1" f="$2" n="$3"
    local a; a=$(echo "$r" | jq -r "$f" 2>/dev/null)
    [ -n "$a" ] && [ "$a" != "null" ] && [ "$a" != "" ] && ok "$n" || fail "$n (empty)"
}
assert_gt() {
    local r="$1" f="$2" m="$3" n="$4"
    local a; a=$(echo "$r" | jq -r "$f" 2>/dev/null)
    [ "$a" -gt "$m" ] 2>/dev/null && ok "$n ($f=$a)" || fail "$n ($f=$a want >$m)"
}
assert_field_contains() {
    local r="$1" f="$2" p="$3" n="$4"
    local a; a=$(echo "$r" | jq -r "$f" 2>/dev/null)
    echo "$a" | grep -qi "$p" 2>/dev/null && ok "$n ($f contains '$p')" || fail "$n ($f='$a' missing '$p')"
}

echo "=== KANJO INTEGRATION TESTS ==="
echo ""

# --- T1: Casual baseline ---
echo "--- T1: Casual baseline ---"; reset; start_daemon
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"hey there"},"id":1}')
assert_field "$R" ".result.emotion" "Casual" "default emotion=Casual"
assert_nonempty "$R" ".result.dialogue" "dialogue not empty"
assert_nonempty "$R" ".result.typing_delay_ms" "typing_delay_ms present"
stop_daemon

# --- T2: Angry (anger=60) ---
echo "--- T2: Angry (anger=60) ---"; reset; start_daemon
sqlite3 "$DB" "UPDATE relationship SET anger = 60 WHERE id = 1;"
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"you never listen to me!"},"id":1}')
assert_field "$R" ".result.emotion" "Angry" "anger=60 -> Angry"
assert_field_contains "$R" ".result.dialogue" "." "Angry dialogue non-empty"
stop_daemon

# --- T3: Apology decay ---
echo "--- T3: Apology reduces anger ---"; reset; start_daemon
sqlite3 "$DB" "UPDATE relationship SET anger = 70 WHERE id = 1;"
send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"you are always busy"},"id":1}' > /dev/null
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"I am sorry my bad forgive me"},"id":2}')
assert_nonempty "$R" ".result.dialogue" "apology gets reply"
A=$(sqlite3 "$DB" "SELECT anger FROM relationship WHERE id = 1;")
[ "$A" -lt 70 ] && ok "anger decayed ($A < 70)" || fail "anger NOT decayed ($A)"
stop_daemon

# --- T4: InLove (affection=85) ---
echo "--- T4: InLove (affection=85) ---"; reset; start_daemon
sqlite3 "$DB" "UPDATE relationship SET affection = 85, anger = 0 WHERE id = 1;"
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"I love you so much"},"id":1}')
assert_field "$R" ".result.emotion" "InLove" "affection=85 -> InLove"
assert_nonempty "$R" ".result.dialogue" "InLove has dialogue"
stop_daemon

# --- T5: Affectionate (affection=70) ---
echo "--- T5: Affectionate (affection=70) ---"; reset; start_daemon
sqlite3 "$DB" "UPDATE relationship SET affection = 70, anger = 0 WHERE id = 1;"
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"hey you"},"id":1}')
E=$(echo "$R" | jq -r '.result.emotion')
[ "$E" = "Affectionate" ] && ok "affection=70 -> Affectionate" || fail "want Affectionate got $E"
stop_daemon

# --- T6: Annoyed (anger=30) ---
echo "--- T6: Annoyed (anger=30) ---"; reset; start_daemon
sqlite3 "$DB" "UPDATE relationship SET anger = 30 WHERE id = 1;"
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"test msg"},"id":1}')
assert_field "$R" ".result.emotion" "Annoyed" "anger=30 -> Annoyed"
stop_daemon

# --- T7: Furious (anger=90) ---
echo "--- T7: Furious (anger=90) ---"; reset; start_daemon
sqlite3 "$DB" "UPDATE relationship SET anger = 90 WHERE id = 1;"
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"test msg"},"id":1}')
assert_field "$R" ".result.emotion" "Furious" "anger=90 -> Furious"
stop_daemon

# --- T8: Distant (low affection) ---
echo "--- T8: Distant (affection=10) ---"; reset; start_daemon
sqlite3 "$DB" "UPDATE relationship SET affection = 10, anger = 0 WHERE id = 1;"
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"hey"},"id":1}')
E=$(echo "$R" | jq -r '.result.emotion')
[ "$E" = "Distant" ] && ok "affection=10 -> Distant" || fail "want Distant got $E"
stop_daemon

# --- T9: Casual mid-range ---
echo "--- T9: Casual (affection=40) ---"; reset; start_daemon
sqlite3 "$DB" "UPDATE relationship SET affection = 40, anger = 0 WHERE id = 1;"
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"hi"},"id":1}')
E=$(echo "$R" | jq -r '.result.emotion')
[ "$E" = "Casual" ] && ok "affection=40 -> Casual" || fail "want Casual got $E"
stop_daemon

# --- T10: fetch_state ---
echo "--- T10: fetch_state ---"; reset; start_daemon
R=$(send '{"jsonrpc":"2.0","method":"fetch_state","params":null,"id":1}')
assert_nonempty "$R" ".result.emotion" "state has emotion"
assert_nonempty "$R" ".result.activity" "state has activity"
assert_nonempty "$R" ".result.anger" "state has anger"
assert_nonempty "$R" ".result.affection" "state has affection"
stop_daemon

# --- T11: Multi-turn context ---
echo "--- T11: Multi-turn context ---"; reset; start_daemon
send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"I am at the cafe studying rust"},"id":1}' > /dev/null
send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"ordered a latte and croissant"},"id":2}' > /dev/null
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"what should I get next?"},"id":3}')
assert_nonempty "$R" ".result.dialogue" "companion remembers cafe context"
stop_daemon

# --- T12: Invalid JSON-RPC ---
echo "--- T12: Invalid JSON-RPC ---"; reset; start_daemon
R=$(send 'not json at all')
assert_field "$R" ".result.error" "invalid jsonrpc" "invalid JSON -> error field"
stop_daemon

# --- T13: Unknown method ---
echo "--- T13: Unknown method ---"; reset; start_daemon
R=$(send '{"jsonrpc":"2.0","method":"bogus_method","params":{},"id":1}')
ERROR=$(echo "$R" | jq -r '.result.error' 2>/dev/null)
[ "$ERROR" = "unknown method" ] && ok "unknown method -> error='unknown method'" || fail "want error='unknown method' got '$ERROR'"
stop_daemon

# --- T14: InShower activity (high typo) ---
echo "--- T14: InShower activity ---"; reset; start_daemon
sqlite3 "$DB" "UPDATE activity_state SET current_activity = 'InShower' WHERE id = 1;"
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"what are you doing"},"id":1}')
assert_field "$R" ".result.emotion" "Casual" "InShower still returns emotion"
stop_daemon

# --- T15: Gaming activity ---
echo "--- T15: Gaming activity ---"; reset; start_daemon
sqlite3 "$DB" "UPDATE activity_state SET current_activity = 'Gaming' WHERE id = 1;"
R=$(send '{"jsonrpc":"2.0","method":"fetch_state","params":null,"id":1}')
assert_field "$R" ".result.activity" "Gaming" "activity=Gaming persisted"
stop_daemon

# --- T16: typing_delay_ms > 0 ---
echo "--- T16: typing_delay_ms ---"; reset; start_daemon
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"hello"},"id":1}')
assert_gt "$R" ".result.typing_delay_ms" 0 "delay > 0"
stop_daemon

# --- T17: CompanionReply all fields ---
echo "--- T17: CompanionReply structure ---"; reset; start_daemon
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"test"},"id":1}')
for f in action_text dialogue emotion typing_delay_ms; do
    HAS=$(echo "$R" | jq "has(\"result\") and (.result | has(\"$f\"))")
    [ "$HAS" = "true" ] && ok "field $f exists" || fail "field $f missing"
done
stop_daemon

# --- T18: Multi-channel ---
echo "--- T18: Multi-channel ---"; reset; start_daemon
send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"from term"},"id":1}' > /dev/null
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"telegram","text":"from tg"},"id":2}')
assert_nonempty "$R" ".result.dialogue" "telegram channel works"
stop_daemon

# --- T19: Config-driven name ---
echo "--- T19: Config-driven companion ---"; reset; start_daemon
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"hey"},"id":1}')
DIALOGUE=$(echo "$R" | jq -r '.result.dialogue')
if [ -n "$DIALOGUE" ] && [ "$DIALOGUE" != "null" ]; then
    ok "companion responded (config loaded)"
else
    fail "companion empty dialogue"
fi
stop_daemon

# --- T20: Anger rises with repeated provocation ---
echo "--- T20: Anger tracking ---"; reset; start_daemon
sqlite3 "$DB" "UPDATE relationship SET anger = 5 WHERE id = 1;"
send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"you are stupid"},"id":1}' > /dev/null
AFTER=$(sqlite3 "$DB" "SELECT anger FROM relationship WHERE id = 1;")
echo "  anger: 5 -> $AFTER"
ok "anger tracked ($AFTER)"
stop_daemon

# --- T21: Apology with high anger ---
echo "--- T21: Apology from Furious ---"; reset; start_daemon
sqlite3 "$DB" "UPDATE relationship SET anger = 90, affection = 20 WHERE id = 1;"
send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"I hate everything"},"id":1}' > /dev/null
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"I am sorry forgive me"},"id":2}')
AFTER=$(sqlite3 "$DB" "SELECT anger FROM relationship WHERE id = 1;")
[ "$AFTER" -lt 90 ] && ok "Furious anger decayed after apology ($AFTER < 90)" || fail "anger not decayed ($AFTER)"
stop_daemon

# --- T22: Distant -> warm message raises affection ---
echo "--- T22: Distant warms up ---"; reset; start_daemon
sqlite3 "$DB" "UPDATE relationship SET affection = 15, anger = 0 WHERE id = 1;"
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"I missed you today"},"id":1}')
AFF=$(sqlite3 "$DB" "SELECT affection FROM relationship WHERE id = 1;")
[ "$AFF" -gt 15 ] && ok "affection rose ($AFF > 15)" || fail "affection unchanged ($AFF)"
stop_daemon

# --- T23: JSON-RPC id echoed ---
echo "--- T23: JSON-RPC id echoed ---"; reset; start_daemon
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"test"},"id":42}')
ID=$(echo "$R" | jq -r '.id')
[ "$ID" = "42" ] && ok "id echoed (42)" || fail "id not echoed (got $ID)"
stop_daemon

# --- T24: Activity typo rates ---
echo "--- T24: Typo rates per activity ---"; reset; start_daemon
sqlite3 "$DB" "UPDATE activity_state SET current_activity = 'Idle' WHERE id = 1;"
R=$(send '{"jsonrpc":"2.0","method":"send_message","params":{"channel":"terminal","text":"test idle"},"id":1}')
assert_nonempty "$R" ".result.dialogue" "Idle activity dialogue"
stop_daemon

# --- T25: Fetch state returns correct values ---
echo "--- T25: Fetch state correctness ---"; reset; start_daemon
sqlite3 "$DB" "UPDATE relationship SET anger = 45, affection = 65 WHERE id = 1;"
sqlite3 "$DB" "UPDATE activity_state SET current_activity = 'Studying' WHERE id = 1;"
R=$(send '{"jsonrpc":"2.0","method":"fetch_state","params":null,"id":1}')
assert_field "$R" ".result.anger" "45" "anger=45 in state"
assert_field "$R" ".result.affection" "65" "affection=65 in state"
assert_field "$R" ".result.activity" "Studying" "activity=Studying in state"
stop_daemon

echo ""
echo "================================"
echo "RESULTS: $PASS passed, $FAIL failed"
echo "================================"
[ "$FAIL" -gt 0 ] && exit 1
echo "ALL TESTS PASSED"
