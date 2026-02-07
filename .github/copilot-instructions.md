# Copilot instructions

## Project overview
- Single-binary Rust CLI that converts a Gmail Takeout .mbox into per‑email .txt files for Open WebUI RAG import; all core logic lives in [src/main.rs](src/main.rs).
- Pipeline: read MBOX → parse MIME → extract text/plain → write formatted .txt with header + body. See `read_mbox()`, `extract_plain_text()`, and the export loop in [src/main.rs](src/main.rs).
- Output format is fixed: first three lines `From:`, `Date:`, `Subject:`, then a separator `---`, then body text. Tests assert this in [src/main.rs](src/main.rs).

## Key implementation details
- MBOX splitting is line-based on lines starting with `From `; corrupt lines are skipped with warnings, not hard failures. See `read_mbox()` in [src/main.rs](src/main.rs).
- MIME parsing uses `mailparse` and prefers the first `text/plain` part, recursing into multipart subparts. See `extract_plain_text()` in [src/main.rs](src/main.rs).
- Filenames are derived from subject: `{:05}_<sanitized>.txt` with sanitization limited to alnum/space/hyphen/underscore and truncated to 80 chars. See `sanitise_filename()` and export loop in [src/main.rs](src/main.rs).
- Corrupt messages are skipped; later valid emails must still export. This behavior is captured by fixtures in [tests/fixtures](tests/fixtures) and tests in [src/main.rs](src/main.rs).

## Developer workflows
- Build release binary as documented in [README.md](README.md): `cargo build --release` then run the binary against an .mbox file. The README example uses `./target/release/mbox-to-text`.
- Run tests with `cargo test`; tests and fixtures are embedded in [src/main.rs](src/main.rs) and [tests/fixtures](tests/fixtures).

## Conventions to follow
- Keep all CLI behavior and parsing logic in the single file [src/main.rs](src/main.rs) unless adding a new module is necessary.
- Preserve the exact output file structure and header order because tests and downstream RAG import depend on it.
- When handling errors, favor warnings + skipping corrupt items rather than failing the whole export (see existing warning patterns in [src/main.rs](src/main.rs)).
