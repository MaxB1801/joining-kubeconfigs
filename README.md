# kconf - Kubernetes Config Merger

A CLI tool to merge multiple kubeconfig files into a single unified configuration.

## Installation

### From Source

```bash
cargo build --release
# Binary will be at target/release/kconf
```

### Pre-built Binaries

Pre-built binaries are available for:
- Linux (x86_64)
- macOS (x86_64/arm64)

## Usage

```bash
kconf <config1> [config2] [config3] ...
```

### Examples

Merge a single kubeconfig:
```bash
kconf ~/Downloads/new-cluster-config.yaml
```

Merge multiple kubeconfigs:
```bash
kconf cluster1.yaml cluster2.yaml cluster3.yaml
```

## Configuration

kconf stores its configuration in `~/.k8sconf/config.yaml`. This file is created automatically on first run with default settings.

### Configuration Options

```yaml
# Destination kubeconfig file path
destination: ~/.kube/config
```

You can modify this file to change where merged configs are written.

## Features

- **Merge multiple kubeconfigs**: Combine any number of kubeconfig files into one
- **Duplicate detection**: Prevents accidentally adding duplicate clusters, contexts, or users
- **Automatic config creation**: Creates the destination config if it doesn't exist
- **Configurable destination**: Set your preferred output location via `~/.k8sconf/config.yaml`

## Error Handling

kconf will error and refuse to merge if:

- A cluster with the same name already exists in the destination
- A context with the same name already exists in the destination
- A user with the same name already exists in the destination
- The source kubeconfig file doesn't exist or is invalid

## Directory Structure

```
~/.k8sconf/
  config.yaml      # Application configuration
~/.kube/
  config           # Default destination for merged kubeconfigs
```

## Building for Multiple Platforms

### Linux
```bash
cargo build --release --target x86_64-unknown-linux-gnu
```

### macOS (Intel)
```bash
cargo build --release --target x86_64-apple-darwin
```

### macOS (Apple Silicon)
```bash
cargo build --release --target aarch64-apple-darwin
```

## Testing

Run the test suite:
```bash
cargo test
```

## License

MIT
