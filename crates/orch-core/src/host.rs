use anyhow::{Result, anyhow};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct HostInfo {
    pub kernel: KernelInfo,
    pub security: SecurityInfo,
    pub providers: Vec<DetectedProvider>,
    pub resources: HostResources,
}

impl HostInfo {
    pub fn detect() -> Result<Self> {
        Ok(Self {
            kernel: detect_kernel()?,
            security: detect_security(),
            providers: vec![],
            resources: detect_resources()?,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct KernelInfo {
    pub release: String,
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl KernelInfo {
    pub fn parse(release: &str) -> Result<Self> {
        let parts: Vec<&str> = release.split('.').collect();
        if parts.len() < 3 {
            return Err(anyhow!("invalid kernel version: {}", release));
        }
        let major: u32 = parts[0].parse()?;
        let minor: u32 = parts[1].parse()?;
        let patch_str = parts[2].split('-').next().unwrap_or("0");
        let patch: u32 = patch_str.parse()?;
        Ok(Self {
            release: release.to_string(),
            major,
            minor,
            patch,
        })
    }

    pub fn meets_minimum(&self, major: u32, minor: u32) -> bool {
        (self.major, self.minor) >= (major, minor)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SecurityInfo {
    pub landlock_abi: u32,
    pub seccomp: bool,
    pub cgroup_v2: bool,
    pub pidfd: bool,
    pub user_namespaces: bool,
    pub apparmor: bool,
    pub selinux: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DetectedProvider {
    pub name: String,
    pub binary: String,
    pub available: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostResources {
    pub cpu_count: u32,
    pub memory_total_bytes: u64,
}

// ---- Detection functions ----

pub fn detect_kernel() -> Result<KernelInfo> {
    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease")?;
    KernelInfo::parse(release.trim())
}

pub fn detect_security() -> SecurityInfo {
    SecurityInfo {
        landlock_abi: detect_landlock_abi(),
        seccomp: std::path::Path::new("/proc/sys/kernel/seccomp/actions_avail").exists(),
        cgroup_v2: std::path::Path::new("/sys/fs/cgroup/cgroup.controllers").exists(),
        pidfd: detect_pidfd(),
        user_namespaces: detect_user_namespaces(),
        apparmor: detect_apparmor(),
        selinux: detect_selinux(),
    }
}

pub fn detect_resources() -> Result<HostResources> {
    let cpu_count = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);

    let meminfo = std::fs::read_to_string("/proc/meminfo")?;
    let memory_total_bytes = parse_memtotal(&meminfo)?;

    Ok(HostResources {
        cpu_count,
        memory_total_bytes,
    })
}

pub fn find_in_path(binary: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(binary))
            .find(|p| p.is_file())
    })
}

pub fn parse_memtotal(meminfo: &str) -> Result<u64> {
    for line in meminfo.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let kb: u64 = rest
                .split_whitespace()
                .next()
                .ok_or_else(|| anyhow!("malformed MemTotal line"))?
                .parse()?;
            return Ok(kb * 1024);
        }
    }
    Err(anyhow!("MemTotal not found in /proc/meminfo"))
}

// ---- Internal detection helpers ----

fn detect_landlock_abi() -> u32 {
    // syscall(SYS_landlock_create_ruleset, NULL, 0, LANDLOCK_CREATE_RULESET_VERSION)
    // Returns ABI version on success, -1 on error
    const LANDLOCK_CREATE_RULESET_VERSION: libc::c_ulong = 1 << 0;

    #[cfg(target_arch = "x86_64")]
    const SYS_LANDLOCK_CREATE_RULESET: libc::c_long = 444;
    #[cfg(target_arch = "aarch64")]
    const SYS_LANDLOCK_CREATE_RULESET: libc::c_long = 444;

    let ret = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            std::ptr::null::<libc::c_void>(),
            0_usize,
            LANDLOCK_CREATE_RULESET_VERSION,
        )
    };

    if ret >= 0 { ret as u32 } else { 0 }
}

fn detect_pidfd() -> bool {
    // pidfd_open is available since kernel 5.3
    // Try opening our own pid — if syscall exists, it works
    let ret = unsafe { libc::syscall(libc::SYS_pidfd_open, std::process::id() as libc::c_int, 0) };
    if ret >= 0 {
        unsafe { libc::close(ret as libc::c_int) };
        true
    } else {
        false
    }
}

fn detect_user_namespaces() -> bool {
    // Check /proc/sys/user/max_user_namespaces > 0
    std::fs::read_to_string("/proc/sys/user/max_user_namespaces")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .is_some_and(|n| n > 0)
}

fn detect_apparmor() -> bool {
    std::fs::read_to_string("/sys/module/apparmor/parameters/enabled")
        .ok()
        .is_some_and(|s| s.trim() == "Y")
}

fn detect_selinux() -> bool {
    std::path::Path::new("/sys/fs/selinux/enforce").exists()
}
