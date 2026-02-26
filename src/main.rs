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
    configs: Vec<PathBuf>,

    /// Remove a context (and its associated cluster/user) from the destination config
    #[arg(long)]
    remove: Option<String>,

    /// Update existing contexts instead of skipping them
    #[arg(long)]
    update: bool,
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
    #[serde(
        rename = "certificate-authority-data",
        skip_serializing_if = "Option::is_none"
    )]
    certificate_authority_data: Option<String>,
    #[serde(
        rename = "certificate-authority",
        skip_serializing_if = "Option::is_none"
    )]
    certificate_authority: Option<String>,
    #[serde(
        rename = "insecure-skip-tls-verify",
        skip_serializing_if = "Option::is_none"
    )]
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
    #[serde(
        rename = "client-certificate-data",
        skip_serializing_if = "Option::is_none"
    )]
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
        let config: AppConfig =
            serde_yaml::from_str(&content).with_context(|| "Failed to parse config file")?;
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

/// Remove a context and its associated cluster and user from the kubeconfig.
/// Returns the number of items removed.
fn remove_context(config: &mut KubeConfig, context_name: &str) -> usize {
    let mut removed = 0;

    // Find the context to get the associated cluster and user names
    let context_info = config
        .contexts
        .iter()
        .find(|c| c.name == context_name)
        .map(|c| (c.context.cluster.clone(), c.context.user.clone()));

    // Remove the context
    let before = config.contexts.len();
    config.contexts.retain(|c| c.name != context_name);
    removed += before - config.contexts.len();

    if let Some((cluster_name, user_name)) = context_info {
        // Only remove the cluster if no other context references it
        let cluster_still_used = config
            .contexts
            .iter()
            .any(|c| c.context.cluster == cluster_name);
        if !cluster_still_used {
            let before = config.clusters.len();
            config.clusters.retain(|c| c.name != cluster_name);
            removed += before - config.clusters.len();
        }

        // Only remove the user if no other context references it
        let user_still_used = config.contexts.iter().any(|c| c.context.user == user_name);
        if !user_still_used {
            let before = config.users.len();
            config.users.retain(|u| u.name != user_name);
            removed += before - config.users.len();
        }
    }

    // Clear current-context if it was the removed context
    if config.current_context.as_deref() == Some(context_name) {
        config.current_context = None;
    }

    removed
}

/// Result of checking for duplicates - contains lists of what can be merged
struct MergeResult {
    clusters_to_add: Vec<NamedCluster>,
    contexts_to_add: Vec<NamedContext>,
    users_to_add: Vec<NamedUser>,
    clusters_to_update: Vec<NamedCluster>,
    contexts_to_update: Vec<NamedContext>,
    users_to_update: Vec<NamedUser>,
    skipped_clusters: Vec<String>,
    skipped_contexts: Vec<String>,
    skipped_users: Vec<String>,
}

fn filter_duplicates(dest: &KubeConfig, source: KubeConfig, update: bool) -> MergeResult {
    let mut result = MergeResult {
        clusters_to_add: Vec::new(),
        contexts_to_add: Vec::new(),
        users_to_add: Vec::new(),
        clusters_to_update: Vec::new(),
        contexts_to_update: Vec::new(),
        users_to_update: Vec::new(),
        skipped_clusters: Vec::new(),
        skipped_contexts: Vec::new(),
        skipped_users: Vec::new(),
    };

    // Filter clusters
    for cluster in source.clusters {
        if dest.clusters.iter().any(|c| c.name == cluster.name) {
            if update {
                result.clusters_to_update.push(cluster);
            } else {
                result.skipped_clusters.push(cluster.name.clone());
            }
        } else {
            result.clusters_to_add.push(cluster);
        }
    }

    // Filter contexts
    for context in source.contexts {
        if dest.contexts.iter().any(|c| c.name == context.name) {
            if update {
                result.contexts_to_update.push(context);
            } else {
                result.skipped_contexts.push(context.name.clone());
            }
        } else {
            result.contexts_to_add.push(context);
        }
    }

    // Filter users
    for user in source.users {
        if dest.users.iter().any(|u| u.name == user.name) {
            if update {
                result.users_to_update.push(user);
            } else {
                result.skipped_users.push(user.name.clone());
            }
        } else {
            result.users_to_add.push(user);
        }
    }

    result
}

fn merge_kubeconfigs(
    dest: &mut KubeConfig,
    merge_result: MergeResult,
    source_current_context: Option<String>,
) -> (usize, usize, usize) {
    let added = merge_result.clusters_to_add.len()
        + merge_result.contexts_to_add.len()
        + merge_result.users_to_add.len();
    let updated = merge_result.clusters_to_update.len()
        + merge_result.contexts_to_update.len()
        + merge_result.users_to_update.len();
    let skipped = merge_result.skipped_clusters.len()
        + merge_result.skipped_contexts.len()
        + merge_result.skipped_users.len();

    // Add new items
    dest.clusters.extend(merge_result.clusters_to_add);
    dest.contexts.extend(merge_result.contexts_to_add);
    dest.users.extend(merge_result.users_to_add);

    // Update existing items in place
    for updated_cluster in merge_result.clusters_to_update {
        if let Some(existing) = dest
            .clusters
            .iter_mut()
            .find(|c| c.name == updated_cluster.name)
        {
            *existing = updated_cluster;
        }
    }
    for updated_context in merge_result.contexts_to_update {
        if let Some(existing) = dest
            .contexts
            .iter_mut()
            .find(|c| c.name == updated_context.name)
        {
            *existing = updated_context;
        }
    }
    for updated_user in merge_result.users_to_update {
        if let Some(existing) = dest.users.iter_mut().find(|u| u.name == updated_user.name) {
            *existing = updated_user;
        }
    }

    // Set current-context if destination doesn't have one
    if dest.current_context.is_none() && source_current_context.is_some() {
        dest.current_context = source_current_context;
    }

    (added, updated, skipped)
}

fn run() -> Result<()> {
    let args = Args::parse();

    // Validate: at least one of configs or --remove must be provided
    if args.configs.is_empty() && args.remove.is_none() {
        anyhow::bail!(
            "Either provide kubeconfig files to merge or use --remove to remove a context"
        );
    }

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

    // Handle --remove flag
    if let Some(ref context_name) = args.remove {
        let removed = remove_context(&mut dest_config, context_name);
        if removed > 0 {
            println!(
                "Removed context '{}' and {} associated item(s)",
                context_name,
                removed - 1
            );
        } else {
            println!("Context '{}' not found in destination config", context_name);
        }
    }

    let mut total_added = 0;
    let mut total_updated = 0;
    let mut total_skipped = 0;

    // Process each source kubeconfig
    for config_path in &args.configs {
        println!("Processing: {:?}", config_path);

        let source_config = load_kubeconfig(config_path)?;
        let source_current_context = source_config.current_context.clone();

        // Filter out duplicates and get what can be merged
        let merge_result = filter_duplicates(&dest_config, source_config, args.update);

        // Report skipped items
        for name in &merge_result.skipped_clusters {
            println!("  Skipping cluster '{}' (already exists)", name);
        }
        for name in &merge_result.skipped_contexts {
            println!("  Skipping context '{}' (already exists)", name);
        }
        for name in &merge_result.skipped_users {
            println!("  Skipping user '{}' (already exists)", name);
        }

        // Report updated items
        for name in &merge_result.clusters_to_update {
            println!("  Updating cluster '{}'", name.name);
        }
        for name in &merge_result.contexts_to_update {
            println!("  Updating context '{}'", name.name);
        }
        for name in &merge_result.users_to_update {
            println!("  Updating user '{}'", name.name);
        }

        // Merge configs
        let (added, updated, skipped) =
            merge_kubeconfigs(&mut dest_config, merge_result, source_current_context);
        total_added += added;
        total_updated += updated;
        total_skipped += skipped;

        if added > 0 {
            println!("  Merged {} item(s)", added);
        }
        if updated > 0 {
            println!("  Updated {} item(s)", updated);
        }
        if skipped > 0 && added == 0 && updated == 0 {
            println!("  Nothing new to merge");
        }
    }

    // Write the merged config
    let output = serde_yaml::to_string(&dest_config)?;
    fs::write(&dest_path, &output)
        .with_context(|| format!("Failed to write destination config: {:?}", dest_path))?;

    println!(
        "Done: {} item(s) added, {} item(s) updated, {} item(s) skipped",
        total_added, total_updated, total_skipped
    );

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
        let source_ctx = source.current_context.clone();

        let merge_result = filter_duplicates(&dest, source, false);
        merge_kubeconfigs(&mut dest, merge_result, source_ctx);

        assert_eq!(dest.clusters.len(), 1);
        assert_eq!(dest.contexts.len(), 1);
        assert_eq!(dest.users.len(), 1);
        assert_eq!(dest.clusters[0].name, "test1-cluster");
    }

    #[test]
    fn test_filter_duplicates_no_duplicates() {
        let dest = create_test_kubeconfig("dest");
        let source = create_test_kubeconfig("source");

        let result = filter_duplicates(&dest, source, false);
        assert_eq!(result.clusters_to_add.len(), 1);
        assert_eq!(result.skipped_clusters.len(), 0);
    }

    #[test]
    fn test_filter_duplicates_with_duplicates() {
        let dest = create_test_kubeconfig("test");
        let source = create_test_kubeconfig("test");

        let result = filter_duplicates(&dest, source, false);
        assert_eq!(result.clusters_to_add.len(), 0);
        assert_eq!(result.skipped_clusters.len(), 1);
        assert_eq!(result.skipped_clusters[0], "test-cluster");
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
        let ctx1 = source1.current_context.clone();
        let ctx2 = source2.current_context.clone();

        let merge_result1 = filter_duplicates(&dest, source1, false);
        merge_kubeconfigs(&mut dest, merge_result1, ctx1);

        let merge_result2 = filter_duplicates(&dest, source2, false);
        merge_kubeconfigs(&mut dest, merge_result2, ctx2);

        assert_eq!(dest.clusters.len(), 2);
        assert_eq!(dest.contexts.len(), 2);
        assert_eq!(dest.users.len(), 2);
    }

    #[test]
    fn test_skip_duplicates_and_merge_new() {
        let mut dest = create_test_kubeconfig("existing");

        // Create a source with one existing and one new cluster
        let mut source = create_test_kubeconfig("existing");
        let new_cluster = NamedCluster {
            name: "new-cluster".to_string(),
            cluster: ClusterInfo {
                server: "https://new.example.com:6443".to_string(),
                certificate_authority_data: Some("bmV3LWNh".to_string()),
                certificate_authority: None,
                insecure_skip_tls_verify: None,
            },
        };
        let new_context = NamedContext {
            name: "new-context".to_string(),
            context: ContextInfo {
                cluster: "new-cluster".to_string(),
                user: "new-user".to_string(),
                namespace: None,
            },
        };
        let new_user = NamedUser {
            name: "new-user".to_string(),
            user: UserInfo {
                client_certificate_data: None,
                client_key_data: None,
                client_certificate: None,
                client_key: None,
                token: Some("new-token".to_string()),
                username: None,
                password: None,
            },
        };
        source.clusters.push(new_cluster);
        source.contexts.push(new_context);
        source.users.push(new_user);

        let merge_result = filter_duplicates(&dest, source, false);

        // Should skip the existing ones
        assert_eq!(merge_result.skipped_clusters.len(), 1);
        assert_eq!(merge_result.skipped_contexts.len(), 1);
        assert_eq!(merge_result.skipped_users.len(), 1);

        // Should add the new ones
        assert_eq!(merge_result.clusters_to_add.len(), 1);
        assert_eq!(merge_result.contexts_to_add.len(), 1);
        assert_eq!(merge_result.users_to_add.len(), 1);

        let (added, _updated, skipped) = merge_kubeconfigs(&mut dest, merge_result, None);
        assert_eq!(added, 3);
        assert_eq!(skipped, 3);

        // Dest should now have 2 of each
        assert_eq!(dest.clusters.len(), 2);
        assert_eq!(dest.contexts.len(), 2);
        assert_eq!(dest.users.len(), 2);
    }

    #[test]
    fn test_remove_context() {
        let mut config = create_test_kubeconfig("test");
        assert_eq!(config.contexts.len(), 1);
        assert_eq!(config.clusters.len(), 1);
        assert_eq!(config.users.len(), 1);
        assert_eq!(config.current_context, Some("test-context".to_string()));

        let removed = remove_context(&mut config, "test-context");
        assert_eq!(removed, 3); // context + cluster + user
        assert_eq!(config.contexts.len(), 0);
        assert_eq!(config.clusters.len(), 0);
        assert_eq!(config.users.len(), 0);
        assert_eq!(config.current_context, None);
    }

    #[test]
    fn test_remove_context_not_found() {
        let mut config = create_test_kubeconfig("test");
        let removed = remove_context(&mut config, "nonexistent-context");
        assert_eq!(removed, 0);
        assert_eq!(config.contexts.len(), 1);
        assert_eq!(config.clusters.len(), 1);
        assert_eq!(config.users.len(), 1);
    }

    #[test]
    fn test_remove_context_preserves_shared_cluster() {
        let mut config = create_test_kubeconfig("test");
        // Add a second context that shares the same cluster
        config.contexts.push(NamedContext {
            name: "other-context".to_string(),
            context: ContextInfo {
                cluster: "test-cluster".to_string(),
                user: "other-user".to_string(),
                namespace: None,
            },
        });
        config.users.push(NamedUser {
            name: "other-user".to_string(),
            user: UserInfo {
                client_certificate_data: None,
                client_key_data: None,
                client_certificate: None,
                client_key: None,
                token: Some("other-token".to_string()),
                username: None,
                password: None,
            },
        });

        let removed = remove_context(&mut config, "test-context");
        // Should remove context + user, but NOT the shared cluster
        assert_eq!(removed, 2);
        assert_eq!(config.contexts.len(), 1);
        assert_eq!(config.clusters.len(), 1); // cluster preserved
        assert_eq!(config.users.len(), 1); // only other-user remains
        assert_eq!(config.contexts[0].name, "other-context");
    }

    #[test]
    fn test_update_duplicates() {
        let mut dest = create_test_kubeconfig("test");
        // Source has same names but different server URL
        let mut source = create_test_kubeconfig("test");
        source.clusters[0].cluster.server = "https://updated.example.com:6443".to_string();
        source.users[0].user.token = Some("updated-token".to_string());

        let merge_result = filter_duplicates(&dest, source, true);

        // With update=true, duplicates go to update lists, not skip lists
        assert_eq!(merge_result.clusters_to_update.len(), 1);
        assert_eq!(merge_result.contexts_to_update.len(), 1);
        assert_eq!(merge_result.users_to_update.len(), 1);
        assert_eq!(merge_result.skipped_clusters.len(), 0);
        assert_eq!(merge_result.clusters_to_add.len(), 0);

        let (added, updated, skipped) = merge_kubeconfigs(&mut dest, merge_result, None);
        assert_eq!(added, 0);
        assert_eq!(updated, 3);
        assert_eq!(skipped, 0);

        // Dest should still have 1 of each, but with updated values
        assert_eq!(dest.clusters.len(), 1);
        assert_eq!(
            dest.clusters[0].cluster.server,
            "https://updated.example.com:6443"
        );
        assert_eq!(dest.users[0].user.token, Some("updated-token".to_string()));
    }
}
