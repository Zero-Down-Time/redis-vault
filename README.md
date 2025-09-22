# Redis Vault

A production-ready Rust application designed to run as a sidecar container alongside Redis instances, providing automated secure backups to S3 or Google Cloud Storage with configurable retention policies.

## Features

- **Non-intrusive backups**: Uses existing `dump.rdb` files without triggering BGSAVE
- **Multi-cloud support**: Backs up to AWS S3 or Google Cloud Storage
- **Role-aware backups**: Configurable to backup from masters, replicas, or both
- **Automatic retention management**: Cleanup old backups based on count and age
- **Kubernetes-ready**: Designed for sidecar deployment pattern
- **Comprehensive logging**: Structured JSON logging with tracing
- **Flexible configuration**: YAML config files or environment variables
- **Alpine-based**: Minimal Docker image (~25-30MB)
- **Environment variable precedence**: Env vars override config file settings

## Quick Start

### Building

```bash
# Build the binary
cargo build --release

# Build Docker image (uses Alpine Linux for minimal size)
docker build -t redis-vault:latest .

# Build with specific versions
docker build \
  --build-arg RUST_VERSION=1.87 \
  --build-arg ALPINE_VERSION=3.22 \
  -t redis-vault:latest .
```

The Docker image is based on Alpine Linux for a minimal footprint (~15MB base image vs ~80MB for Debian). The final image size is approximately 25-30MB.

### Running with Docker Compose

```bash
# Set AWS credentials
export AWS_ACCESS_KEY_ID=your-key
export AWS_SECRET_ACCESS_KEY=your-secret

# Start Redis with backup sidecar
docker-compose up -f docker/docker-compose.yml
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

This allows you to:
- Use a base configuration file with environment-specific overrides
- Deploy the same image with different configurations
- Override specific settings without modifying files

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
```

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
| `RUST_LOG` | Log level | `info` |

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

Best for production Redis deployments:
- Shared volume between Redis and backup containers
- Automatic restart on failure
- Resource limits to prevent impact on Redis

### Docker Compose

Ideal for development and testing:
- Easy local testing
- Multiple Redis instances with different backup configs
- Simulates production topology

## Security Considerations

1. **Least Privilege**: Backup container has read-only access to Redis data
2. **IAM Roles**: Use IAM roles instead of keys when possible
3. **Encryption**: Enable S3/GCS encryption at rest
4. **Network Isolation**: Backup sidecar doesn't expose any ports
5. **Non-root User**: Runs as unprivileged user in container

## License

AGPLv3 - See LICENSE file for details
