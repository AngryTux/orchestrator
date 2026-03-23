use orch_core::repertoire::*;
use std::path::PathBuf;

// ---- Scene 3.1: Provider spec parsing ----

const CLAUDE_YAML: &str = r#"
kind: Provider
version: 1
metadata:
  name: claude
  display_name: "Claude (Anthropic)"
  url: "https://docs.anthropic.com/en/docs/claude-code"
detection:
  binary: claude
  version_cmd: ["claude", "--version"]
  auth_paths: ["~/.claude/"]
invocation:
  cmd: ["claude", "--print", "--tools", ""]
  prompt_flag: "-p"
  model_flag: "--model"
  system_prompt_flag: "--system-prompt"
  json_schema_flag: "--json-schema"
  output_format_flag: ["--output-format", "json"]
  extra_flags: ["--no-session-persistence"]
auth:
  env_var: "ANTHROPIC_API_KEY"
  methods: [detect, env, input]
install:
  hint: "https://docs.anthropic.com/en/docs/claude-code"
  commands:
    linux: ["curl -fsSL https://docs.anthropic.com/install.sh | sh"]
"#;

#[test]
fn parse_provider_spec() {
    let spec: ProviderSpec = serde_yaml::from_str(CLAUDE_YAML).unwrap();
    assert_eq!(spec.kind, "Provider");
    assert_eq!(spec.version, 1);
    assert_eq!(spec.metadata.name, "claude");
    assert_eq!(spec.metadata.display_name.as_deref(), Some("Claude (Anthropic)"));
    assert_eq!(spec.detection.binary, "claude");
    assert_eq!(spec.invocation.prompt_flag, "-p");
    assert_eq!(spec.auth.env_var, "ANTHROPIC_API_KEY");
    assert_eq!(spec.auth.methods, vec!["detect", "env", "input"]);
}

#[test]
fn provider_spec_rejects_missing_binary() {
    let yaml = r#"
kind: Provider
version: 1
metadata:
  name: broken
detection:
  version_cmd: ["broken", "--version"]
invocation:
  cmd: ["broken"]
  prompt_flag: "-p"
auth:
  env_var: "BROKEN_KEY"
  methods: [env]
"#;
    let result = serde_yaml::from_str::<ProviderSpec>(yaml);
    assert!(result.is_err());
}

// ---- Scene 3.2: Integration spec parsing ----

const ARRANGER_YAML: &str = r#"
kind: Integration
version: 1
metadata:
  name: arranger
  description: "Intent intake and classification"
role: arranger
provider:
  default: claude
  model: haiku
phases:
  - name: intake
    system_prompt: |
      You are an intake analyst. Classify the user's intent.
    json_schema:
      type: object
      properties:
        approved:
          type: boolean
        classification:
          type: string
      required: [approved, classification]
"#;

const MAESTRO_YAML: &str = r#"
kind: Integration
version: 1
metadata:
  name: maestro
  description: "Composition and consolidation"
role: maestro
provider:
  default: claude
  model: opus
phases:
  - name: compose
    system_prompt: "Break down the request into sections."
  - name: consolidate
    system_prompt: "Merge multiple outputs into a coherent answer."
"#;

#[test]
fn parse_arranger_integration() {
    let spec: IntegrationSpec = serde_yaml::from_str(ARRANGER_YAML).unwrap();
    assert_eq!(spec.metadata.name, "arranger");
    assert_eq!(spec.role, IntegrationRole::Arranger);
    assert_eq!(spec.provider.model, "haiku");
    assert_eq!(spec.phases.len(), 1);
    assert_eq!(spec.phases[0].name, "intake");
    assert!(spec.phases[0].json_schema.is_some());
}

#[test]
fn parse_maestro_integration() {
    let spec: IntegrationSpec = serde_yaml::from_str(MAESTRO_YAML).unwrap();
    assert_eq!(spec.role, IntegrationRole::Maestro);
    assert_eq!(spec.phases.len(), 2);
    assert_eq!(spec.phases[0].name, "compose");
    assert_eq!(spec.phases[1].name, "consolidate");
}

// ---- Scene 3.3: Formation spec parsing ----

const SOLO_YAML: &str = r#"
kind: Formation
version: 1
metadata:
  name: solo
  description: "Single workspace, single musician"
min_sections: 1
max_sections: 1
parallel: false
consolidation: passthrough
"#;

const DUET_YAML: &str = r#"
kind: Formation
version: 1
metadata:
  name: duet
  description: "Two workspaces in parallel, consolidated"
min_sections: 2
max_sections: 2
parallel: true
consolidation: required
"#;

#[test]
fn parse_solo_formation() {
    let spec: FormationSpec = serde_yaml::from_str(SOLO_YAML).unwrap();
    assert_eq!(spec.metadata.name, "solo");
    assert_eq!(spec.min_sections, 1);
    assert_eq!(spec.max_sections, 1);
    assert!(!spec.parallel);
    assert_eq!(spec.consolidation, ConsolidationType::Passthrough);
}

#[test]
fn parse_duet_formation() {
    let spec: FormationSpec = serde_yaml::from_str(DUET_YAML).unwrap();
    assert_eq!(spec.metadata.name, "duet");
    assert!(spec.parallel);
    assert_eq!(spec.consolidation, ConsolidationType::Required);
}

// ---- Scene 3.4: Isolation profile parsing ----

const SANDBOX_YAML: &str = r#"
kind: IsolationProfile
version: 1
metadata:
  name: sandbox
  description: "Standard isolation for daily use"
  risk: low
namespaces:
  user: true
  pid: true
  mount: true
  network: false
mounts:
  - source: "${provider_binary}"
    target: "/usr/local/bin/${provider_name}"
    readonly: true
  - source: "${workspace_dir}"
    target: "/workspace"
    readonly: true
landlock:
  filesystem:
    enabled: true
    read: ["${workspace_dir}"]
    write: ["${tmpdir}"]
  network:
    enabled: true
    tcp_connect: ["${provider_endpoints}"]
    tcp_bind: deny
  ipc:
    enabled: true
    scope: isolated
  signal:
    enabled: true
    scope: isolated
seccomp:
  enabled: true
  profile: default
cgroup:
  enabled: true
  cpu: "1.0"
  memory: "512M"
  pids: 10
environment:
  clean: true
  credential_inject: true
  allowed_vars: ["PATH", "HOME", "TERM"]
spawn:
  close_range: true
  tmpdir_isolated: true
  openat2_beneath: true
  dns_preresolve: true
lsm:
  apparmor:
    enabled: auto
  selinux:
    enabled: auto
"#;

#[test]
fn parse_sandbox_isolation_profile() {
    let spec: IsolationProfileSpec = serde_yaml::from_str(SANDBOX_YAML).unwrap();
    assert_eq!(spec.metadata.name, "sandbox");
    assert_eq!(spec.metadata.risk.as_deref(), Some("low"));
    assert!(spec.extends.is_none());

    let ns = spec.namespaces.unwrap();
    assert!(ns.user);
    assert!(ns.pid);
    assert!(!ns.network);

    assert_eq!(spec.mounts.len(), 2);
    assert!(spec.mounts[0].readonly);

    let ll = spec.landlock.unwrap();
    assert!(ll.filesystem.unwrap().enabled);
    assert_eq!(ll.network.as_ref().unwrap().tcp_bind.as_deref(), Some("deny"));

    let cg = spec.cgroup.unwrap();
    assert_eq!(cg.memory.as_deref(), Some("512M"));
    assert_eq!(cg.pids, Some(10));

    let env = spec.environment.unwrap();
    assert!(env.clean);
    assert!(env.credential_inject);
    assert_eq!(env.allowed_vars, vec!["PATH", "HOME", "TERM"]);

    let spawn = spec.spawn.unwrap();
    assert!(spawn.close_range);
    assert!(spawn.dns_preresolve);
}

#[test]
fn parse_isolation_profile_with_extends() {
    let yaml = r#"
kind: IsolationProfile
version: 1
metadata:
  name: my-custom
extends: sandbox
landlock:
  network:
    enabled: true
    tcp_connect:
      - "${provider_endpoints}"
      - "internal-api.company.com:443"
"#;
    let spec: IsolationProfileSpec = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(spec.extends.as_deref(), Some("sandbox"));
    assert!(spec.namespaces.is_none()); // inherited from parent
}

// ---- Scene 3.5: Resolution ----

#[test]
fn resolve_provider_from_repertoire() {
    let dir = tempdir_with_spec("repertoire/providers", "claude.yaml", CLAUDE_YAML);
    let repertoire = Repertoire::new(dir.join("custom"), dir.join("repertoire"));

    let spec = repertoire.load_provider("claude").unwrap();
    assert_eq!(spec.metadata.name, "claude");
}

#[test]
fn resolve_provider_user_custom_takes_priority() {
    let custom_yaml = r#"
kind: Provider
version: 1
metadata:
  name: claude
  display_name: "My Custom Claude"
detection:
  binary: claude
  version_cmd: ["claude", "--version"]
invocation:
  cmd: ["claude"]
  prompt_flag: "-p"
auth:
  env_var: "MY_KEY"
  methods: [env]
"#;
    let dir = tempdir_with_spec("repertoire/providers", "claude.yaml", CLAUDE_YAML);
    write_spec(&dir, "custom/providers", "claude.yaml", custom_yaml);
    let repertoire = Repertoire::new(dir.join("custom"), dir.join("repertoire"));

    let spec = repertoire.load_provider("claude").unwrap();
    assert_eq!(spec.metadata.display_name.as_deref(), Some("My Custom Claude"));
}

#[test]
fn resolve_provider_not_found() {
    let dir = std::env::temp_dir().join(format!("orch-rep-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let repertoire = Repertoire::new(dir.join("custom"), dir.join("repertoire"));

    let result = repertoire.load_provider("nonexistent");
    assert!(result.is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn resolve_formation_from_repertoire() {
    let dir = tempdir_with_spec("repertoire/formations", "solo.yaml", SOLO_YAML);
    let repertoire = Repertoire::new(dir.join("custom"), dir.join("repertoire"));

    let spec = repertoire.load_formation("solo").unwrap();
    assert_eq!(spec.metadata.name, "solo");
}

#[test]
fn resolve_isolation_from_repertoire() {
    let dir = tempdir_with_spec("repertoire/isolation", "sandbox.yaml", SANDBOX_YAML);
    let repertoire = Repertoire::new(dir.join("custom"), dir.join("repertoire"));

    let spec = repertoire.load_isolation("sandbox").unwrap();
    assert_eq!(spec.metadata.name, "sandbox");
}

#[test]
fn resolve_integration_from_repertoire() {
    let dir = tempdir_with_spec("repertoire/integrations", "arranger.yaml", ARRANGER_YAML);
    let repertoire = Repertoire::new(dir.join("custom"), dir.join("repertoire"));

    let spec = repertoire.load_integration("arranger").unwrap();
    assert_eq!(spec.metadata.name, "arranger");
}

// ---- Helpers ----

fn tempdir_with_spec(subdir: &str, filename: &str, content: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "orch-rep-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    write_spec(&base, subdir, filename, content);
    base
}

fn write_spec(base: &std::path::Path, subdir: &str, filename: &str, content: &str) {
    let dir = base.join(subdir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(filename), content).unwrap();
}
