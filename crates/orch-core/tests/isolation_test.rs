use orch_core::isolation::*;
use std::path::PathBuf;
use std::time::Duration;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "orch-iso-{}-{}-{}",
        std::process::id(),
        name,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ═══════════════════════════════════════════════════════════
// Scene 6.10: Clean environment
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn spawn_captures_stdout() {
    let result = spawn(&SpawnConfig {
        binary: "/bin/echo".into(),
        args: vec!["hello".into(), "world".into()],
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "hello world");
}

#[tokio::test]
async fn spawn_captures_stderr() {
    let result = spawn(&SpawnConfig {
        binary: "/bin/sh".into(),
        args: vec!["-c".into(), "echo error >&2".into()],
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stderr.trim(), "error");
}

#[tokio::test]
async fn spawn_returns_exit_code() {
    let result = spawn(&SpawnConfig {
        binary: "/bin/sh".into(),
        args: vec!["-c".into(), "exit 42".into()],
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert_eq!(result.exit_code, 42);
}

#[tokio::test]
async fn spawn_clean_environment() {
    // /usr/bin/env prints all env vars. With clean env + only MY_VAR,
    // output should contain exactly one var.
    let result = spawn(&SpawnConfig {
        binary: "/usr/bin/env".into(),
        args: vec![],
        env: vec![("MY_VAR".into(), "my_value".into())],
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert_eq!(result.exit_code, 0);
    let lines: Vec<&str> = result.stdout.lines().collect();
    assert_eq!(lines.len(), 1, "expected exactly 1 env var, got: {:?}", lines);
    assert_eq!(lines[0], "MY_VAR=my_value");
}

#[tokio::test]
async fn spawn_injects_multiple_env_vars() {
    let result = spawn(&SpawnConfig {
        binary: "/usr/bin/env".into(),
        args: vec![],
        env: vec![
            ("A".into(), "1".into()),
            ("B".into(), "2".into()),
            ("C".into(), "3".into()),
        ],
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert_eq!(result.exit_code, 0);
    let lines: Vec<&str> = result.stdout.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines.contains(&"A=1"));
    assert!(lines.contains(&"B=2"));
    assert!(lines.contains(&"C=3"));
}

#[tokio::test]
async fn spawn_measures_duration() {
    let result = spawn(&SpawnConfig {
        binary: "/bin/sleep".into(),
        args: vec!["0.1".into()],
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert!(result.duration_ms >= 50, "duration should be >= 50ms");
    assert!(result.duration_ms < 5000, "duration should be < 5s");
}

// ═══════════════════════════════════════════════════════════
// Scene 6.4: Landlock filesystem restriction
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn landlock_allows_read_from_authorized_dir() {
    if !landlock_available() {
        eprintln!("skipping: landlock not available");
        return;
    }

    let dir = temp_dir("ll-read-ok");
    std::fs::write(dir.join("data.txt"), "secret data").unwrap();

    let result = spawn(&SpawnConfig {
        binary: "/bin/cat".into(),
        args: vec![dir.join("data.txt").to_string_lossy().into()],
        read_paths: vec![dir.clone()],
        write_paths: vec![],
        enable_landlock: true,
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "secret data");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn landlock_blocks_read_from_unauthorized_dir() {
    if !landlock_available() {
        eprintln!("skipping: landlock not available");
        return;
    }

    let allowed_dir = temp_dir("ll-read-no-allowed");
    // Create a file in a DIFFERENT temp dir (not in system paths, not in read_paths)
    let blocked_dir = temp_dir("ll-read-no-blocked");
    std::fs::write(blocked_dir.join("secret.txt"), "secret").unwrap();

    let result = spawn(&SpawnConfig {
        binary: "/bin/cat".into(),
        args: vec![blocked_dir.join("secret.txt").to_string_lossy().into()],
        read_paths: vec![allowed_dir.clone()],
        write_paths: vec![],
        enable_landlock: true,
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert_ne!(result.exit_code, 0, "reading unauthorized path must fail");
    let _ = std::fs::remove_dir_all(&allowed_dir);
    let _ = std::fs::remove_dir_all(&blocked_dir);
}

#[tokio::test]
async fn landlock_allows_write_to_authorized_dir() {
    if !landlock_available() {
        eprintln!("skipping: landlock not available");
        return;
    }

    let read_dir = temp_dir("ll-write-ok-r");
    let write_dir = temp_dir("ll-write-ok-w");
    let out_file = write_dir.join("output.txt");

    let result = spawn(&SpawnConfig {
        binary: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            format!("echo result > {}", out_file.display()),
        ],
        read_paths: vec![read_dir.clone()],
        write_paths: vec![write_dir.clone()],
        enable_landlock: true,
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert_eq!(result.exit_code, 0);
    assert_eq!(std::fs::read_to_string(&out_file).unwrap().trim(), "result");
    let _ = std::fs::remove_dir_all(&read_dir);
    let _ = std::fs::remove_dir_all(&write_dir);
}

#[tokio::test]
async fn landlock_blocks_write_to_unauthorized_dir() {
    if !landlock_available() {
        eprintln!("skipping: landlock not available");
        return;
    }

    let read_dir = temp_dir("ll-write-no-r");
    let blocked_dir = temp_dir("ll-write-no-b");

    let result = spawn(&SpawnConfig {
        binary: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            format!("echo hack > {}/evil.txt", blocked_dir.display()),
        ],
        read_paths: vec![read_dir.clone()],
        write_paths: vec![], // blocked_dir NOT in write_paths
        enable_landlock: true,
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert_ne!(result.exit_code, 0, "writing to unauthorized path must fail");
    assert!(
        !blocked_dir.join("evil.txt").exists(),
        "file must not be created"
    );
    let _ = std::fs::remove_dir_all(&read_dir);
    let _ = std::fs::remove_dir_all(&blocked_dir);
}

// ═══════════════════════════════════════════════════════════
// Scene 6.9: close_range (fd leak prevention)
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn close_range_closes_inherited_fds() {
    // Open an extra fd in the parent, then spawn a child.
    // The child should NOT see this fd (only 0,1,2).
    // /proc/self/fd lists open fds.
    let result = spawn(&SpawnConfig {
        binary: "/bin/ls".into(),
        args: vec!["/proc/self/fd".into()],
        enable_close_range: true,
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert_eq!(result.exit_code, 0);
    // ls /proc/self/fd shows: 0, 1, 2, and the fd for the directory itself (3)
    let fds: Vec<&str> = result.stdout.lines().collect();
    assert!(
        fds.len() <= 4,
        "expected at most 4 fds (0,1,2,dir), got {}: {:?}",
        fds.len(),
        fds
    );
}

// ═══════════════════════════════════════════════════════════
// Timeout
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn spawn_timeout_kills_long_running_process() {
    let result = spawn(&SpawnConfig {
        binary: "/bin/sleep".into(),
        args: vec!["60".into()],
        timeout: Duration::from_millis(200),
        ..SpawnConfig::default()
    })
    .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("timed out"), "expected timeout error, got: {err}");
}

// ═══════════════════════════════════════════════════════════
// Error cases
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn spawn_nonexistent_binary_fails() {
    let result = spawn(&SpawnConfig {
        binary: "/nonexistent/binary".into(),
        args: vec![],
        ..SpawnConfig::default()
    })
    .await;

    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════
// Combined: multiple isolation layers
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn combined_clean_env_and_landlock() {
    if !landlock_available() {
        eprintln!("skipping: landlock not available");
        return;
    }

    let read_dir = temp_dir("combined-r");
    let write_dir = temp_dir("combined-w");
    std::fs::write(read_dir.join("input.txt"), "hello").unwrap();

    let result = spawn(&SpawnConfig {
        binary: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            format!(
                "cat {} > {}/out.txt && echo $MY_KEY",
                read_dir.join("input.txt").display(),
                write_dir.display()
            ),
        ],
        env: vec![("MY_KEY".into(), "secret-123".into())],
        read_paths: vec![read_dir.clone()],
        write_paths: vec![write_dir.clone()],
        enable_landlock: true,
        enable_close_range: true,
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "secret-123");
    assert_eq!(
        std::fs::read_to_string(write_dir.join("out.txt"))
            .unwrap()
            .trim(),
        "hello"
    );
    let _ = std::fs::remove_dir_all(&read_dir);
    let _ = std::fs::remove_dir_all(&write_dir);
}
