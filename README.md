# Gmail-To-RAG

# About

This fast parsing tool converts a Gmail Takeout `.mbox` archive 
to a folder containig individual email `.txt` files, suitable for importing as
a Retrieval-Augmented Generation (RAG) Knowledge in Open WebUI.

see: https://docs.openwebui.com/

Performance tested on an Apple M5. Takes ~10 seconds to parse a 7GB mbox archive 
containing 50'000 emails.

Graceful failure: Corrupt emails cause warnings and are skipped.

# Prerequisites 

## Rust

If you don't have Rust installed yet, install via Rustup:
`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`


# Usage

## Optional: Change defaults in configuration file
Edit `Config.toml`

## 1. Navigate into the project
`cd gmail-to-rag`

## 2. Build the release binary
`cargo build --release`

## 3. Run it against your mbox file
`./target/release/gmail-to-rag /path/to/your/mail.mbox ./emails`

## 4. Import into Open WebUI

Workspace > Knowledge Tab > Create Knowledge > Add Content > Upload Directory > [select `emails` directory created in step 3]
