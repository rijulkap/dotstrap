# dotstrap

`dotstrap` installs development tools and links their configuration files from
a versioned TOML manifest.

## Application flow

The application processes every request in three phases:

1. `cli` parses the command line and loads the manifest.
2. Validation expands tags, verifies dependencies and configuration sources,
   and creates a dependency-ordered execution plan.
3. `manager` applies that plan using the process and filesystem helpers in
   `shell`.

No installation or configuration occurs until the complete request has passed
validation. Execution then stops on the first failed tool so dependants are not
processed after a prerequisite fails.

## Usage

```console
cargo run -- \
  --manifest /path/to/dotfiles.toml \
  --os linux_x64 \
  validate
```

Install every tool:

```console
cargo run -- \
  --manifest /path/to/dotfiles.toml \
  --os linux_x64 \
  install
```

Install selected tools or all tools matching a tag:

```console
cargo run -- \
  --manifest /path/to/dotfiles.toml \
  --os linux_x64 \
  install --tools git,neovim --tags dev
```

Available subcommands are:

- `install`
- `configure`
- `install-and-configure`
- `remove-symlinks`
- `validate`

An empty tool/tag selection means all tools.

`remove-symlinks` removes configuration targets only when they are symbolic
links. Missing targets are ignored, while ordinary files and directories are
never deleted.

Pass the global `--force` flag to reinstall directly selected tools even when
their check executable already exists. Dependencies retain their normal checks.
Use `--force-all` to force the selected tools and their entire dependency
chains. During configuration, forced tools replace an existing symlink before
creating the new link. Neither mode overwrites a regular file or directory.

## Manifest

[`schema.toml`](schema.toml) is a representative schema example and test
fixture. It demonstrates minimal tools, dependencies, tags, platform-specific
install commands, file configuration, and directory configuration.

Tool dependencies use `deps`. Installation commands and configuration targets
are selected using the exact platform key passed to `--os`.

A tool check may be one executable shared by every platform:

```toml
check = "git"
```

It may instead provide platform-specific executable names:

```toml
check = { linux_x64 = "fdfind", windows_x64 = "fd" }
```

When the current platform is absent from a platform-specific check, dotstrap
runs the installer without performing a pre-install or post-install check.

Commands belonging to one tool run in order inside one shell, allowing values
such as environment variables to carry between those commands. Each tool gets
a fresh shell. Installed tools with a successful `check` executable are
skipped.

Relative configuration sources are resolved from the directory containing the
manifest. A leading `~` in a configuration target is expanded to the current
user's home directory.

## Development

Run the project checks with:

```console
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo doc --no-deps
```

## Releases

Pushing a tag whose name starts with `v` runs the release workflow on native
Linux, Windows, and macOS runners. For example:

```console
git tag v0.1.0
git push origin v0.1.0
```

After all builds and test suites pass, the workflow creates or updates the
corresponding GitHub release with:

- `dotstrap-linux-x64`
- `dotstrap-linux-aarch64`
- `dotstrap-windows-x64.exe`
- `dotstrap-macos-aarch64`
- `dotstrap-macos-x64`
