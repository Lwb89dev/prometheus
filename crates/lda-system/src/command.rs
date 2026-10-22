use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use lda_common::{Error, Result};

const MAX_OUTPUT: usize = 64 * 1024;

/// Run a program with a fixed argument list. There is no shell.
pub fn run_argv(program: &Path, args: &[&str], timeout: Duration) -> Result<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdout_thread =
        std::thread::spawn(move || stdout_pipe.map(read_limited).unwrap_or_default());
    let stderr_thread =
        std::thread::spawn(move || stderr_pipe.map(read_limited).unwrap_or_default());
    let started = Instant::now();
    let status = loop {
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            return Err(Error::Timeout(format!("{} timed out", program.display())));
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();
    if !status.success() {
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(Error::Other(format!(
            "{} exited {}: {}",
            program.display(),
            status.code().unwrap_or(-1),
            clip(&detail, 400)
        )));
    }
    Ok(stdout)
}

fn read_limited(mut pipe: impl Read) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    while buf.len() < MAX_OUTPUT {
        let want = (MAX_OUTPUT - buf.len()).min(chunk.len());
        match pipe.read(&mut chunk[..want]) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let mut sink = [0u8; 4096];
    while pipe.read(&mut sink).unwrap_or(0) > 0 {}
    String::from_utf8_lossy(&buf).into_owned()
}

fn clip(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

pub fn find_program(candidates: &[&str]) -> Option<PathBuf> {
    for candidate in candidates {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for candidate in candidates {
            let name = Path::new(candidate).file_name()?;
            let joined = dir.join(name);
            if joined.is_file() {
                return Some(joined);
            }
        }
    }
    None
}
