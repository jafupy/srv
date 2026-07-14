# Srv

Fast native static file server with instant LAN sharing.

```sh
srv dist
```

Srv binds to the local network by default, prints both local and network URLs, renders a terminal QR code, and reloads connected browsers when files change.

## Features

- Single native executable
- Automatic LAN URL and QR code
- Live reload without a build-tool plugin
- Automatic fallback to the next available port
- SPA fallback and clean URLs
- Range requests, HEAD requests, ETags, and `Last-Modified`
- Safe capability-based filesystem access
- Hidden-file protection by default
- Directory listings with file sizes
- CORS, custom headers, and cache-control options
- Custom 404 page
- Optional browser launch

## Install

From the repository:

```sh
cargo install --git https://github.com/jafupy/srv
```

From a checkout:

```sh
cargo install --path .
```

## Usage

```text
srv [OPTIONS] [DIRECTORY]
```

Common examples:

```sh
# Serve the current directory on the LAN
srv

# Serve a build directory and open it locally
srv dist --open

# Single-page application fallback
srv dist --single

# Local machine only
srv --host 127.0.0.1

# Mobile API testing
srv dist --cors --header 'Cross-Origin-Resource-Policy: cross-origin'

# Cache fingerprinted assets for one year
srv dist --cache 31536000 --immutable

# Disable live reload and directory listings
srv dist --no-reload --no-listing
```

Run `srv --help` for the complete option list.

## Defaults

Srv is intentionally LAN-first. It binds to `0.0.0.0:3000`, advances to the next available port when necessary, hides dotfiles, enables directory listings and clean URLs, and sends `Cache-Control: no-cache` for successful files. Use `--host 127.0.0.1` when the server should not be reachable by other devices.

Live reload ignores `.git`, `node_modules`, and `target` trees and debounces rapid filesystem events.

## Security

Anyone who can reach the selected interface can request visible files under the served directory. Srv prevents path traversal and blocks symlinks that escape the served root, but it does not provide authentication. Do not serve sensitive directories on an untrusted network.

## Scope

Srv is a development and local-sharing server, not a production reverse proxy. TLS termination, authentication, rate limiting, and application routing belong in a production web server.
