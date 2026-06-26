# Development Setup

This guide helps a new contributor prepare Mado for local development on macOS, Linux, or Windows.

Mado is a Rust workspace with GPUI applications. The normal local loop is:

1. Install the platform build prerequisites.
2. Install Rust and the workspace quality tools.
3. Build or run the app.
4. Use the right `just` gate for the current feedback loop.

## Shared Rust Tools

Install Rust with `rustup`, then install the stable toolchain components used by the local and CI gates:

```sh
rustup toolchain install stable
rustup default stable
rustup component add rustfmt clippy llvm-tools-preview
```

Install the local quality tools:

```sh
cargo install just
cargo install cargo-deny
cargo install cargo-audit
cargo install cargo-machete
cargo install cargo-llvm-cov
cargo install cargo-fuzz
```

Install nightly when reproducing `just check-ci`, because that gate runs `fuzz-smoke`:

```sh
rustup toolchain install nightly
```

Install the full-check tools only when working on the scheduled deep gate or reproducing `just check-full`:

```sh
cargo install cargo-mutants
cargo install cargo-udeps
```

## macOS

Required platform tools:

- Xcode Command Line Tools, or Xcode with the command-line tools selected.
- Rust stable with `rustfmt`, `clippy`, and `llvm-tools-preview`.
- The shared Cargo quality tools above.

Recommended setup:

```sh
xcode-select --install
rustup toolchain install stable
rustup component add rustfmt clippy llvm-tools-preview
cargo install just
cargo install cargo-deny
cargo install cargo-audit
cargo install cargo-machete
cargo install cargo-llvm-cov
```

If `xcode-select --install` opens a GUI prompt, complete the installer and any license prompts. Afterward, verify:

```sh
xcode-select -p
clang --version
cargo check --workspace
```

## Linux

CI currently builds on Ubuntu with these system packages:

```sh
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential \
  libasound2-dev \
  libfontconfig1-dev \
  libssl-dev \
  libwayland-dev \
  libx11-dev \
  libxcb-render0-dev \
  libxcb-shape0-dev \
  libxcb-xfixes0-dev \
  libxcb1-dev \
  libxkbcommon-dev \
  libxkbcommon-x11-dev \
  pkg-config
```

`gpui-component` may require additional desktop and rendering dependencies on Linux. Its upstream setup script has been tested on Ubuntu 24.04:

```sh
sudo apt update
sudo apt install -y \
  gcc g++ clang libfontconfig-dev libwayland-dev \
  libwebkit2gtk-4.1-dev libxkbcommon-x11-dev libx11-xcb-dev \
  libssl-dev libzstd-dev \
  vulkan-validationlayers libvulkan1
```

Then install Rust and the shared Cargo tools:

```sh
rustup toolchain install stable
rustup component add rustfmt clippy llvm-tools-preview
cargo install just
cargo install cargo-deny
cargo install cargo-audit
cargo install cargo-machete
cargo install cargo-llvm-cov
```

If the machine is not Ubuntu or Debian-based, map the same library families to the platform package manager and document any new package names before relying on them.

Verify:

```sh
pkg-config --version
cargo check --workspace
```

## Windows

Required platform tools:

- Rust stable for the MSVC target.
- Visual Studio Build Tools or Visual Studio with the "Desktop development with C++" workload.
- A Windows SDK installed through Visual Studio.
- CMake for GPUI-related native builds.
- The shared Cargo quality tools above.

Visual Studio is a heavyweight GUI installer. Install or confirm:

- Visual Studio Build Tools, or Visual Studio.
- The "Desktop development with C++" workload.
- The MSVC compiler and Windows SDK components selected by that workload.
- CMake, either through Visual Studio components or another package manager.

The `gpui-component` Windows setup path can be prepared with PowerShell, `winget`, and Scoop:

```powershell
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
Invoke-RestMethod -Uri https://get.scoop.sh | Invoke-Expression

winget install Microsoft.VisualStudio.2022.Community --silent --override "--wait --quiet --add ProductLang En-us --add Microsoft.VisualStudio.Workload.NativeDesktop --includeRecommended"
scoop bucket add extras
scoop install cmake
```

Review the Visual Studio installer choice before running the `winget` command. It installs Visual Studio Community with the native desktop workload and recommended components.

After the user completes the installer, verify in a fresh terminal:

```powershell
rustup toolchain install stable-x86_64-pc-windows-msvc
rustup default stable-x86_64-pc-windows-msvc
rustup component add rustfmt clippy llvm-tools-preview
cargo install just
cargo install cargo-deny
cargo install cargo-audit
cargo install cargo-machete
cargo install cargo-llvm-cov
cargo check --workspace
```

If `cargo check` reports that `link.exe` or the Windows SDK cannot be found, repair the Visual Studio workload before changing registry or PATH settings.

## Starting Development

Build the whole workspace:

```sh
cargo build --workspace
```

Run the current application package:

```sh
cargo run -p mado
```

Run the visual demo application:

```sh
cargo run -p mado-demo
```

Use the smallest useful check while iterating:

```sh
just fmt
just clippy
just test-unit
```

Before handing off ordinary code changes, run:

```sh
just check
```

Let CI run the push and pull request gate:

```sh
just check-ci
```

Reserve the daily or explicitly requested deep gate for:

```sh
just check-full
```

This gate delegates bounded longer fuzz runs to `xtask`, which runs every registered nightly fuzz target.

Docs-only changes do not require Rust tests. Code changes should run at least the smallest relevant local command, and shared core behavior should run `just check` when practical.

## Troubleshooting

If a build fails because a Cargo command is missing, install only that missing tool and rerun the same command.

If Linux linking fails for X11, Wayland, fontconfig, ALSA, or OpenSSL, compare the installed packages with the Ubuntu package list in this document and the CI workflow.

If macOS linking fails after installing Xcode or Command Line Tools, run `xcode-select -p` and `clang --version`, then finish any license or installer prompts.

If Windows linking fails, verify the MSVC Rust toolchain first, then repair Visual Studio Build Tools with the C++ workload and Windows SDK.

## Agent Notes

This document is written for human contributors first. Agents should use it as a setup checklist, not as permission to perform every install automatically.

Agents may inspect the machine and run harmless verification commands, such as:

```sh
rustc --version
cargo --version
just --version
cargo llvm-cov --version
```

Agents may offer to install lightweight command-line tools only after user approval. Examples include Rust components, Cargo subcommands, and Linux packages.

Agents must stop and ask the user to manually install or confirm heavyweight platform toolchains before continuing. Do this for:

- Visual Studio or Visual Studio Build Tools on Windows.
- Xcode or Xcode Command Line Tools on macOS when the installer opens a GUI prompt.
- Scoop bootstrap, `winget` Visual Studio installs, or execution policy changes on Windows.
- Any package manager, IDE, SDK, or system update that requires account login, license acceptance, reboot, admin policy approval, or a large GUI installer.

After the user finishes a heavyweight install, agents should verify it with command-line checks instead of assuming it succeeded.
