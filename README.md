# docker-shell

Interactively select a Docker container and open an interactive shell inside it, or run a new container from an image.

## Features

- **Fuzzy container selection** — type to filter and pick from running containers
- **Run new containers** — launch a container from an image with the host directory mounted
- **Customizable shell** — choose `bash`, `sh`, or any custom shell
- **User mode** — run as default, host user, root, or custom `user:group`
- **Working directory** — use the container's default, root `/`, or a custom path
- **Mount control** — in `--run` mode, choose to mount the current directory at `/workspace`, a custom path, or not at all
- **CLI or interactive** — pass arguments directly or let the menus guide you

## Prerequisites

- [Docker](https://www.docker.com/) installed and running
- The user running `docker-shell` must have access to the Docker socket

## Building

### Native build

```bash
cargo build --release
```

### Docker build

```bash
bash build.sh
```

## Usage

### Interactive mode

Run without arguments to enter interactive menus:

```bash
./target/release/docker-shell
```

### CLI mode

Specify options directly:

```bash
# Target a specific container
./target/release/docker-shell --container my-app

# Choose a shell
./target/release/docker-shell --container my-app --shell zsh

# Run as a specific user
./target/release/docker-shell --container my-app --user root

# Set a working directory
./target/release/docker-shell --container my-app --workdir /app

# Run a new container from an image
./target/release/docker-shell --run --container ubuntu:latest

# Run without mounting the current directory
./target/release/docker-shell --run --container ubuntu:latest --workdir none

# Run with a custom mount point
./target/release/docker-shell --run --container ubuntu:latest --workdir /app
```

### Mixed mode

Use `--custom` to enable interactive menus for options you don't specify on the command line:

```bash
./target/release/docker-shell --container my-app --custom
```

## Arguments

| Argument | Short | Description |
|---|---|---|
| `--custom` | `-c` | Show interactive menus for unspecified options |
| `--shell` | `-s` | Shell to use (`bash`, `sh`, `zsh`, etc.) |
| `--user` | `-u` | User mode: `default`, `host`, `root`, or `user:group` |
| `--workdir` | `-w` | Working directory (`default`, `/`, `none`, or custom path) |
| `container` | — | Name or ID of the target container (or image for `--run`) |
| `--run` | `-r` | Run a new container from an image instead of entering an existing one |

## How it works

1. Connects to the Docker daemon via the local socket
2. Fetches all running containers using the Docker API
3. Lets you pick a container (fuzzy search in interactive mode)
4. Determines shell, user, and working directory from args or menus
5. Executes `docker exec -it` with the resolved options

### `--run` mode

When using `--run`, docker-shell:
1. Lists available Docker images
2. Lets you select an image to run
3. Prompts for shell, user, and mount point options
4. Mounts the current host directory into the container (or skips mounting with `none`)
5. Executes `docker run -it --rm` with the resolved options

## License

MIT
