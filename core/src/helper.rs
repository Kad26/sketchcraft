//! Running another program (ffmpeg, Python, a DWG converter, curl): always with a hard time limit,
//! and the helper plus everything it started is killed when the limit passes, or when SketchCraft or
//! the MCP server exits (SPECTER-005). Arguments only, never a shell.

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Longest an output stream is kept (the rest is read and dropped, so a chatty helper can't fill memory or block).
const KEEP: usize = 1 << 20;

/// Time limits, by kind of helper.
pub const VIDEO: Duration = Duration::from_secs(600);
pub const IFC: Duration = Duration::from_secs(300);
pub const DWG: Duration = Duration::from_secs(180);
pub const PROBE: Duration = Duration::from_secs(30);
pub const DOWNLOAD: Duration = Duration::from_secs(60);

/// Helpers running right now (their process ids; each leads its own process group on Unix).
static RUNNING: Mutex<Vec<u32>> = Mutex::new(Vec::new());

fn running() -> std::sync::MutexGuard<'static, Vec<u32>> {
    RUNNING.lock().unwrap_or_else(|e| e.into_inner())
}

/// Kill a helper and everything it started.
fn kill_tree(pid: u32) {
    #[cfg(unix)]
    {
        // the helper leads its own process group (see `run`): one signal reaches all of it
        let _ = Command::new("kill").args(["-KILL", "--", &format!("-{pid}")]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

/// Kill every helper still running. Called when the app or the MCP server exits.
pub fn kill_all() {
    let pids: Vec<u32> = std::mem::take(&mut *running());
    for p in pids {
        kill_tree(p);
    }
}

fn drain(mut r: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let (mut keep, mut buf) = (Vec::new(), [0u8; 8192]);
        while let Ok(n) = r.read(&mut buf) {
            if n == 0 {
                break;
            }
            if keep.len() < KEEP {
                keep.extend_from_slice(&buf[..n.min(KEEP - keep.len())]);
            }
        }
        keep
    })
}

/// Run `c` for at most `limit`. On time-out the helper and its whole process tree are killed and the
/// error says so (`ErrorKind::TimedOut`). Nothing is read from the keyboard.
pub fn run(mut c: Command, limit: Duration) -> std::io::Result<Output> {
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    let mut child = c.spawn()?;
    let pid = child.id();
    running().push(pid);
    let out = child.stdout.take().map(drain);
    let err = child.stderr.take().map(drain);
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Ok(s),
            Ok(None) if start.elapsed() >= limit => {
                kill_tree(pid);
                let _ = child.kill();
                let _ = child.wait();
                running().retain(|p| *p != pid);
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, format!("it ran longer than {} seconds and was stopped", limit.as_secs())));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => break Err(e),
        }
    };
    running().retain(|p| *p != pid);
    let status = status?;
    let join = |h: Option<std::thread::JoinHandle<Vec<u8>>>| h.and_then(|h| h.join().ok()).unwrap_or_default();
    Ok(Output { status, stdout: join(out), stderr: join(err) })
}

/// A short phrase for a failed start or a time-out, to put after "couldn't run X".
pub fn why(e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::TimedOut {
        e.to_string()
    } else {
        "it couldn't be started".to_string()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_helper_that_runs_too_long_is_killed_with_what_it_started() {
        // a shell that starts a grandchild sleeping a minute: both must be gone at the time limit
        let mut c = Command::new("sh");
        c.args(["-c", "sleep 60 & echo $! ; sleep 60"]);
        let t = Instant::now();
        let r = run(c, Duration::from_millis(400));
        assert!(t.elapsed() < Duration::from_secs(10));
        assert_eq!(r.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
        assert!(running().is_empty());
    }

    #[test]
    fn a_quick_helper_returns_its_output() {
        let mut c = Command::new("sh");
        c.args(["-c", "echo hello; echo oops 1>&2"]);
        let o = run(c, Duration::from_secs(10)).unwrap();
        assert!(o.status.success());
        assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "hello");
        assert_eq!(String::from_utf8_lossy(&o.stderr).trim(), "oops");
    }
}
