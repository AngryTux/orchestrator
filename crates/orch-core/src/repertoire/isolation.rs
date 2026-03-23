use serde::{Deserialize, Serialize};

use super::SpecMetadata;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsolationProfileSpec {
    pub kind: String,
    pub version: u32,
    pub metadata: SpecMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespaces: Option<NamespaceConfig>,
    #[serde(default)]
    pub mounts: Vec<MountConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landlock: Option<LandlockConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seccomp: Option<SeccompConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cgroup: Option<CgroupConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<EnvironmentConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawn: Option<SpawnConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lsm: Option<LsmConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceConfig {
    #[serde(default)]
    pub user: bool,
    #[serde(default)]
    pub pid: bool,
    #[serde(default)]
    pub mount: bool,
    #[serde(default)]
    pub network: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountConfig {
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub readonly: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandlockConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filesystem: Option<LandlockFilesystem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<LandlockNetwork>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipc: Option<LandlockScope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<LandlockScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandlockFilesystem {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub read: Vec<String>,
    #[serde(default)]
    pub write: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandlockNetwork {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub tcp_connect: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_bind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandlockScope {
    #[serde(default)]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeccompConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_profile")]
    pub profile: String,
}

fn default_profile() -> String {
    "default".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CgroupConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pids: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentConfig {
    #[serde(default)]
    pub clean: bool,
    #[serde(default)]
    pub credential_inject: bool,
    #[serde(default)]
    pub allowed_vars: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnConfig {
    #[serde(default)]
    pub close_range: bool,
    #[serde(default)]
    pub tmpdir_isolated: bool,
    #[serde(default)]
    pub openat2_beneath: bool,
    #[serde(default)]
    pub dns_preresolve: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LsmConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apparmor: Option<LsmModuleConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selinux: Option<LsmModuleConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LsmModuleConfig {
    #[serde(default = "default_auto")]
    pub enabled: String,
}

fn default_auto() -> String {
    "auto".into()
}
