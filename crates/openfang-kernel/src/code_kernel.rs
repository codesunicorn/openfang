//! Persistent per-session Python code kernel ("PTC").
//!
//! Instead of forcing the model through one LLM round-trip per tool call, an
//! agent can write a single Python script that calls tools and processes their
//! results in-process. Each iteration of the classic loop costs a full
//! re-send of the conversation (tens of thousands of tokens on a long turn);
//! collapsing N tool steps into one script cuts that to a single turn.
//!
//! Design:
//! - One `python3 -u` process per (agent, channel-scoped session key), kept
//!   alive across calls so variables/functions persist between invocations.
//! - The child speaks JSON-lines on stdin/stdout. A script calls
//!   `call_tool("file_read", path=...)`; the kernel relays that to the normal
//!   tool implementations (allowlisted subset — no shell, no recursion),
//!   then hands the result back to the blocked script.
//! - Nested calls go through the same workspace sandbox and deny floors as
//!   top-level calls, because they use the same implementations.
//! - Idle processes are reaped; each call has a hard timeout.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tracing::{debug, warn};

/// Tools a code-kernel script may call.
///
/// Deliberately excludes `shell_exec` (the agent can call it directly, and
/// arbitrary code + shell in one step defeats the approval gate) and
/// `code_execute` itself.
pub const NESTED_TOOLS: &[&str] = &[
    "file_read",
    "file_write",
    "file_list",
    "memory_recall",
    "memory_store",
    "session_search",
    "web_fetch",
    "web_search",
];

/// Bootstrap executed inside the child interpreter.
const BOOTSTRAP: &str = r#"
import sys, json, io, traceback, contextlib

_real_out = sys.stdout
_ns = {"__name__": "__main__", "__builtins__": __builtins__}

def call_tool(name, **kwargs):
    """Call an OpenFang tool from inside the script; blocks for the result."""
    _real_out.write(json.dumps({"type": "tool_call", "name": name, "input": kwargs}) + "\n")
    _real_out.flush()
    line = sys.stdin.readline()
    if not line:
        raise RuntimeError("kernel connection closed")
    msg = json.loads(line)
    if msg.get("type") != "tool_result":
        raise RuntimeError("unexpected kernel message: %r" % (msg,))
    if not msg.get("ok"):
        raise RuntimeError(msg.get("error") or "tool call failed")
    return msg.get("content", "")

_ns["call_tool"] = call_tool

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    req = json.loads(line)
    if req.get("type") != "exec":
        continue
    buf = io.StringIO()
    try:
        with contextlib.redirect_stdout(buf):
            exec(compile(req["code"], "<code_execute>", "exec"), _ns)
        reply = {"type": "result", "ok": True, "stdout": buf.getvalue()}
    except Exception:
        reply = {"type": "result", "ok": False, "stdout": buf.getvalue(),
                 "error": traceback.format_exc()}
    _real_out.write(json.dumps(reply) + "\n")
    _real_out.flush()
"#;

/// A live Python interpreter bound to one session.
struct Process {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr: ChildStderr,
    last_used: Instant,
}

impl Process {
    async fn drain_stderr(&mut self) -> String {
        // Non-blocking-ish: only read what's already buffered.
        let mut buf = vec![0u8; 8192];
        use tokio::io::AsyncReadExt;
        match tokio::time::timeout(Duration::from_millis(20), self.stderr.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => String::from_utf8_lossy(&buf[..n]).to_string(),
            _ => String::new(),
        }
    }
}

/// Manages the persistent interpreters.
pub struct CodeKernelManager {
    interpreter: String,
    workspace: Option<PathBuf>,
    sessions: Mutex<HashMap<String, Arc<Mutex<Process>>>>,
    max_idle: Duration,
    timeout: Duration,
}

impl CodeKernelManager {
    pub fn new(interpreter: String, workspace: Option<PathBuf>, timeout: Duration) -> Self {
        Self {
            interpreter,
            workspace,
            sessions: Mutex::new(HashMap::new()),
            max_idle: Duration::from_secs(30 * 60),
            timeout,
        }
    }

    async fn spawn(&self) -> Result<Process, String> {
        let mut cmd = Command::new(&self.interpreter);
        cmd.arg("-u")
            .arg("-c")
            .arg(BOOTSTRAP)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if let Some(ws) = &self.workspace {
            cmd.current_dir(ws);
        }
        // Same isolation posture as shell_exec: no inherited credentials.
        cmd.env_clear();
        for var in ["PATH", "HOME", "LANG", "LC_ALL", "TMPDIR"] {
            if let Ok(v) = std::env::var(var) {
                cmd.env(var, v);
            }
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to start '{0}': {e}. Is python3 installed?", self.interpreter))?;
        let stdin = child.stdin.take().ok_or("no stdin on code kernel")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("no stdout on code kernel")?);
        let stderr = child.stderr.take().ok_or("no stderr on code kernel")?;
        Ok(Process {
            child,
            stdin,
            stdout,
            stderr,
            last_used: Instant::now(),
        })
    }

    async fn session(&self, key: &str) -> Result<Arc<Mutex<Process>>, String> {
        let mut sessions = self.sessions.lock().await;
        sessions.retain(|_, p| {
            // Best-effort reaping of idle interpreters.
            p.try_lock()
                .map(|proc| proc.last_used.elapsed() < self.max_idle || proc.child.id().is_some())
                .unwrap_or(true)
        });
        if let Some(existing) = sessions.get(key) {
            return Ok(Arc::clone(existing));
        }
        let proc = self.spawn().await?;
        let arc = Arc::new(Mutex::new(proc));
        sessions.insert(key.to_string(), Arc::clone(&arc));
        Ok(arc)
    }

    /// Execute `code` in the session's interpreter.
    ///
    /// `dispatch` is invoked for every `call_tool(...)` the script makes; it
    /// receives (tool_name, input_json) and returns the tool's string result.
    pub async fn execute<F, Fut>(
        &self,
        session_key: &str,
        code: &str,
        dispatch: F,
    ) -> Result<String, String>
    where
        F: Fn(String, serde_json::Value) -> Fut,
        Fut: std::future::Future<Output = Result<String, String>>,
    {
        let proc = self.session(session_key).await?;
        let mut proc = proc.lock().await;
        proc.last_used = Instant::now();

        let payload = serde_json::json!({"type": "exec", "code": code});
        let line = format!("{payload}\n");
        if let Err(e) = proc.stdin.write_all(line.as_bytes()).await {
            // Interpreter died — drop it so the next call respawns.
            let _ = proc.child.kill().await;
            return Err(format!("code kernel died (write failed: {e}); retry the call"));
        }
        let _ = proc.stdin.flush().await;

        let deadline = tokio::time::Instant::now() + self.timeout;
        let mut buf = String::new();
        loop {
            buf.clear();
            let read = tokio::time::timeout_at(deadline, proc.stdout.read_line(&mut buf)).await;
            match read {
                Err(_) => {
                    let _ = proc.child.kill().await;
                    return Err(format!(
                        "code execution timed out after {}s (interpreter killed)",
                        self.timeout.as_secs()
                    ));
                }
                Ok(Err(e)) => {
                    let _ = proc.child.kill().await;
                    return Err(format!("code kernel read error: {e}"));
                }
                Ok(Ok(0)) => {
                    let err = proc.drain_stderr().await;
                    return Err(format!(
                        "code kernel exited unexpectedly.{}",
                        if err.trim().is_empty() {
                            String::new()
                        } else {
                            format!(" stderr: {}", err.trim())
                        }
                    ));
                }
                Ok(Ok(_)) => {}
            }

            let msg: serde_json::Value = match serde_json::from_str(buf.trim()) {
                Ok(v) => v,
                Err(_) => continue, // stray output (e.g. a print that bypassed redirect)
            };

            match msg["type"].as_str() {
                Some("tool_call") => {
                    let name = msg["name"].as_str().unwrap_or("").to_string();
                    let input = msg["input"].clone();
                    debug!(tool = %name, "code kernel nested tool call");
                    let reply = if !NESTED_TOOLS.contains(&name.as_str()) {
                        serde_json::json!({
                            "type": "tool_result",
                            "ok": false,
                            "error": format!(
                                "'{name}' is not available inside code_execute. Allowed: {}",
                                NESTED_TOOLS.join(", ")
                            )
                        })
                    } else {
                        match dispatch(name, input).await {
                            Ok(content) => serde_json::json!({
                                "type": "tool_result", "ok": true, "content": content
                            }),
                            Err(e) => serde_json::json!({
                                "type": "tool_result", "ok": false, "error": e
                            }),
                        }
                    };
                    let reply_line = format!("{reply}\n");
                    if proc.stdin.write_all(reply_line.as_bytes()).await.is_err() {
                        let _ = proc.child.kill().await;
                        return Err("code kernel died while replying to a tool call".to_string());
                    }
                    let _ = proc.stdin.flush().await;
                }
                Some("result") => {
                    let stdout = msg["stdout"].as_str().unwrap_or("");
                    if msg["ok"].as_bool().unwrap_or(false) {
                        return Ok(stdout.to_string());
                    }
                    let err = msg["error"].as_str().unwrap_or("unknown error");
                    return Ok(format!("{stdout}\n[error]\n{err}").trim().to_string());
                }
                _ => continue,
            }
        }
    }

    /// Kill every interpreter (daemon shutdown).
    pub async fn shutdown(&self) {
        let mut sessions = self.sessions.lock().await;
        for (key, proc) in sessions.drain() {
            if let Ok(mut p) = proc.try_lock() {
                if let Err(e) = p.child.kill().await {
                    warn!(session = %key, error = %e, "failed to kill code kernel");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn ok_dispatch(name: String, input: serde_json::Value) -> Result<String, String> {
        Ok(format!("{name}:{}", input["v"].as_str().unwrap_or("")))
    }

    fn have_python() -> bool {
        std::process::Command::new("python3")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[tokio::test]
    async fn test_execute_and_persist_state() {
        if !have_python() {
            return;
        }
        let mgr = CodeKernelManager::new("python3".into(), None, Duration::from_secs(20));
        let out = mgr
            .execute("s1", "x = 41\nprint('hello')", ok_dispatch)
            .await
            .unwrap();
        assert!(out.contains("hello"), "got: {out}");

        // State persists across invocations in the same session.
        let out = mgr.execute("s1", "print(x + 1)", ok_dispatch).await.unwrap();
        assert!(out.contains("42"), "got: {out}");

        // A different session is a fresh interpreter.
        let out = mgr.execute("s2", "print('x' in globals())", ok_dispatch)
            .await
            .unwrap();
        assert!(out.contains("False"), "got: {out}");
    }

    #[tokio::test]
    async fn test_nested_tool_call_roundtrip() {
        if !have_python() {
            return;
        }
        let mgr = CodeKernelManager::new("python3".into(), None, Duration::from_secs(20));
        let out = mgr
            .execute(
                "s3",
                "r = call_tool('file_read', v='alpha')\nprint('got', r)",
                ok_dispatch,
            )
            .await
            .unwrap();
        assert!(out.contains("got file_read:alpha"), "got: {out}");

        // Tool errors surface as exceptions inside the script.
        let out = mgr
            .execute(
                "s3",
                "try:\n    call_tool('shell_exec', command='rm -rf /')\nexcept RuntimeError as e:\n    print('blocked')",
                ok_dispatch,
            )
            .await
            .unwrap();
        assert!(out.contains("blocked"), "got: {out}");
    }

    #[tokio::test]
    async fn test_syntax_error_reported_not_fatal() {
        if !have_python() {
            return;
        }
        let mgr = CodeKernelManager::new("python3".into(), None, Duration::from_secs(20));
        let out = mgr.execute("s4", "def (: pass", ok_dispatch).await.unwrap();
        assert!(out.contains("[error]"), "got: {out}");
        // Session still usable afterwards.
        let out = mgr.execute("s4", "print('alive')", ok_dispatch).await.unwrap();
        assert!(out.contains("alive"), "got: {out}");
    }
}
