default:
    @just --list

binaries:
    rm binaries -r || true
    mkdir binaries
    cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.28
    cp target/x86_64-unknown-linux-gnu/release/vthumb binaries/linux_vthumb_cli
    cp target/x86_64-unknown-linux-gnu/release/vthumb-gui binaries/linux_vthumb_gui

    cargo build --release --target x86_64-pc-windows-gnu
    cp target/x86_64-pc-windows-gnu/release/vthumb.exe binaries/windows_vthumb_cli.exe
    cp target/x86_64-pc-windows-gnu/release/vthumb-gui.exe binaries/windows_vthumb_gui.exe

gui *ARGS:
    cargo run -p thumbnailer-gui -- {{ARGS}}

cli *ARGS:
    cargo run -p thumbnailer-cli -- {{ARGS}}

build:
    cargo build --workspace

release:
    cargo build --workspace --release

clippy:
    cargo clippy --workspace --all-targets --all-features

fix:
    cargo clippy --workspace --all-targets --all-features --fix --allow-dirty --allow-staged

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

check: fmt-check clippy

clean:
    cargo clean

tree:
    cargo tree --workspace
