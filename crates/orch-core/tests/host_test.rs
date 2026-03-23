use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

use orch_core::host::*;

// ---- Scene 4.1: Kernel version parsing ----

#[test]
fn parse_kernel_version_standard() {
    let info = KernelInfo::parse("6.18.13-200.fc43.x86_64").unwrap();
    assert_eq!(info.major, 6);
    assert_eq!(info.minor, 18);
    assert_eq!(info.patch, 13);
    assert_eq!(info.release, "6.18.13-200.fc43.x86_64");
}

#[test]
fn parse_kernel_version_minimal() {
    let info = KernelInfo::parse("5.15.0").unwrap();
    assert_eq!(info.major, 5);
    assert_eq!(info.minor, 15);
    assert_eq!(info.patch, 0);
}

#[test]
fn parse_kernel_version_with_suffix() {
    let info = KernelInfo::parse("6.12.1-arch1-1").unwrap();
    assert_eq!(info.major, 6);
    assert_eq!(info.minor, 12);
    assert_eq!(info.patch, 1);
}

#[test]
fn kernel_meets_minimum_exact() {
    let info = KernelInfo::parse("6.12.0").unwrap();
    assert!(info.meets_minimum(6, 12));
}

#[test]
fn kernel_meets_minimum_above() {
    let info = KernelInfo::parse("6.18.13").unwrap();
    assert!(info.meets_minimum(6, 12));
}

#[test]
fn kernel_below_minimum() {
    let info = KernelInfo::parse("5.15.0").unwrap();
    assert!(!info.meets_minimum(6, 12));
}

#[test]
fn kernel_major_above_minimum() {
    let info = KernelInfo::parse("7.0.0").unwrap();
    assert!(info.meets_minimum(6, 12));
}

// ---- Scene 4.6: Resource parsing ----

#[test]
fn parse_memtotal_from_meminfo() {
    let meminfo = "\
MemTotal:       16384000 kB
MemFree:         8192000 kB
MemAvailable:   12288000 kB
";
    let bytes = parse_memtotal(meminfo).unwrap();
    assert_eq!(bytes, 16_384_000 * 1024);
}

// ---- Scene 4.1-4.6: Detection on real system ----

#[test]
fn detect_kernel_on_this_system() {
    let info = detect_kernel().unwrap();
    assert!(info.major >= 5, "kernel too old: {}", info.release);
}

#[test]
fn detect_security_capabilities() {
    let sec = detect_security();
    // On a modern Linux system, at least seccomp or cgroup_v2 should be available
    assert!(
        sec.seccomp || sec.cgroup_v2,
        "expected at least seccomp or cgroup_v2"
    );
}

#[test]
fn detect_host_resources() {
    let res = detect_resources().unwrap();
    assert!(res.cpu_count > 0);
    assert!(res.memory_total_bytes > 0);
}

// ---- Scene 4.5: Provider binary detection ----

#[test]
fn find_binary_that_exists() {
    // `ls` is guaranteed to be in PATH on Linux
    assert!(find_in_path("ls").is_some());
}

#[test]
fn find_binary_that_does_not_exist() {
    assert!(find_in_path("nonexistent-binary-orch-test-12345").is_none());
}

// ---- Scene 4.7: HostInfo::detect caches everything ----

#[test]
fn host_info_detect_returns_all_fields() {
    let info = HostInfo::detect().unwrap();
    assert!(info.kernel.major >= 5);
    assert!(info.resources.cpu_count > 0);
    assert!(info.resources.memory_total_bytes > 0);
    // security is a struct — just verify it exists
    let _ = info.security.landlock_abi;
}

// ---- Scene 4.8: GET /v1/system/info endpoint ----

#[tokio::test]
async fn info_endpoint_returns_host_info() {
    let app = orch_core::server::app_stateless();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/system/info")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert!(json["kernel"]["major"].is_u64());
    assert!(json["kernel"]["release"].is_string());
    assert!(json["security"]["seccomp"].is_boolean());
    assert!(json["security"]["landlock_abi"].is_u64());
    assert!(json["resources"]["cpu_count"].is_u64());
    assert!(json["resources"]["memory_total_bytes"].is_u64());
}
