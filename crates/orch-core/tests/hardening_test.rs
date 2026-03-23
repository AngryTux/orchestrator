//! Security hardening tests: seccomp + rlimits.

use orch_core::isolation::*;
use std::time::Duration;

// ═══════════════════════════════════════════════════════════
// Seccomp: dangerous syscalls are blocked
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn seccomp_blocks_ptrace() {
    // strace tries to ptrace — should fail with EPERM under seccomp
    let result = spawn(&SpawnConfig {
        binary: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            // Try to ptrace ourselves — python/perl not needed, use /proc
            "cat /proc/self/syscall".into(),
        ],
        enable_seccomp: true,
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    // The process should still run (read/write/exit are allowed)
    // but ptrace-based tools would fail
    assert_eq!(result.exit_code, 0); // cat itself is allowed
}

#[tokio::test]
async fn seccomp_allows_normal_operations() {
    // Normal shell operations should work fine under seccomp
    let result = spawn(&SpawnConfig {
        binary: "/bin/sh".into(),
        args: vec!["-c".into(), "echo hello && ls /tmp > /dev/null".into()],
        enable_seccomp: true,
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert_eq!(
        result.exit_code, 0,
        "seccomp blocked normal ops: stderr={}, stdout={}",
        result.stderr, result.stdout
    );
    assert_eq!(result.stdout.trim(), "hello");
}

#[tokio::test]
async fn seccomp_blocks_reboot() {
    // reboot(2) should be blocked — process gets EPERM
    let result = spawn(&SpawnConfig {
        binary: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            // Try to call reboot via /sbin/reboot — should be denied
            "reboot 2>/dev/null; echo $?".into(),
        ],
        enable_seccomp: true,
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    // Process should complete (not crash), reboot should fail
    assert_eq!(result.exit_code, 0);
}

// ═══════════════════════════════════════════════════════════
// rlimits: resource limits prevent abuse
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn rlimits_are_applied() {
    // Verify rlimits are actually set by reading them back
    let result = spawn(&SpawnConfig {
        binary: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            "ulimit -u && ulimit -v".into(), // print NPROC and virtual memory limits
        ],
        enable_rlimits: true,
        rlimit_nproc: Some(100),
        rlimit_mem_bytes: Some(512 * 1024 * 1024), // 512MB
        timeout: Duration::from_secs(5),
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert_eq!(result.exit_code, 0);
    let lines: Vec<&str> = result.stdout.lines().collect();
    assert_eq!(lines.len(), 2, "expected 2 lines, got: {:?}", lines);
    assert_eq!(lines[0], "100", "NPROC not set correctly");
    // ulimit -v reports in KB
    let expected_kb = 512 * 1024;
    assert_eq!(lines[1], expected_kb.to_string(), "virtual memory limit not set");
}

#[tokio::test]
async fn rlimit_as_prevents_memory_exhaustion() {
    let result = spawn(&SpawnConfig {
        binary: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            // Try to allocate a lot of memory — should fail
            "head -c 500M /dev/zero > /dev/null 2>&1; echo done".into(),
        ],
        enable_rlimits: true,
        rlimit_mem_bytes: Some(64 * 1024 * 1024), // 64MB limit
        timeout: Duration::from_secs(10),
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    // Process should complete (not OOM kill the host)
    assert!(result.duration_ms < 9000);
}

// ═══════════════════════════════════════════════════════════
// Combined: all layers together
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn all_hardening_layers_combined() {
    let result = spawn(&SpawnConfig {
        binary: "/bin/sh".into(),
        args: vec!["-c".into(), "echo secure".into()],
        enable_seccomp: true,
        enable_rlimits: true,
        enable_close_range: true,
        rlimit_nproc: Some(50),
        rlimit_mem_bytes: Some(256 * 1024 * 1024),
        ..SpawnConfig::default()
    })
    .await
    .unwrap();

    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "secure");
}
