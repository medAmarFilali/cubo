# 🧊 Cubo 🧊

A lightweight, early-stage containerization tool written in Rust. Cubo is an experiment and a work in progress. It is not production-ready and its behavior and interfaces may change without notice.

## Overview

Cubo provides container-like isolation using Linux primitives and OCI-compatible image management:

- Runs commands inside isolated filesystems using `chroot`, Linux namespaces, and process isolation.
- Creates ephemeral rootfs per container under a configurable runtime root directory.
- Supports pulling OCI-compliant images from registries (Docker Hub, GitHub Container Registry, etc.).
- Builds custom images from Cubofile or Cubofile.toml specifications.
- Manages container metadata, lifecycle, and logs with OCI-inspired storage formats.
- Supports volume mounts, environment variables, working directories, and port mappings.
- Uses `tracing` for structured logging and `tokio` for async orchestration.

Current scope is intentionally narrow. Some features remain incomplete or partially implemented.

## Status and Safety

- Early-stage and experimental: interfaces and behavior are unstable and may change.
- Linux only. Uses `nix`, `chroot`, and Linux namespaces; typically requires root privileges.
- Provides basic isolation through namespaces and chroot, but not production-grade security.
- Persists OCI-like config and state per container; runtime reconciles state on startup.
- Several features (network isolation, full namespace support) are under development.

## Requirements

- Rust toolchain (edition 2021).
- Linux host with standard coreutils available in `/bin`.
- Root privileges for running containers (due to `chroot`, namespaces, and process setup).
- Internet connectivity for pulling images from registries (optional).

## Build

```bash
cargo build --release
```

The binary will be at `target/release/cubo`.

## Installation

After building, you can optionally install the binary to your system:

```bash
sudo cp target/release/cubo /usr/local/bin/
```

Or add the `target/release` directory to your `PATH`.

## Usage

Basic pattern (run with root privileges):

```bash
sudo cubo <COMMAND> [OPTIONS]
```

Available subcommands:

- `run`: Run a container from a blueprint (image) name and optional command.
- `ps`: List containers (running and stopped with `--all`).
- `stop`: Stop running container(s) by ID or name.
- `rm`: Remove container(s) by ID or name.
- `logs`: Fetch logs from a container with options for following, tailing, and timestamps.
- `pull`: Pull an OCI-compliant image from a registry.
- `build`: Build a custom image from a Cubofile or Cubofile.toml.
- `blueprint`: List available blueprints (images).
- `rmb`: Remove blueprints (images) by name or ID.

### Run

Run a container from an image (blueprint):

```bash
sudo cubo run <BLUEPRINT> [COMMAND [ARGS...]] \
  [--name NAME] \
  [--workdir DIR] \
  [--env KEY=VAL ...] \
  [--volume HOST:CONT[:ro] ...] \
  [--publish HOST:CONT[/tcp|udp] ...] \
  [--interactive]
```

Examples:

- Run an interactive shell in an Alpine container:

  ```bash
  sudo cubo pull alpine:latest
  sudo cubo run alpine:latest /bin/sh --interactive
  ```

- Run a command with environment variables and a read-only bind mount:

  ```bash
  sudo cubo run alpine:latest \
    --env GREETING=hello \
    --volume /tmp:/data:ro \
    /bin/sh -c 'echo $GREETING; ls -la /data'
  ```

- Run a web server with port mapping:

  ```bash
  sudo cubo run nginx:latest \
    --name my-web-server \
    --publish 8080:80 \
    --workdir /usr/share/nginx/html
  ```

Options:

- `--name`: Assign a human-readable name to the container.
- `--workdir`: Set the working directory inside the container.
- `--env`: Set environment variables (can be repeated).
- `--volume`: Mount a host directory into the container (format: `host:container[:ro]`).
- `--publish`: Map host ports to container ports (format: `host:container[/tcp|udp]`).
- `--interactive`: Run in interactive/attached mode (default is detached).

Notes:

- If no command is specified, Cubo uses the default `CMD` from the image configuration.
- Volume mounts support both read-write and read-only modes.
- Port publishing is parsed and stored but network isolation is under development.

### List Containers

```bash
cubo ps [--all]
```

Lists containers discovered under the configured root directory. By default, shows only running containers. Use `--all` to see all containers including stopped ones.

Example output:

```
CONTAINER ID   NAME          IMAGE           COMMAND       CREATED        STATUS
a1b2c3d4       my-alpine     alpine:latest   /bin/sh       2 hours ago    running
e5f6g7h8       web-server    nginx:latest    nginx -g ...  1 day ago      stopped
```

### Stop Containers

```bash
sudo cubo stop <ID|NAME> [<ID|NAME> ...] [--force]
```

Stops one or more running containers. Containers are stopped gracefully by sending SIGTERM to the main process. Use `--force` to send SIGKILL immediately.

Examples:

```bash
sudo cubo stop my-alpine
sudo cubo stop a1b2c3d4 e5f6g7h8
sudo cubo stop --force unresponsive-container
```

### Remove Containers

```bash
sudo cubo rm <ID|NAME> [<ID|NAME> ...] [--force]
```

Removes container(s) and their associated filesystem. Running containers must be stopped first, or use `--force` to stop and remove in one operation.

Examples:

```bash
sudo cubo rm my-alpine
sudo cubo rm --force running-container
sudo cubo rm a1b2c3d4 e5f6g7h8
```

### Fetch Logs

```bash
sudo cubo logs <ID|NAME> [--follow] [--tail N] [--timestamps]
```

Fetches logs from a container. Logs are captured from the container's stdout and stderr.

Options:

- `--follow` or `-f`: Follow log output in real-time (like `tail -f`).
- `--tail N` or `-n N`: Show only the last N lines of logs.
- `--timestamps` or `-t`: Include timestamps in the log output.

Examples:

```bash
sudo cubo logs my-alpine
sudo cubo logs --follow --timestamps web-server
sudo cubo logs --tail 100 my-alpine
```

### Pull Images

```bash
sudo cubo pull <IMAGE>
```

Pulls an OCI-compliant image from a registry. Supports Docker Hub, GitHub Container Registry, and other OCI-compliant registries.

Examples:

```bash
sudo cubo pull alpine:latest
sudo cubo pull ubuntu:22.04
sudo cubo pull nginx:1.25
sudo cubo pull ghcr.io/owner/image:tag
```

The image is downloaded, extracted, and stored in the image store under `root_dir/images/`. Image configuration (CMD, ENV, WORKDIR, etc.) is preserved and used when running containers.

### Build Images

```bash
sudo cubo build <PATH> [--tag NAME:TAG] [--file CUBOFILE] [--no-cache]
```

Builds a custom image from a Cubofile or Cubofile.toml specification.

Options:

- `--tag` or `-t`: Name and tag for the built image (e.g., `myapp:v1.0`). Defaults to `<dir-name>:latest`.
- `--file` or `-f`: Path to the build file. Auto-detects `Cubofile.toml` or `Cubofile` if not specified.
- `--no-cache`: Do not use cache when building the image.

Examples:

```bash
sudo cubo build . --tag myapp:v1.0
sudo cubo build /path/to/context --file Cubofile.custom
sudo cubo build . --no-cache
```

#### Cubofile Format (Text)

The text-based Cubofile supports Docker-like instructions:

```dockerfile
FROM alpine:latest

RUN apk add --no-cache python3 py3-pip
RUN mkdir -p /app

COPY app.py /app/
COPY requirements.txt /app/

WORKDIR /app
RUN pip3 install -r requirements.txt

ENV APP_ENV=production
ENV PORT=8080

EXPOSE 8080/tcp

CMD ["python3", "app.py"]
```

Supported instructions:

- `FROM`: Base image (required).
- `RUN`: Execute a command during build.
- `COPY`: Copy files from build context to image.
- `WORKDIR`: Set working directory.
- `ENV`: Set environment variables.
- `EXPOSE`: Document exposed ports.
- `CMD`: Default command to run.

#### Cubofile.toml Format

The TOML-based format provides a declarative alternative:

```toml
[image]
base = "alpine:latest"

[config]
workdir = "/app"
expose = ["8080/tcp"]

[config.env]
PORT = "8080"
APP_ENV = "production"
PYTHONUNBUFFERED = "1"

[[config.run]]
command = "apk add --no-cache python3 py3-pip"

[[config.run]]
command = "mkdir -p /app"

[[config.copy]]
source = "app.py"
destination = "/app/"

[[config.copy]]
source = "requirements.txt"
destination = "/app/"

[[config.run]]
command = "pip3 install -r /app/requirements.txt"

[config.cmd]
command = ["python3", "app.py"]
```

### List Blueprints

```bash
cubo blueprint [--all true]
```

Lists available blueprints (images) in the image store.

### Remove Blueprints

```bash
sudo cubo rmb <NAME|ID> [<NAME|ID> ...] [--force]
```

Removes blueprints (images) from the image store. Use `--force` to remove images that are in use by containers.

## How It Works

### Container Runtime

- `ContainerRuntime` manages an in-memory map of containers and a root directory.
- Root directory is configurable via `--root-dir` flag or `CUBO_ROOT` environment variable.
- Each container gets a unique ID (UUID) and an optional human-readable name.
- Container lifecycle: `created` → `running` → `stopped`.

### Container Creation

- `create_container` scaffolds a bundle directory: `rootfs/`, `config.json`, and `state.json`.
- Copies essential host binaries (sh, echo, etc.) or unpacks an OCI image layer into `rootfs/`.
- Sets up metadata, environment variables, working directory, and volume mount points.

### Container Execution

- `start_container` forks a child process.
- Child process sets up Linux namespaces (PID, mount, UTS, IPC, user).
- Performs `chroot` into the container's `rootfs/`.
- Sets environment variables and working directory.
- Executes the requested command via `execv`.

### Volume Mounts

- Volume mounts are specified as `host_path:container_path[:ro]`.
- Currently simulated by creating directory structures inside the rootfs.
- Real bind mounts are under development.

### Image Management

- Images are stored under `root_dir/images/`.
- Each image has a manifest, configuration, and layer blobs.
- Pulling an image downloads OCI-compliant artifacts from a registry.
- Building an image processes Cubofile instructions and creates layers.

### Logging

- Container stdout and stderr are captured and stored.
- Logs are accessible via the `logs` command.
- Supports real-time following and tail-like behavior.

## Architecture

Key modules:

- `src/cli.rs`: Command-line interface definitions using `clap`.
- `src/main.rs`: Entry point and command dispatch.
- `src/error.rs`: Centralized error types using `thiserror`.
- `src/commands/`: CLI subcommand implementations.
  - `run.rs`: Container creation and execution.
  - `ps.rs`: Container listing.
  - `stop.rs`: Container stopping.
  - `rm.rs`: Container removal.
  - `logs.rs`: Log fetching and streaming.
  - `pull.rs`: Image pulling from registries.
  - `build.rs`: Image building from Cubofiles.
  - `blueprints.rs`: Blueprint listing.
  - `rmb.rs`: Blueprint removal.
- `src/container/`: Core container and image logic.
  - `runtime.rs`: Container lifecycle, process management, chroot, namespaces.
  - `container_store.rs`: Container persistence and state management.
  - `image_store.rs`: Image storage, manifest handling, layer extraction.
  - `rootfs.rs`: Rootfs preparation, layer unpacking, filesystem operations.
  - `builder.rs`: Image building from Cubofile instructions.
  - `registry.rs`: OCI registry client, image pulling, authentication.
  - `cubofile.rs`: Text-based Cubofile parser.
  - `cubofile_toml.rs`: TOML-based Cubofile parser.
  - `namespace.rs`: Linux namespace setup and management.
  - `mod.rs`: Container types, configuration, and helpers.

## Root Directory Configuration

The root directory stores all container bundles, images, and state.

Configuration precedence:

1. `--root-dir` CLI flag (highest priority).
2. `CUBO_ROOT` environment variable.
3. Default resolution (in order):
   - `$XDG_STATE_HOME/cubo`
   - `$XDG_DATA_HOME/cubo`
   - `$HOME/.local/state/cubo`
   - `/tmp/cubo` (fallback if HOME is unset).

Examples:

```bash
# Use custom root directory via flag
sudo cubo --root-dir /var/lib/cubo run alpine:latest /bin/sh

# Use custom root directory via environment variable
export CUBO_ROOT=/var/lib/cubo
sudo -E cubo run alpine:latest /bin/sh
```

## On-disk Layout

Cubo uses an OCI-inspired directory structure:

```
root_dir/
├── <container-id>/
│   ├── config.json          # Container configuration
│   ├── state.json           # Runtime state (OCI-compliant)
│   └── rootfs/              # Container root filesystem
│       ├── bin/
│       ├── etc/
│       ├── lib/
│       └── ...
└── images/
    └── <image-name>/
        ├── manifest.json    # OCI manifest
        ├── config.json      # Image configuration
        └── blobs/
            └── sha256/      # Layer blobs
                ├── <layer-hash>
                └── ...
```

### Container Bundle Structure

Each container has a bundle directory under `root_dir/<id>/`:

- `config.json`: Full container configuration (command, env, volumes, ports, etc.).
- `state.json`: OCI-compliant runtime state.
- `rootfs/`: Container root filesystem (unpacked image layers or minimal filesystem).

### State JSON Format

`state.json` follows the OCI runtime state specification:

```json
{
  "ociVersion": "1.0.2",
  "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "status": "running",
  "pid": 12345,
  "bundle": "/var/lib/cubo/a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "annotations": {
    "name": "my-container",
    "blueprint": "alpine:latest"
  }
}
```

Status values: `creating`, `created`, `running`, `stopped`, `paused`, `unknown`.

### Image Store Structure

Images are stored under `root_dir/images/<image-name>/`:

- `manifest.json`: OCI image manifest (layers, config digest).
- `config.json`: Image configuration (CMD, ENV, WORKDIR, etc.).
- `blobs/sha256/<hash>`: Compressed or extracted layer content.

### Atomic Writes

All writes to `config.json` and `state.json` are atomic:

1. Write to a temporary file in the same directory.
2. Rename the temporary file over the target (atomic operation).

This ensures consistency even if the process is interrupted.

### Startup Reconciliation

On runtime initialization, Cubo reconciles container state:

1. Discovers existing bundles under `root_dir`.
2. Loads `config.json` and `state.json` for each bundle.
3. Checks if containers marked as `running` are actually alive (via PID).
4. Updates state to `stopped` and stamps `finished_at` for dead processes.
5. Persists reconciled state atomically.

## Development

### Running Tests

```bash
cargo test
```

Integration tests are in `tests/integration_tests.rs`.

### Logging and Debugging

Use `RUST_LOG` environment variable to control log verbosity:

```bash
RUST_LOG=debug cargo run -- run alpine:latest /bin/sh
RUST_LOG=info,cubo::container::runtime=trace cargo run -- ps --all
```

Enable backtraces for debugging:

```bash
RUST_BACKTRACE=1 cargo run -- ...
RUST_BACKTRACE=full cargo run -- ...
```

### Code Structure

- Follow Rust standard formatting: `cargo fmt`.
- Lint with Clippy: `cargo clippy`.
- Run tests before committing: `cargo test`.

### Contributing

Contributions are welcome, but keep in mind this is an experimental project with unstable interfaces. Before submitting a pull request:

1. Ensure all tests pass.
2. Add tests for new functionality.
3. Follow existing code style and patterns.
4. Update documentation as needed.

## Roadmap

Future improvements and features under consideration:

- Full Linux namespace support (user, mount, PID, network, UTS, IPC, cgroup).
- Real bind mounts and tmpfs support; volume driver interface.
- Network namespace isolation and port publishing with iptables.
- cgroups v2 resource limits (CPU, memory, PIDs, I/O).
- Full OCI runtime spec compliance.
- Image layer caching and deduplication.
- Multi-stage builds and build caching.
- Better CLI output formatting and progress indicators.
- Support for Docker Compose-like multi-container orchestration.
- Windows and macOS support (via virtualization).

## Disclaimer

This is early-stage, experimental software. It is not an educational guide and not production-ready. It does not provide strong isolation and must not be used to run untrusted code. Use at your own risk.

## License

See LICENSE file for details.

## Author

Amar FILALI
