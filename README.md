# Low-Bandwidth Educational Access Platform

[AID India](https://aidindia.in/) helps to bring education and educational resources to children in remote villages of India.
These villages often face challenges with the stability of their internet connections. In such an environment,
serving quality lectures can be challenging due to constraints of bandwidth, cost and latency.

The `Low-Bandwidth Educational Access Platform` (`LEAP`) is the result of a collaboration between AID India and T4EQ in an
attempt to bring quality educational videos to these children by caching the content locally at the
point where the data is served.

The `LEAP` system consists of:
- A frontend video player
- A backend serving locally cached video content to devices in a local network.
- A management API to initiate the download of video content from remote servers,
as well as managing the local content available in LEAP.

`LEAP` is designed to run in low power devices such as raspberry pi and similar SBCs, and is
tailored such that it uses minimal resources during runtime. The default build targets
a Raspberry Pi 4 with 1GiB of RAM.

## Provisioning

On first boot, or if no external storage device is detected, the device boots into a provisioning mode. 
In this mode, it starts a dedicated wireless hotspot named `LEAP-setup`.

Connecting to this hotspot allows you to access a web-based configuration interface (implemented via 
the `leap-provision-site` crate) to perform the following tasks:

- **Select External Storage**: Choose the external drive to be used for configuration and data 
storage. The system SD card remains read-only to increase the lifetime of the card.
- **Configure Networking**: Set up network connectivity options.
- **Configure LEAP Options**: Set LEAP-specific parameters, such as the remote S3 server bucket and 
retry parameters.

## Getting started

Clone the repository with:

```sh
git clone --recurse-submodules git@github.com:T4EQ/leap.git
```

This will create a `leap` directory inside your current working directory.
The layout of this directory is as follows:

```
.
├── README.md           -> This file. Contains information about the project.
├── leap-api            -> A library crate containing data types that are shared between frontend and backend.
├── leap-linux          -> The full Linux image build definition using Buildroot and Nix.
├── leap-provision-site -> The web-based configuration interface crate used during provisioning.
├── leap-server         -> The main web server application crate, build using actix-web.
├── leap-site           -> The frontend application crate, built using Yew. It is built as a wasm application and embedded into the leap-server binary.
│   ├── index.html      -> The entrypoint of the single-page web application. The body tag is populated from the Yew app.
│   ├── index.scss      -> The sylesheet applicable to the whole frontend.
│   └── ...             -> Other files belonging a regular Rust crate.
├── xtask               -> A binary crate that implements tasks to build and run the whole project.
├── flake.nix           -> Declares the dependencies of the project in order to perform reproducible builds. You can safely ignore this file.
├── flake.lock          -> Pins the dependencies of the repository. You can safely ignore this file.
└── .cargo
   └── config.toml      -> Configuration file defining cargo aliases.
```

### Dependencies

This website relies on the following software:
- `Rust`, `v1.95`. It is recommended you install a toolchain using [rustup](rustup.rs), to help you manage multiple installs.
  Alternatively, using the `nix develop` environment will include the right versions out of the box.
- `trunk`, `v0.21` application bundler. See install instructions [here](https://trunkrs.dev/) 
- _Optional:_ `Nix` package manager. See install instructions [here](https://nixos.org/download/).

The `Nix` package manager is used to ensure that the build results are fully reproducible across 
all machines. When using `Nix`, you don't need to worry about installing other tools separately. 
It is mostly intended for continuous integration/deployment in GitHub, so you may decide to skip 
its installation for your local development setup.

### Build with Cargo

To build the web server simply run:

```sh
# Build in debug mode:
cargo b

# Build in release mode:
cargo br
```

Note that `b` and `br` are cargo aliases defined in `.cargo/config.toml`. These aliases invoke `xtask` 
with the corresponding arguments to trigger a build. You can read more about the `xtask` pattern 
[here](https://github.com/matklad/cargo-xtask).

You will find the build under `target/{debug,release}/leap-server` depending on the build profile you used.
This is a self-contained binary that contains the frontend build inside itself.

Alternatively, you can serve the web server locally on port `8080` using:

```sh
# Run in debug mode:
cargo r

# Run in release mode:
cargo rr
```

Note that this command does not terminate, but keeps serving the website at `http://localhost:8080`. 

For development, you might want to use [`cargo-watch`](https://crates.io/crates/cargo-watch) to watch 
all file changes and automatically rebuild the `leap-server` (and both `leap-site` and `leap-provision-site`) 
when they change, so that simply refreshing the web browser page displays the latest edits. 

```sh
cargo watch -x 'rr'
```

### Build with Nix

As mentioned above, this setup is mainly intended for continuous integration/deployment. However, 
if you have the `Nix` package manager installed in your system, it will automatically ensure you 
get the right tool versions out of the box. 

You can build the `leap-server` with:

```sh
nix build --extra-experimental-features 'nix-command flakes' .
```

And find the result under the `result/bin/leap-server` path.

Or you can enter a development shell with the full environment preconfigured and run the build steps 
listed in [Build with Cargo](#build-with-cargo), using:

```sh
nix develop --extra-experimental-features 'nix-command flakes' .
```

See [direnv](https://direnv.net/) to automatically execute `nix develop` when you change your working directory
to a subdirectory inside this repository.

Note that you do _not_ need `Nix` to use the right versions of the rust toolchain. Installing `Rust` 
via `rustup.rs` will make sure you get the right toolchain (indicated in the `rust-toolchain.toml`) 
file every time you run cargo.

### Build with Nix for the target

```sh
nix build --extra-experimental-features 'nix-command flakes' .#leap-target
```

A static binary will be present under the `result/bin/leap-server` path, compiled for the aarch64 architecture.

Note that this build can be particularly slow if you are building on an x64 computer, because it is 
cross compiling to aarch64 and running aarch64 binaries. In order to run aarch64 binaries, you might 
need to register an interpreter for aarch64 elf binaries using [binfmt](https://man.archlinux.org/man/binfmt.d.5.en).

### Build the Linux Image

To build the full Linux image, use:

```sh
nix build --extra-experimental-features 'nix-command flakes' .#leap-linux
```
