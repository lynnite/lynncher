# The first ss14 launcher written in rust

A cross-platform launcher for Space Station 14 written in Rust. It builds and runs natively on Linux, Windows and macOS.

## Current features

- Fully customizable UI, change color scheming, select an image for the backround
- Multiaccount
- Proxy for launcher traffic
- Basic event log and visible data paths
- Choose your hub server. default `https://hub.playss14.com/`
- Choose your auth server. default `https://auth.playss14.com/`
- Sideload content bundles (has not been tested yet so be careful)
- Auto-reconnect to last server disconnected setting
- Privacy changes
- Opt-in automatic updates
- Edgy logo
- Rust

## How to install

### Windows

Go to [releases](https://github.com/lynnite/lynncher/releases) and dowload the latest version (preferably)

Download the .exe and run it

### Linux (Debian/Ubuntu based)

Go to [releases](https://github.com/lynnite/lynncher/releases) and dowload the latest version (preferably)

Download the .deb, use a software manager to execute it,

Or install with apt:

    sudo apt install ./lynncher-linux-x86_64.deb 

Or install with dpkg
    
    sudo dpkg -i lynncher-linux-x86_64.deb 

## How to install dev/alternative to the packages if theres none for your OS

### Linux

Clone the repository or download a release from releases

    git clone https://github.com/lynnite/lynncher.git
    cd lynncher
    ./run-linux.sh

The script will:

1. Check if `cargo` and `rustc` are installed.
2. Install Rust via `rustup` if they are missing.
3. Detect and install the required X11/Wayland/OpenGL runtime libraries
   using your package manager (`apt`, `dnf`, `pacman` or `zypper`).
4. Build the launcher with `cargo build --release`.
5. Launch the program.

If the script is not executable yet:

    chmod +x run-linux.sh

### Windows

Clone the repository or download a release from releases

    git clone https://github.com/lynnite/lynncher.git
    cd lynncher
    run-windows.bat

The script will:

1. Check if `cargo` and `rustc` are installed.
2. Install Rust via `rustup` if they are missing (using `winget` first,
   falling back to downloading `rustup-init.exe`).
3. Build the launcher with `cargo build --release`.
4. Launch the program.

### Manual build

If you already have Rust installed and want to build manually:

    cargo build --release
    ./target/release/ss14-launcher-rust        # Linux / macOS
    # or
    .\target\release\ss14-launcher-rust.exe    # Windows

## Building a distributable package

### Linux (.deb)

To build a Debian/Ubuntu `.deb` package, you need `dpkg-deb` (usually
provided by the `dpkg-dev` package):

    sudo apt install dpkg-dev
    ./build-linux.sh

This produces `target/lynncher-<version>_amd64.deb` and copies it to
`lynncher-linux-x86_64.deb` locally. The package installs the launcher to
`/usr/bin/lynncher` together with a desktop entry and icon.

### Linux (.rpm)

To build an RPM package for Fedora/RHEL-based systems, you need `rpmbuild`
(provided by the `rpm-build` package):

    sudo dnf install rpm-build
    ./build-rpm.sh

This uses the spec file in `packaging/lynncher.spec.template`, produces the
`.rpm` under `~/rpmbuild/RPMS/`, and copies it to
`lynncher-linux-x86_64.rpm` locally.

### Windows (.exe)

Build on a Windows machine with Rust installed:

    build-windows.bat

This produces `target\release\ss14-launcher-rust.exe` and copies it to
`lynncher-windows-x86_64.exe` locally.

## Privacy

The launcher only contacts external services you explicitly use:

- The hub/auth servers you configure (e.g. `hub.playss14.com`) when you
  browse or connect to servers.
- GitHub's release API, only when you click **Check for updates**
  in the **Options** > **Updates** section. The launcher never contacts
  GitHub on startup or without your action, unless you specifically enabled this in the options.
