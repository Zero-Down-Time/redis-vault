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
  # Redis connection string
  connection_string: "redis://localhost:6379"

  # Path to Redis data directory containing dump.rdb
  data_path: "/data"

  # Unique name for this Redis node
  node_name: "redis-master-01"

  # Backup configuration based on Redis role
  backup_master: true      # Backup if this node is a master
  backup_replica: false    # Backup if this node is a replica

backup:
  # Storage backend URL (S3 or GCS)
  # Format: s3://bucket-name/prefix/ or gs://bucket-name/prefix/
  storage_url: "s3://my-redis-vault/production/redis/"

  # Interval between backup checks
  # Supports formats like: 30s, 5m, 1h, 6h, 1d
  interval: "1h"

  # Filename of the Redis dump file
  dump_filename: "dump.rdb"

  # Initial delay before starting backups (allows Redis replication to stabilize)
  # Supports formats like: 30s, 5m, 10m
  initial_delay: "300s"

# Examples of storage_url:
# S3:  storage_url: "s3://my-bucket/path/to/backups/"
# GCS: storage_url: "gs://my-bucket/path/to/backups/"

retention:
  # Number of recent backups to keep
  keep_last: 7

  # Keep backups newer than this duration
  # Supports formats like: 7d, 30d, 1w
  keep_duration: "30d"

logging:
  # Log format: "text" or "json"
  format: "text"

  # Application log level: trace, debug, info, warn, error
  # Note: Default log level for other crates is set to "warn"
  # Use RUST_LOG environment variable to override all log levels
  level: "info"

metrics:
  # Enable Prometheus metrics endpoint
  enabled: false

  # Port for metrics server
  port: 9090

  # Listen address for metrics server
  listen_address: "0.0.0.0"
```

### Backup File Naming

Backup files are automatically named using the following structure:

```
{prefix}/{node_name}_{timestamp}.rdb
```

**Example filename:**
```
redis-vault/redis-master-01_2024-12-01T14:30:22Z.rdb
```

**Components:**
- `prefix`: Storage prefix from configuration (e.g., "redis-vault")
- `node_name`: Redis node identifier (e.g., "redis-master-01")
- `timestamp`: File modification time in RFC3339 format (ISO 8601)
- `.rdb`: File extension

**Note:** The timestamp reflects the Redis dump file's last modification time, ensuring backups are named based on when the data was actually created by Redis, not when the backup process ran.

### Environment Variables

Environment variables **override** any values set in the configuration file. This allows for easy deployment-specific overrides.

#### **Redis Configuration**

| Variable | Description | Default |
|----------|-------------|---------|
| `REDIS_CONNECTION` | Redis connection string | `redis://localhost:6379` |
| `REDIS_DATA_PATH` | Path to Redis data directory | `/data` |
| `REDIS_NODE_NAME` | Unique name for this Redis node | `redis-node` |
| `BACKUP_MASTER` | Backup if node is master (`true` or `false`) | `true` |
| `BACKUP_REPLICA` | Backup if node is replica (`true` or `false`) | `true` |

#### **Backup Configuration**

| Variable | Description | Default | Example |
|----------|-------------|---------|---------|
| `STORAGE_URL` | Storage backend URL (S3 or GCS) | `s3://redis-vault-bucket/` | `s3://my-bucket/redis/` or `gs://my-bucket/backups/` |
| `BACKUP_INTERVAL` | Time between backup checks | `1h` | `30m`, `6h`, `1d` |
| `DUMP_FILENAME` | Redis dump filename | `dump.rdb` | `dump.rdb` |
| `INITIAL_DELAY` | Initial delay before first backup | `300s` | `60s`, `5m`, `10m` |

**Note:** `STORAGE_URL` uses URL format:
- **S3:** `s3://bucket-name/optional-prefix/`
- **GCS:** `gs://bucket-name/optional-prefix/`

The storage backend (S3 or GCS) is automatically determined from the URL scheme.

#### **Retention Configuration**

| Variable | Description | Default | Example |
|----------|-------------|---------|---------|
| `RETENTION_KEEP_LAST` | Number of recent backups to keep | `7` | `30`, `90` |
| `RETENTION_KEEP_DURATION` | Keep backups newer than this duration | None | `7d`, `30d`, `90d` |

#### **Logging Configuration**

| Variable | Description | Default | Options |
|----------|-------------|---------|---------|
| `LOG_FORMAT` | Log format | `text` | `text`, `json` |
| `LOG_LEVEL` | Application log level | `info` | `trace`, `debug`, `info`, `warn`, `error` |
| `RUST_LOG` | Override all log levels (takes precedence over `LOG_LEVEL`) | None | `debug`, `redis_vault=trace` |

#### **Metrics Configuration**

| Variable | Description | Default | Example |
|----------|-------------|---------|---------|
| `METRICS_ENABLED` | Enable Prometheus metrics endpoint | `false` | `true`, `false` |
| `METRICS_PORT` | Port for metrics server | `9090` | `8080`, `9090` |
| `METRICS_LISTEN_ADDRESS` | Listen address for metrics server | `0.0.0.0` | `0.0.0.0`, `127.0.0.1` |

### Configuration Override Example

```bash
# Base configuration in config.yaml sets storage to dev environment
# Override for production deployment:
export STORAGE_URL="s3://prod-redis-backups/redis-vault/"
export RETENTION_KEEP_LAST="30"
export RETENTION_KEEP_DURATION="90d"
export METRICS_ENABLED="true"

# These environment variables will override the file configuration
redis-vault --config config.yaml
```

**Example with GCS:**
```bash
# Use Google Cloud Storage instead
export STORAGE_URL="gs://prod-redis-backups/redis-vault/"
export REDIS_NODE_NAME="redis-master-01"
export LOG_FORMAT="json"
export METRICS_PORT="8080"

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
