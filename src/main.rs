use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

/// CLI tool to join kubeconfig files together
#[derive(Parser, Debug)]
#[command(name = "kconf")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Kubeconfig files to merge into the destination config
    #[arg(required = true)]
    configs: Vec<PathBuf>,
}

/// Application configuration stored in ~/.k8sconf/config.yaml
#[derive(Debug, Serialize, Deserialize)]
struct AppConfig {
    /// Destination kubeconfig file path
    destination: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            destination: "~/.kube/config".to_string(),
        }
    }
}

/// Kubeconfig structure
#[derive(Debug, Serialize, Deserialize, Clone)]
struct KubeConfig {
    #[serde(rename = "apiVersion")]
    api_version: String,
    kind: String,
    clusters: Vec<NamedCluster>,
    contexts: Vec<NamedContext>,
    users: Vec<NamedUser>,
    #[serde(rename = "current-context", skip_serializing_if = "Option::is_none")]
    current_context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    preferences: Option<HashMap<String, serde_yaml::Value>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct NamedCluster {
    name: String,
    cluster: ClusterInfo,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct ClusterInfo {
    server: String,
    #[serde(rename = "certificate-authority-data", skip_serializing_if = "Option::is_none")]
    certificate_authority_data: Option<String>,
    #[serde(rename = "certificate-authority", skip_serializing_if = "Option::is_none")]
    certificate_authority: Option<String>,
    #[serde(rename = "insecure-skip-tls-verify", skip_serializing_if = "Option::is_none")]
    insecure_skip_tls_verify: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct NamedContext {
    name: String,
    context: ContextInfo,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct ContextInfo {
    cluster: String,
    user: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    namespace: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct NamedUser {
    name: String,
    user: UserInfo,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct UserInfo {
    #[serde(rename = "client-certificate-data", skip_serializing_if = "Option::is_none")]
    client_certificate_data: Option<String>,
    #[serde(rename = "client-key-data", skip_serializing_if = "Option::is_none")]
    client_key_data: Option<String>,
    #[serde(rename = "client-certificate", skip_serializing_if = "Option::is_none")]
    client_certificate: Option<String>,
    #[serde(rename = "client-key", skip_serializing_if = "Option::is_none")]
    client_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    password: Option<String>,
}

#[derive(Error, Debug)]
enum KconfError {
    #[error("Cluster '{0}' already exists in the destination config")]
    DuplicateCluster(String),
    #[error("Context '{0}' already exists in the destination config")]
    DuplicateContext(String),
    #[error("User '{0}' already exists in the destination config")]
    DuplicateUser(String),
    #[error("Kubeconfig file not found: {0}")]
    ConfigNotFound(PathBuf),
}

fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(&path[2..]);
        }
    }
    PathBuf::from(path)
}

fn get_app_config_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home.join(".k8sconf"))
}

fn load_app_config() -> Result<AppConfig> {
    let config_dir = get_app_config_dir()?;
    let config_path = config_dir.join("config.yaml");

    if config_path.exists() {
        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config file: {:?}", config_path))?;
        let config: AppConfig = serde_yaml::from_str(&content)
            .with_context(|| "Failed to parse config file")?;
        Ok(config)
    } else {
        // Create default config
        fs::create_dir_all(&config_dir)
            .with_context(|| format!("Failed to create config directory: {:?}", config_dir))?;
        let config = AppConfig::default();
        let content = serde_yaml::to_string(&config)?;
        fs::write(&config_path, &content)
            .with_context(|| format!("Failed to write default config: {:?}", config_path))?;
        Ok(config)
    }
}

fn load_kubeconfig(path: &PathBuf) -> Result<KubeConfig> {
    if !path.exists() {
        return Err(KconfError::ConfigNotFound(path.clone()).into());
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read kubeconfig: {:?}", path))?;
    let config: KubeConfig = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse kubeconfig: {:?}", path))?;
    Ok(config)
}

fn create_empty_kubeconfig() -> KubeConfig {
    KubeConfig {
        api_version: "v1".to_string(),
        kind: "Config".to_string(),
        clusters: Vec::new(),
        contexts: Vec::new(),
        users: Vec::new(),
        current_context: None,
        preferences: Some(HashMap::new()),
    }
}

fn check_duplicates(dest: &KubeConfig, source: &KubeConfig) -> Result<()> {
    // Check for duplicate clusters
    for source_cluster in &source.clusters {
        if dest.clusters.iter().any(|c| c.name == source_cluster.name) {
            return Err(KconfError::DuplicateCluster(source_cluster.name.clone()).into());
        }
    }

    // Check for duplicate contexts
    for source_context in &source.contexts {
        if dest.contexts.iter().any(|c| c.name == source_context.name) {
            return Err(KconfError::DuplicateContext(source_context.name.clone()).into());
        }
    }

    // Check for duplicate users
    for source_user in &source.users {
        if dest.users.iter().any(|u| u.name == source_user.name) {
            return Err(KconfError::DuplicateUser(source_user.name.clone()).into());
        }
    }

    Ok(())
}

fn merge_kubeconfigs(dest: &mut KubeConfig, source: KubeConfig) {
    dest.clusters.extend(source.clusters);
    dest.contexts.extend(source.contexts);
    dest.users.extend(source.users);

    // Set current-context if destination doesn't have one
    if dest.current_context.is_none() && source.current_context.is_some() {
        dest.current_context = source.current_context;
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    
    // Load application config
    let app_config = load_app_config()?;
    let dest_path = expand_tilde(&app_config.destination);

    println!("Destination kubeconfig: {:?}", dest_path);

    // Load or create destination kubeconfig
    let mut dest_config = if dest_path.exists() {
        load_kubeconfig(&dest_path)?
    } else {
        // Ensure parent directory exists
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {:?}", parent))?;
        }
        create_empty_kubeconfig()
    };

    // Process each source kubeconfig
    for config_path in &args.configs {
        println!("Processing: {:?}", config_path);
        
        let source_config = load_kubeconfig(config_path)?;
        
        // Check for duplicates before merging
        check_duplicates(&dest_config, &source_config)?;
        
        // Merge configs
        merge_kubeconfigs(&mut dest_config, source_config);
        
        println!("  Merged successfully");
    }

    // Write the merged config
    let output = serde_yaml::to_string(&dest_config)?;
    fs::write(&dest_path, &output)
        .with_context(|| format!("Failed to write destination config: {:?}", dest_path))?;

    println!("Successfully merged {} config(s) into {:?}", args.configs.len(), dest_path);

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_kubeconfig(name: &str) -> KubeConfig {
        KubeConfig {
            api_version: "v1".to_string(),
            kind: "Config".to_string(),
            clusters: vec![NamedCluster {
                name: format!("{}-cluster", name),
                cluster: ClusterInfo {
                    server: format!("https://{}.example.com:6443", name),
                    certificate_authority_data: Some("dGVzdC1jYS1kYXRh".to_string()),
                    certificate_authority: None,
                    insecure_skip_tls_verify: None,
                },
            }],
            contexts: vec![NamedContext {
                name: format!("{}-context", name),
                context: ContextInfo {
                    cluster: format!("{}-cluster", name),
                    user: format!("{}-user", name),
                    namespace: None,
                },
            }],
            users: vec![NamedUser {
                name: format!("{}-user", name),
                user: UserInfo {
                    client_certificate_data: Some("dGVzdC1jZXJ0LWRhdGE=".to_string()),
                    client_key_data: Some("dGVzdC1rZXktZGF0YQ==".to_string()),
                    client_certificate: None,
                    client_key: None,
                    token: None,
                    username: None,
                    password: None,
                },
            }],
            current_context: Some(format!("{}-context", name)),
            preferences: Some(HashMap::new()),
        }
    }

    #[test]
    fn test_merge_kubeconfigs() {
        let mut dest = create_empty_kubeconfig();
        let source = create_test_kubeconfig("test1");

        merge_kubeconfigs(&mut dest, source.clone());

        assert_eq!(dest.clusters.len(), 1);
        assert_eq!(dest.contexts.len(), 1);
        assert_eq!(dest.users.len(), 1);
        assert_eq!(dest.clusters[0].name, "test1-cluster");
    }

    #[test]
    fn test_check_duplicates_no_duplicates() {
        let dest = create_test_kubeconfig("dest");
        let source = create_test_kubeconfig("source");

        let result = check_duplicates(&dest, &source);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_duplicates_cluster() {
        let dest = create_test_kubeconfig("test");
        let source = create_test_kubeconfig("test");

        let result = check_duplicates(&dest, &source);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Cluster"));
    }

    #[test]
    fn test_expand_tilde() {
        let expanded = expand_tilde("~/.kube/config");
        assert!(!expanded.to_string_lossy().starts_with("~"));
    }

    #[test]
    fn test_kubeconfig_serialization() {
        let config = create_test_kubeconfig("test");
        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: KubeConfig = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(parsed.clusters.len(), 1);
        assert_eq!(parsed.contexts.len(), 1);
        assert_eq!(parsed.users.len(), 1);
    }

    #[test]
    fn test_load_kubeconfig_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config");

        let config = create_test_kubeconfig("file-test");
        let yaml = serde_yaml::to_string(&config).unwrap();
        fs::write(&config_path, &yaml).unwrap();

        let loaded = load_kubeconfig(&config_path).unwrap();
        assert_eq!(loaded.clusters[0].name, "file-test-cluster");
    }

    #[test]
    fn test_merge_multiple_configs() {
        let mut dest = create_empty_kubeconfig();
        let source1 = create_test_kubeconfig("cluster1");
        let source2 = create_test_kubeconfig("cluster2");

        merge_kubeconfigs(&mut dest, source1);
        merge_kubeconfigs(&mut dest, source2);

        assert_eq!(dest.clusters.len(), 2);
        assert_eq!(dest.contexts.len(), 2);
        assert_eq!(dest.users.len(), 2);
    }
}
