# STARS Server Rust Implementation - Copilot Instructions

## Project Overview
STARSRUST is a Rust reimplementation of the STARS (remote control) server, originally from the Perl version by Takashi Kosuge at KEK Tsukuba. It's a multi-threaded TCP server that manages client connections, command routing, and access control through configuration files.

**Key characteristics:**
- Single-threaded event loop with spawned handler threads per connection
- Configuration-driven security (host allowlists, command permissions, reconnection rules)
- Custom error handling via `StarsError` type
- Minimal external dependencies (regex, rand, chrono, clap, configparser, dns-lookup)

## Architecture & Data Flow

### Core Components
1. **Main Server Loop** ([src/main.rs](src/main.rs#L125-L190))
   - `TcpListener` accepts connections on configurable port (default 6057)
   - Each client gets a unique `nodekey` (random u16)
   - Spawns a thread per connection for bidirectional communication

2. **Shared State** ([src/main.rs](src/main.rs#L145-L155))
   - `nodes: Arc<Mutex<NodeList>>` - HashMap of active connections (name → TcpStream)
   - `sd: Arc<Mutex<StarsData>>` - All configuration data (permissions, aliases, reconnect rules)
   - Pattern: Always scope lock blocks with `{}` to ensure drops ([src/main.rs](src/main.rs#L160-L166))

3. **Configuration Loading** ([src/utilities.rs](src/utilities.rs#L33-L75))
   - Files loaded from `libdir` (default: `takaserv-lib/`)
   - Comments (lines starting with `#`) and empty lines ignored
   - Atomic initialization before server loop starts

4. **Access Control** ([src/utilities.rs](src/utilities.rs#L101-L144))
   - Wildcard patterns in config files converted to regex (e.g., `*.kek.jp` → regex `.*\.kek\.jp`)
   - Host checking supports both hostname and IP matching
   - Per-node `.allow` files for terminal-specific access control

### Configuration Files (in `takaserv-lib/`)
- `allow.cfg` - Host allowlist (hostname/IP patterns)
- `command_deny.cfg` & `command_allow.cfg` - Command restrictions
- `aliases.cfg` - Bidirectional alias ↔ real name mappings
- `reconnectable_*.cfg` - Reconnection permission rules
- `shutdown_allow.cfg` - Shutdown permission list
- Node-specific `<nodename>.allow` - Per-node terminal allowlists

## Project-Specific Patterns & Conventions

### Error Handling
- **Type alias**: `GenericResult<T> = Result<T, GenericError>` where `GenericError = Box<dyn Error + Send + Sync>`
- **Custom error**: `StarsError` struct with String message (implements `std::error::Error`)
- **Pattern**: Return early from functions on error; use `?` operator liberally
- **Server startup**: Use `startcheck()` helper to validate config loading without panicking

### Debug Builds
- Use `dbprint!` macro (custom; prints only in `#[cfg(debug_assertions)]`)
- Define with: `dbprint!(variable_name)` for debug output
- Enabled by `cargo build` (disabled in `cargo build --release`)

### Lazy Static Pattern
- Custom `lazy_static!` macro replaces external crate (Rust 1.80+ with `LazyLock`)
- Used for regex patterns: `static ref SEARCHCMD1: Regex = Regex::new(...)`
- Regexes compiled once at first use, not per invocation

### Thread Safety
- **Critical**: Always scope mutex locks with `{}` blocks - DO NOT hold locks across thread spawning
- Clone `Arc<Mutex<T>>` and `TcpStream` for passing to handler threads
- Never use `.expect()` on lock acquisition unless absolutely certain - prefer error handling

## Build & Development Workflow

### Build Commands
```bash
cargo build              # Debug build with dbprint enabled
cargo build --release   # Optimized release (LTO, stripped symbols)
```

### Configuration
- **Runtime config**: `stars.cfg` (INI format) - keys: `starsport`, `starslib`, `starskey`
- **CLI args** (override config): `-p/--port`, `-l/--libdir`, `-k/--keydir`, `-t/--timeout`
- **Timeout**: Read timeout in milliseconds (0 = no timeout)

### Testing Config Files
- Parse test: Try loading with `cargo build` - any file format errors appear at startup
- Test patterns: Create test files in `takaserv-lib/` and run server locally

## Key Files & When to Edit

| File | Purpose | When to modify |
|------|---------|----------------|
| [src/main.rs](src/main.rs) | Server loop, threading, message routing | Protocol changes, connection handling |
| [src/definitions.rs](src/definitions.rs) | Constants, type aliases, macros | Config filenames, buffer sizes, version |
| [src/starsdata.rs](src/starsdata.rs) | Data structure for loaded config | Add new config categories |
| [src/utilities.rs](src/utilities.rs) | File I/O, regex patterns, helpers | Config parsing logic, security checks |
| [src/starserror.rs](src/starserror.rs) | Error type definition | Custom error fields (rarely needed) |

## Important Constraints & Gotchas

1. **No async/await** - Uses threads and blocking I/O; not event-driven
2. **Regex lifetimes** - `lazy_static!` regexes are leaked; safe for Rust 1.80+ with `LazyLock`
3. **String splitting** - Use `.split_whitespace()` for config parsing (handles tabs, multiple spaces)
4. **Newline handling** - `SEARCHSPLIT` regex normalizes `\r\n` to `\n` from client messages
5. **Lock poisoning** - Server continues on lock acquisition failure; wrapped in `.expect()` assuming recovery
6. **DNS lookups** - Blocking call; can add latency on first connection - fallback to IP on failure

## Integration Points

- **External crates** (see Cargo.toml for versions):
  - `clap::Parser` - CLI argument parsing with derive macros
  - `configparser::ini::Ini` - INI file parsing (stars.cfg)
  - `dns_lookup::lookup_addr` - Reverse DNS resolution for hostnames
  - `regex::Regex` - Pattern matching for security rules
  - `chrono::DateTime<Local>` - Timestamps for logging

- **File system** - Relative paths from server launch directory; `takaserv-lib/` must be co-located
