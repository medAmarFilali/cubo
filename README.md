# Cubo

A lightweight container runtime written in Rust. Cubo provides process and resource isolation using Linux namespaces, similar to Docker.

## Features

- Container lifecycle management (run, stop, remove, list)
- Image building from Cubofiles (text and TOML formats)
- Image pulling from OCI-compatible registries
- Linux namespace isolation (user, mount, PID, UTS, network)
- Volume mounts and port mapping
- Environment variables and resource limits

## Requirements

- Linux (uses Linux-specific namespaces and syscalls)
- Rust toolchain

## Building

```bash
cargo build --release
```

## Usage

### Pull an image

```bash
cubo pull alpine:latest
```

### Run a container

```bash
cubo run alpine:latest /bin/sh
```

### List containers

```bash
cubo ps
```

### Stop a container

```bash
cubo stop <container_id>
```

### Remove a container

```bash
cubo rm <container_id>
```

### Build an image

```bash
cubo build -t myimage:latest .
```

## Cubofile Formats

Cubo supports two formats for defining image builds: text-based Cubofile and TOML-based Cubofile.toml.

### Text Format (Cubofile)

```
BASE alpine:latest
RUN apk add --no-cache curl
ENV APP_ENV=production
WORKDIR /app
COPY ./src /app/src
CMD /bin/sh
```

Supported instructions:
- `BASE` - Base image
- `RUN` - Execute command during build
- `COPY` - Copy files into image
- `ENV` - Set environment variable
- `WORKDIR` - Set working directory
- `CMD` - Default command

### TOML Format (Cubofile.toml)

```toml
[image]
base = "alpine:latest"

[[run]]
command = "apk add --no-cache python3"

[[run]]
command = "pip3 install flask"

[config]
workdir = "/app"
cmd = ["python3", "app.py"]
expose = ["8080/tcp"]

[config.env]
APP_ENV = "production"
PORT = "8080"
```

The TOML format provides a structured alternative with sections for image configuration, run commands, and container settings.

## Planned Improvements

- Rootless containers by default for improved security
- Cgroups v2 support for better resource isolation
- Container networking (bridge networks, DNS)
- Container logging and log drivers
- Health checks
- Multi-stage builds

## License

See LICENSE file for details.
