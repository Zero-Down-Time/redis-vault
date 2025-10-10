# Redis Vault

A production-ready Rust application designed to run as a sidecar container alongside Redis instances, providing automated secure backups to S3 or Google Cloud Storage with configurable retention policies.

## Features

- **Non-intrusive backups**: Uses existing `dump.rdb` files without triggering BGSAVE
- **Multi-cloud support**: Backs up to AWS S3 or Google Cloud Storage
- **Role-aware backups**: Configurable to backup from masters, replicas, or both
- **Automatic retention management**: Cleanup old backups based on count and age
- **Kubernetes-ready**: Designed for sidecar deployment pattern
- **Comprehensive logging**: Structured JSON logging
- **Flexible configuration**: YAML config files or environment variables
- **Environment variable precedence**: Env vars override config file settings
- **Alpine-based**: Minimal Docker image (~13MB)

## Quick Start

### Building

```bash
# Build the binary
cargo build --release

# Build Docker image (uses Alpine Linux for minimal size)
docker build -t redis-vault:latest .

# Build with specific versions
docker build \
  --build-arg RUST_VERSION=1.90 \
  --build-arg ALPINE_VERSION=3.22 \
  -t redis-vault:latest .
```

### Running in Kubernetes

```bash
# Create namespace
kubectl create namespace redis

# Apply configurations
kubectl apply -f k8s/deployment.yaml
```

## Configuration

The application can be configured via environment variables or YAML file. **Environment variables take precedence over the configuration file.**

### Configuration Priority

1. **Environment Variables** (highest priority)
2. **Configuration File** (config.yaml)
3. **Default Values** (lowest priority)

### Configuration File (config.yaml)

```yaml
redis:
  connection_string: "redis://localhost:6379"
  data_path: "/data"
  node_name: "redis-node"
  backup_master: true
  backup_replica: true

backup:
  interval: "1h"
  dump_filename: "dump.rdb"
  initial_delay: 300s

storage:
  type: S3  # or GCS
  bucket: "redis-vault"
  prefix: "redis-vault"
  region: null
  endpoint: null

retention:
  keep_last: 7
  keep_duration: null

logging:
  format: "text"  # or "json"
  level: "info"   # trace, debug, info, warn, error
```

### Backup File Naming

Backup files are automatically named using the following structure:

```
{prefix}/{node_name}_{timestamp}.rdb
```

**Example filename:**
```
redis-vault/redis-master-01_20241201_143022.rdb
```

**Components:**
- `prefix`: Storage prefix from configuration (e.g., "redis-vault")
- `node_name`: Redis node identifier (e.g., "redis-master-01")
- `timestamp`: Backup creation time in YYYYMMDD_HHMMSS format
- `.rdb`: File extension

### Environment Variables

Environment variables **override** any values set in the configuration file. This allows for easy deployment-specific overrides.

| Variable | Description | Default |
|----------|-------------|---------|
| `REDIS_CONNECTION` | Redis connection string | `redis://localhost:6379` |
| `REDIS_DATA_PATH` | Path to Redis data directory | `/data` |
| `REDIS_NODE_NAME` | Unique name for this Redis node | `redis-node` |
| `BACKUP_MASTER` | Backup if node is master | `true` |
| `BACKUP_REPLICA` | Backup if node is replica | `true` |
| `BACKUP_INTERVAL` | Time between backup checks | `1h` |
| `INITIAL_DELAY` | Initial time to allow replication to setup | `60s` |
| `DUMP_FILENAME` | Redis dump filename | `dump.rdb` |
| `STORAGE_TYPE` | Storage backend (`s3` or `gcs`) | `s3` |
| `S3_BUCKET` | S3 bucket name | `redis-vault` |
| `S3_PREFIX` | S3 key prefix | `redis-vault` |
| `AWS_REGION` | AWS region | None |
| `S3_ENDPOINT` | Custom S3 endpoint (for MinIO, etc.) | None |
| `GCS_BUCKET` | GCS bucket name | Required for GCS |
| `GCS_PREFIX` | GCS object prefix | `redis-vault` |
| `GCS_PROJECT_ID` | GCP project ID | None |
| `RETENTION_KEEP_LAST` | Number of recent backups to keep | `7` |
| `RETENTION_KEEP_DURATION` | Keep backups newer than | None |
| `LOG_FORMAT` | Log format (`text` or `json`) | `text` |
| `LOG_LEVEL` | Application log level (`trace`, `debug`, `info`, `warn`, `error`) | `info` |
| `RUST_LOG` | Override all log levels (takes precedence over LOG_LEVEL) | None |
| `METRICS_ENABLED` | Enable Prometheus metrics endpoint | `false` |
| `METRICS_PORT` | Port for metrics server | `9090` |
| `METRICS_LISTEN_ADDRESS` | Listen address for metrics server | `0.0.0.0` |

### Configuration Override Example

```bash
# Base configuration in config.yaml sets bucket to "dev-backups"
# Override for production deployment:
export S3_BUCKET="prod-backups"
export RETENTION_KEEP_LAST="30"

# These environment variables will override the file configuration
redis-vault --config config.yaml
```

## Deployment Patterns

### Kubernetes StatefulSet Sidecar

- Shared volume between Redis and backup containers
- Automatic restart on failure
- Resource limits to prevent impact on Redis
- Backup container has read-only access to Redis data
- Runs as unprivileged user in container
- Use IAM roles instead of keys when possible

## License

AGPLv3 - See LICENSE file for details
