# Prerequisites 

## Rust

If you don't have Rust installed yet:

`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.zshrc`


# Usage

## 1. Navigate into the project
cd mbox-to-text

## 2. Build the release binary
cargo build --release

## 3. Run it against your mbox file
./target/release/mbox-to-text /path/to/your/mail.mbox ./emails
