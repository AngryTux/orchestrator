use anyhow::Context;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

/// Configuration for spawning an isolated process.
#[derive(Debug, Clone)]
pub struct SpawnConfig {
    pub binary: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub read_paths: Vec<PathBuf>,
    pub write_paths: Vec<PathBuf>,
    pub enable_landlock: bool,
    pub enable_close_range: bool,
    pub enable_seccomp: bool,
    pub enable_rlimits: bool,
    pub rlimit_nproc: Option<u64>,
    pub rlimit_mem_bytes: Option<u64>,
    pub timeout: Duration,
}

impl Default for SpawnConfig {
    fn default() -> Self {
        Self {
            binary: String::new(),
            args: vec![],
            env: vec![],
            read_paths: vec![],
            write_paths: vec![],
            enable_landlock: false,
            enable_close_range: false,
            enable_seccomp: false,
            enable_rlimits: false,
            rlimit_nproc: None,
            rlimit_mem_bytes: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpawnResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub signal: Option<i32>,
}

/// Spawn a process with configured isolation layers.
///
/// Layers applied in the child (after fork, before exec):
/// 1. `PR_SET_NO_NEW_PRIVS` — unconditional
/// 2. rlimits — NPROC + AS (memory)
/// 3. Landlock — filesystem restriction
/// 4. Seccomp — syscall whitelist
/// 5. close_range — fd leak prevention
///
/// Order matters: Landlock before close_range (needs PathFd),
/// seccomp last (blocks syscalls used by earlier layers).
pub async fn spawn(config: &SpawnConfig) -> anyhow::Result<SpawnResult> {
    let config = config.clone();
    tokio::task::spawn_blocking(move || spawn_blocking(&config))
        .await
        .context("spawn task panicked")?
}

fn spawn_blocking(config: &SpawnConfig) -> anyhow::Result<SpawnResult> {
    let start = std::time::Instant::now();

    let mut cmd = Command::new(&config.binary);
    cmd.args(&config.args);
    cmd.env_clear();
    for (k, v) in &config.env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let enable_landlock = config.enable_landlock;
    let enable_close_range = config.enable_close_range;
    let enable_seccomp = config.enable_seccomp;
    let enable_rlimits = config.enable_rlimits;
    let rlimit_nproc = config.rlimit_nproc;
    let rlimit_mem_bytes = config.rlimit_mem_bytes;
    let read_paths = config.read_paths.clone();
    let write_paths = config.write_paths.clone();

    // SAFETY: pre_exec runs after fork() in the child's own address space.
    // Memory allocation (used by landlock crate) is safe because the child
    // has no other threads. Syscalls used are async-signal-safe.
    unsafe {
        cmd.pre_exec(move || {
            // 1. Prevent privilege escalation (unconditional)
            let ret = libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
            if ret != 0 {
                return Err(std::io::Error::last_os_error());
            }

            // 2. Resource limits (before spawning children)
            if enable_rlimits {
                apply_rlimits(rlimit_nproc, rlimit_mem_bytes)?;
            }

            // 3. Landlock filesystem (needs PathFd → must come before close_range)
            if enable_landlock {
                apply_landlock(&read_paths, &write_paths)?;
            }

            // 4. Seccomp syscall filter (after Landlock, which uses syscalls)
            if enable_seccomp {
                apply_seccomp()?;
            }

            // 5. Close inherited fds (last — uses CLOEXEC, closed on exec)
            if enable_close_range {
                apply_close_range()?;
            }

            Ok(())
        });
    }

    let mut child = cmd.spawn().context("failed to spawn process")?;

    loop {
        match child.try_wait().context("failed to check child status")? {
            Some(status) => {
                let (stdout, stderr) = read_child_output(&mut child)?;
                use std::os::unix::process::ExitStatusExt;
                return Ok(SpawnResult {
                    exit_code: status.code().unwrap_or(-1),
                    stdout,
                    stderr,
                    duration_ms: start.elapsed().as_millis() as u64,
                    signal: status.signal(),
                });
            }
            None if start.elapsed() >= config.timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(anyhow::anyhow!(
                    "process timed out after {}s",
                    config.timeout.as_secs()
                ));
            }
            None => {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn read_child_output(child: &mut std::process::Child) -> anyhow::Result<(String, String)> {
    use std::io::Read;
    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(ref mut out) = child.stdout {
        out.read_to_string(&mut stdout)?;
    }
    if let Some(ref mut err) = child.stderr {
        err.read_to_string(&mut stderr)?;
    }
    Ok((stdout, stderr))
}

pub fn landlock_available() -> bool {
    crate::host::detect_security().landlock_abi > 0
}

// ─── rlimits ─────────────────────────────────────────────

fn apply_rlimits(nproc: Option<u64>, mem_bytes: Option<u64>) -> Result<(), std::io::Error> {
    if let Some(n) = nproc {
        let limit = libc::rlimit {
            rlim_cur: n,
            rlim_max: n,
        };
        if unsafe { libc::setrlimit(libc::RLIMIT_NPROC, &limit) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    if let Some(bytes) = mem_bytes {
        let limit = libc::rlimit {
            rlim_cur: bytes,
            rlim_max: bytes,
        };
        if unsafe { libc::setrlimit(libc::RLIMIT_AS, &limit) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

// ─── Seccomp ─────────────────────────────────────────────

fn apply_seccomp() -> Result<(), std::io::Error> {
    // Allowlist of syscalls needed for a typical CLI process.
    // Everything NOT on this list returns EPERM.
    // Allowlist: syscalls needed by shells, CLIs, and dynamic linker.
    // Blocked (notably): ptrace, mount, reboot, kexec, init_module, bpf,
    // pivot_root, chroot, swapon/off, acct, settimeofday, personality.
    #[cfg(target_arch = "x86_64")]
    let allowed: &[i64] = &[
        // I/O
        libc::SYS_read, libc::SYS_write, libc::SYS_open, libc::SYS_close,
        libc::SYS_openat, libc::SYS_pread64, libc::SYS_pwrite64,
        libc::SYS_readv, libc::SYS_writev, libc::SYS_lseek,
        libc::SYS_ioctl, libc::SYS_pipe, libc::SYS_pipe2,
        libc::SYS_dup, libc::SYS_dup2, libc::SYS_dup3,
        // File metadata
        libc::SYS_stat, libc::SYS_fstat, libc::SYS_lstat,
        libc::SYS_newfstatat, libc::SYS_statx,
        libc::SYS_access, libc::SYS_faccessat, libc::SYS_faccessat2,
        libc::SYS_readlink, libc::SYS_readlinkat,
        libc::SYS_getdents64, libc::SYS_getcwd, libc::SYS_chdir, libc::SYS_fchdir,
        // File ops
        libc::SYS_mkdir, libc::SYS_mkdirat,
        libc::SYS_unlink, libc::SYS_unlinkat,
        libc::SYS_rename, libc::SYS_renameat2,
        libc::SYS_fcntl, libc::SYS_flock,
        libc::SYS_ftruncate, libc::SYS_fallocate,
        libc::SYS_umask, libc::SYS_chmod, libc::SYS_fchmod, libc::SYS_fchmodat,
        // Memory
        libc::SYS_mmap, libc::SYS_mprotect, libc::SYS_munmap, libc::SYS_brk,
        libc::SYS_madvise, libc::SYS_mremap,
        // Signals
        libc::SYS_rt_sigaction, libc::SYS_rt_sigprocmask, libc::SYS_rt_sigreturn,
        libc::SYS_sigaltstack, libc::SYS_tgkill, libc::SYS_kill,
        // Process
        libc::SYS_getpid, libc::SYS_getppid, libc::SYS_gettid,
        libc::SYS_getuid, libc::SYS_getgid, libc::SYS_geteuid, libc::SYS_getegid,
        libc::SYS_getgroups, libc::SYS_getresuid, libc::SYS_getresgid,
        libc::SYS_exit, libc::SYS_exit_group,
        libc::SYS_wait4, libc::SYS_waitid,
        libc::SYS_clone, libc::SYS_clone3, libc::SYS_fork, libc::SYS_vfork,
        libc::SYS_execve, libc::SYS_execveat,
        libc::SYS_set_tid_address, libc::SYS_prctl, libc::SYS_arch_prctl,
        libc::SYS_uname, libc::SYS_getpgrp, libc::SYS_setpgid, libc::SYS_getpgid,
        libc::SYS_setsid, libc::SYS_setuid, libc::SYS_setgid,
        // Scheduling / time
        libc::SYS_sched_yield, libc::SYS_sched_getaffinity,
        libc::SYS_nanosleep, libc::SYS_clock_nanosleep,
        libc::SYS_clock_gettime, libc::SYS_clock_getres, libc::SYS_gettimeofday,
        // Polling
        libc::SYS_select, libc::SYS_pselect6, libc::SYS_poll, libc::SYS_ppoll,
        libc::SYS_epoll_create1, libc::SYS_epoll_ctl, libc::SYS_epoll_wait,
        libc::SYS_epoll_pwait, libc::SYS_eventfd2,
        // Networking
        libc::SYS_socket, libc::SYS_connect, libc::SYS_accept, libc::SYS_accept4,
        libc::SYS_bind, libc::SYS_listen, libc::SYS_shutdown,
        libc::SYS_sendto, libc::SYS_recvfrom, libc::SYS_sendmsg, libc::SYS_recvmsg,
        libc::SYS_getsockopt, libc::SYS_setsockopt,
        libc::SYS_getsockname, libc::SYS_getpeername,
        // Threading / sync
        libc::SYS_futex, libc::SYS_set_robust_list, libc::SYS_get_robust_list,
        libc::SYS_rseq,
        // Misc safe
        libc::SYS_getrandom, libc::SYS_sysinfo,
        libc::SYS_prlimit64, libc::SYS_setrlimit, libc::SYS_getrlimit,
        libc::SYS_close_range,
    ];

    #[cfg(not(target_arch = "x86_64"))]
    return Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "seccomp only supported on x86_64",
    ));

    #[cfg(target_arch = "x86_64")]
    {
        let filter = build_seccomp_bpf(allowed);
        let prog = libc::sock_fprog {
            len: filter.len() as u16,
            filter: filter.as_ptr() as *mut libc::sock_filter,
        };

        // SECCOMP_SET_MODE_FILTER = 1
        let ret = unsafe { libc::syscall(libc::SYS_seccomp, 1_u32, 0_u32, &prog) };
        if ret != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(target_arch = "x86_64")]
fn build_seccomp_bpf(allowed: &[i64]) -> Vec<libc::sock_filter> {
    const AUDIT_ARCH_X86_64: u32 = 0xc000_003e;
    const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
    const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;

    let n = allowed.len();
    let mut f: Vec<libc::sock_filter> = Vec::with_capacity(n + 5);

    // [0] Load arch from seccomp_data (offset 4)
    f.push(bpf_stmt(0x20, 4)); // BPF_LD | BPF_W | BPF_ABS

    // [1] Verify x86_64 — if not, jump to deny (at index n+3)
    // From index 1: target = 1 + 1 + jf = n + 3, so jf = n + 1
    f.push(bpf_jump(0x15, AUDIT_ARCH_X86_64, 0, (n + 1) as u8));

    // [2] Load syscall number (offset 0)
    f.push(bpf_stmt(0x20, 0)); // BPF_LD | BPF_W | BPF_ABS

    // [3..n+2] Check each allowed syscall
    for (i, &nr) in allowed.iter().enumerate() {
        let jump_to_allow = (n - i) as u8; // skip remaining checks + deny
        f.push(bpf_jump(0x15, nr as u32, jump_to_allow, 0));
    }

    // [n+3] Default: deny with EPERM
    f.push(bpf_stmt(0x06, SECCOMP_RET_ERRNO | (libc::EPERM as u32)));

    // [n+4] Allow
    f.push(bpf_stmt(0x06, SECCOMP_RET_ALLOW));

    f
}

fn bpf_stmt(code: u16, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    }
}

fn bpf_jump(code: u16, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter { code, jt, jf, k }
}

// ─── Landlock ────────────────────────────────────────────

fn apply_landlock(
    read_paths: &[PathBuf],
    write_paths: &[PathBuf],
) -> Result<(), std::io::Error> {
    use landlock::{
        Access, AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr,
        RulesetStatus, ABI,
    };

    let abi = ABI::V4;
    let access_all = AccessFs::from_all(abi);
    let access_read = AccessFs::from_read(abi);
    let access_write = access_all & !AccessFs::Execute;

    let mut ruleset = Ruleset::default()
        .handle_access(access_all)
        .map_err(to_io)?
        .create()
        .map_err(to_io)?;

    for path in read_paths {
        let fd = PathFd::new(path).map_err(to_io)?;
        ruleset = ruleset
            .add_rule(PathBeneath::new(fd, access_read))
            .map_err(to_io)?;
    }

    for path in write_paths {
        let fd = PathFd::new(path).map_err(to_io)?;
        ruleset = ruleset
            .add_rule(PathBeneath::new(fd, access_write))
            .map_err(to_io)?;
    }

    for system_path in ["/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc", "/dev", "/proc"] {
        if std::path::Path::new(system_path).exists() {
            let fd = PathFd::new(system_path).map_err(to_io)?;
            ruleset = ruleset
                .add_rule(PathBeneath::new(fd, access_read))
                .map_err(to_io)?;
        }
    }

    let status = ruleset.restrict_self().map_err(to_io)?;
    if status.ruleset != RulesetStatus::FullyEnforced {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("landlock not fully enforced: {:?}", status.ruleset),
        ));
    }
    Ok(())
}

fn to_io<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::PermissionDenied, e.to_string())
}

// ─── close_range ─────────────────────────────────────────

fn apply_close_range() -> Result<(), std::io::Error> {
    const CLOSE_RANGE_CLOEXEC: libc::c_uint = 1 << 2;
    let ret = unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, CLOSE_RANGE_CLOEXEC) };
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
