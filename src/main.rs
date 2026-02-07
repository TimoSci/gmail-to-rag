mod config;

use config::Config;
use mailparse::{parse_mail, MailHeaderMap};
use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Represents a single raw email extracted from an MBOX file.
struct RawMessage {
    bytes: Vec<u8>,
}

/// Reads an MBOX file and splits it into individual messages.
///
/// MBOX format uses lines starting with "From " as message delimiters.
/// We accumulate lines between these markers, skipping the separator line itself.
/// Lines that fail to read (I/O errors, corrupt bytes) are skipped with a warning.
fn read_mbox(path: &Path) -> Vec<RawMessage> {
    let file = fs::File::open(path).expect("Failed to open mbox file");
    let reader = BufReader::new(file);

    let mut messages: Vec<RawMessage> = Vec::new();
    let mut current: Vec<u8> = Vec::new();
    let mut line_num: usize = 0;

    for line in reader.split(b'\n') {
        line_num += 1;
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!(
                    "  Warning: failed to read line {} in {}: {} (skipping line)",
                    line_num,
                    path.display(),
                    e
                );
                continue;
            }
        };

        if line.starts_with(b"From ") && !current.is_empty() {
            messages.push(RawMessage {
                bytes: std::mem::take(&mut current),
            });
        } else {
            current.extend_from_slice(&line);
            current.push(b'\n');
        }
    }

    // Don't forget the last message
    if !current.is_empty() {
        messages.push(RawMessage { bytes: current });
    }

    messages
}

/// Extracts the plain-text body from a parsed email.
///
/// For multipart messages, walks all parts and returns the first text/plain part.
/// For single-part messages, returns the body directly.
fn extract_plain_text(mail: &mailparse::ParsedMail) -> String {
    if mail.subparts.is_empty() {
        return mail.get_body().unwrap_or_default();
    }

    for part in &mail.subparts {
        if let Some(ct) = part.get_content_disposition().params.get("content-type") {
            if ct.contains("text/plain") {
                return part.get_body().unwrap_or_default();
            }
        }
        // mailparse exposes ctype directly
        if part.ctype.mimetype == "text/plain" {
            return part.get_body().unwrap_or_default();
        }
        // Recurse into nested multipart
        let nested = extract_plain_text(part);
        if !nested.is_empty() {
            return nested;
        }
    }

    String::new()
}

/// Sanitises a string for use as a filename.
///
/// Keeps only alphanumeric characters, spaces, hyphens, and underscores.
/// Truncates to `max_length` characters.
fn sanitise_filename(s: &str, max_length: usize) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();

    cleaned.chars().take(max_length).collect()
}

fn main() {
    let args: Vec<String> = env::args().collect();

    // Parse optional --config flag
    let mut config_path: Option<PathBuf> = None;
    let mut positional: Vec<&str> = Vec::new();

    let mut i = 1;
    while i < args.len() {
        if args[i] == "--config" {
            if i + 1 < args.len() {
                config_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            } else {
                eprintln!("Error: --config requires a path argument");
                std::process::exit(1);
            }
        } else {
            positional.push(&args[i]);
            i += 1;
        }
    }

    if positional.is_empty() || positional.len() > 2 {
        eprintln!("Usage: mbox-to-text [--config config.toml] <path/to/mail.mbox> [output_dir]");
        eprintln!();
        eprintln!("Converts a Gmail MBOX export into individual text files");
        eprintln!("suitable for uploading to Open WebUI as a RAG knowledge base.");
        eprintln!();
        eprintln!("Arguments:");
        eprintln!("  <mbox_path>            Path to the .mbox file (required)");
        eprintln!("  [output_dir]           Directory for output files (default from config)");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  --config <path>        Path to config.toml (default: ./config.toml)");
        std::process::exit(1);
    }

    // Load config: explicit --config path > ./config.toml > built-in defaults
    let config_file = config_path.unwrap_or_else(|| PathBuf::from("./config.toml"));
    let config = Config::load(&config_file);

    let mbox_path = PathBuf::from(positional[0]);
    let output_dir = if positional.len() == 2 {
        PathBuf::from(positional[1])
    } else {
        PathBuf::from(&config.paths.default_output_dir)
    };

    if !mbox_path.exists() {
        eprintln!("Error: mbox file not found: {}", mbox_path.display());
        std::process::exit(1);
    }

    fs::create_dir_all(&output_dir).expect("Failed to create output directory");

    println!("Reading mbox file: {}", mbox_path.display());
    let messages = read_mbox(&mbox_path);
    println!("Found {} messages. Exporting...", messages.len());

    let mut exported = 0;
    let mut skipped = 0;

    for (i, raw) in messages.iter().enumerate() {
        let mail = match parse_mail(&raw.bytes) {
            Ok(m) => m,
            Err(e) => {
                eprintln!(
                    "  Warning: skipping message {} — MIME parse error: {}",
                    i, e
                );
                skipped += 1;
                continue;
            }
        };

        let headers = &mail.headers;
        let subject = headers
            .get_first_value("Subject")
            .unwrap_or_else(|| config.fallbacks.default_subject.clone());
        let from = headers
            .get_first_value("From")
            .unwrap_or_else(|| config.fallbacks.default_from.clone());
        let date = headers
            .get_first_value("Date")
            .unwrap_or_else(|| config.fallbacks.default_date.clone());

        let body = extract_plain_text(&mail);
        if body.is_empty() {
            eprintln!(
                "  Warning: message {} ({}) has no extractable plain-text body — exporting with empty body",
                i, subject
            );
        }

        let safe_subject = sanitise_filename(&subject, config.output.max_filename_length);
        let filename = config.format_filename(i, &safe_subject);
        let filepath = output_dir.join(&filename);
        let content = config.format_content(&from, &date, &subject, &body);

        if let Err(e) = fs::write(&filepath, &content) {
            eprintln!(
                "  Warning: skipping message {} — failed to write {}: {}",
                i, filename, e
            );
            skipped += 1;
            continue;
        }

        exported += 1;
    }

    if skipped > 0 {
        eprintln!(
            "\n⚠  {} message(s) were skipped due to errors (see warnings above)",
            skipped
        );
    }

    println!(
        "Done. Exported {}/{} emails to {}",
        exported,
        messages.len(),
        output_dir.display()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Path to the dummy mbox fixture shipped with the test suite.
    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("dummy.mbox")
    }

    /// Path to the mbox fixture containing a mix of valid and corrupt emails.
    fn corrupt_fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("with_corrupt.mbox")
    }

    /// Mirrors the main() export logic using a Config. Returns (exported, skipped) counts.
    fn export_messages(messages: &[RawMessage], output_dir: &Path) -> (usize, usize) {
        export_messages_with_config(messages, output_dir, &Config::default())
    }

    fn export_messages_with_config(
        messages: &[RawMessage],
        output_dir: &Path,
        config: &Config,
    ) -> (usize, usize) {
        let mut exported = 0;
        let mut skipped = 0;

        for (i, raw) in messages.iter().enumerate() {
            let mail = match parse_mail(&raw.bytes) {
                Ok(m) => m,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };

            let headers = &mail.headers;
            let subject = headers
                .get_first_value("Subject")
                .unwrap_or_else(|| config.fallbacks.default_subject.clone());
            let from = headers
                .get_first_value("From")
                .unwrap_or_else(|| config.fallbacks.default_from.clone());
            let date = headers
                .get_first_value("Date")
                .unwrap_or_else(|| config.fallbacks.default_date.clone());

            let body = extract_plain_text(&mail);
            let safe_subject = sanitise_filename(&subject, config.output.max_filename_length);
            let filename = config.format_filename(i, &safe_subject);
            let filepath = output_dir.join(&filename);
            let content = config.format_content(&from, &date, &subject, &body);

            if fs::write(&filepath, &content).is_err() {
                skipped += 1;
                continue;
            }

            exported += 1;
        }

        (exported, skipped)
    }

    // ---------------------------------------------------------------
    // read_mbox tests
    // ---------------------------------------------------------------

    #[test]
    fn test_read_mbox_splits_three_messages() {
        let messages = read_mbox(&fixture_path());
        assert_eq!(messages.len(), 3, "dummy.mbox should contain exactly 3 emails");
    }

    #[test]
    fn test_read_mbox_messages_are_parseable() {
        let messages = read_mbox(&fixture_path());
        for (i, raw) in messages.iter().enumerate() {
            assert!(
                parse_mail(&raw.bytes).is_ok(),
                "Message {} should parse without errors",
                i
            );
        }
    }

    // ---------------------------------------------------------------
    // Header extraction tests
    // ---------------------------------------------------------------

    #[test]
    fn test_simple_email_headers() {
        let messages = read_mbox(&fixture_path());
        let mail = parse_mail(&messages[0].bytes).unwrap();

        let from = mail.headers.get_first_value("From").unwrap();
        let subject = mail.headers.get_first_value("Subject").unwrap();
        let date = mail.headers.get_first_value("Date").unwrap();

        assert!(from.contains("alice@example.com"), "From should contain alice's address");
        assert_eq!(subject, "Q4 Budget Review");
        assert!(date.contains("2025"), "Date should contain the year");
    }

    #[test]
    fn test_multipart_email_headers() {
        let messages = read_mbox(&fixture_path());
        let mail = parse_mail(&messages[1].bytes).unwrap();

        let from = mail.headers.get_first_value("From").unwrap();
        let subject = mail.headers.get_first_value("Subject").unwrap();

        assert!(from.contains("github.com"));
        assert!(subject.contains("RAG upload fails"));
    }

    #[test]
    fn test_encoded_subject_header() {
        let messages = read_mbox(&fixture_path());
        let mail = parse_mail(&messages[2].bytes).unwrap();

        let subject = mail.headers.get_first_value("Subject").unwrap();
        assert!(
            subject.contains("Holiday Party RSVP"),
            "Encoded subject should decode to readable text, got: {}",
            subject
        );
    }

    // ---------------------------------------------------------------
    // extract_plain_text tests
    // ---------------------------------------------------------------

    #[test]
    fn test_extract_plain_text_simple() {
        let messages = read_mbox(&fixture_path());
        let mail = parse_mail(&messages[0].bytes).unwrap();
        let body = extract_plain_text(&mail);

        assert!(body.contains("Q4 budget review"), "Body should contain email text");
        assert!(body.contains("Revenue up 12%"), "Body should contain the key highlights");
    }

    #[test]
    fn test_extract_plain_text_multipart_prefers_plain() {
        let messages = read_mbox(&fixture_path());
        let mail = parse_mail(&messages[1].bytes).unwrap();
        let body = extract_plain_text(&mail);

        assert!(body.contains("A new issue has been opened"), "Should extract plain text part");
        assert!(
            !body.contains("<html>"),
            "Should NOT contain HTML tags — plain text part should be preferred"
        );
    }

    #[test]
    fn test_extract_plain_text_third_email() {
        let messages = read_mbox(&fixture_path());
        let mail = parse_mail(&messages[2].bytes).unwrap();
        let body = extract_plain_text(&mail);

        assert!(body.contains("RSVP for the annual holiday party"));
        assert!(body.contains("The Grand Ballroom"));
    }

    // ---------------------------------------------------------------
    // sanitise_filename tests
    // ---------------------------------------------------------------

    #[test]
    fn test_sanitise_simple_subject() {
        assert_eq!(sanitise_filename("Q4 Budget Review", 80), "Q4 Budget Review");
    }

    #[test]
    fn test_sanitise_special_characters() {
        let result = sanitise_filename("[open-webui/open-webui] Issue #4521: Fails!", 80);
        assert_eq!(result, "_open-webui_open-webui_ Issue _4521_ Fails_");
    }

    #[test]
    fn test_sanitise_truncates_at_configured_length() {
        let long_subject = "A".repeat(120);
        assert_eq!(sanitise_filename(&long_subject, 80).len(), 80);
        assert_eq!(sanitise_filename(&long_subject, 50).len(), 50);
        assert_eq!(sanitise_filename(&long_subject, 120).len(), 120);
    }

    #[test]
    fn test_sanitise_empty_string() {
        assert_eq!(sanitise_filename("", 80), "");
    }

    #[test]
    fn test_sanitise_unicode_emoji() {
        let result = sanitise_filename("🎉 Holiday Party RSVP - Don't Miss Out!", 80);
        assert!(result.contains("Holiday Party RSVP"));
        assert!(!result.contains("🎉"));
    }

    // ---------------------------------------------------------------
    // Corrupt email handling tests
    // ---------------------------------------------------------------

    #[test]
    fn test_corrupt_mbox_does_not_panic() {
        let messages = read_mbox(&corrupt_fixture_path());
        assert!(
            messages.len() >= 2,
            "Should split at least the valid messages from the corrupt mbox, got {}",
            messages.len()
        );
    }

    #[test]
    fn test_corrupt_messages_are_skipped_during_export() {
        let messages = read_mbox(&corrupt_fixture_path());
        let output_dir = TempDir::new().unwrap();
        let (exported, _skipped) = export_messages(&messages, output_dir.path());

        assert!(
            exported >= 2,
            "Should export at least the 2 clearly valid emails, got {}",
            exported
        );
    }

    #[test]
    fn test_valid_emails_survive_after_corruption() {
        let messages = read_mbox(&corrupt_fixture_path());
        let output_dir = TempDir::new().unwrap();
        export_messages(&messages, output_dir.path());

        let files: Vec<_> = fs::read_dir(output_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();

        let found_post_corruption = files.iter().any(|entry| {
            let content = fs::read_to_string(entry.path()).unwrap_or_default();
            content.contains("Valid email after corruption")
                || content.contains("should still be exported")
        });

        assert!(
            found_post_corruption,
            "The valid email after the corrupt ones should have been exported"
        );
    }

    #[test]
    fn test_export_counts_are_consistent() {
        let messages = read_mbox(&corrupt_fixture_path());
        let output_dir = TempDir::new().unwrap();
        let (exported, skipped) = export_messages(&messages, output_dir.path());

        assert_eq!(
            exported + skipped,
            messages.len(),
            "exported ({}) + skipped ({}) should equal total messages ({})",
            exported,
            skipped,
            messages.len()
        );

        let file_count = fs::read_dir(output_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .count();
        assert_eq!(
            file_count, exported,
            "Number of files on disk should match exported count"
        );
    }

    #[test]
    fn test_purely_corrupt_bytes_are_skipped() {
        let messages = vec![RawMessage {
            bytes: b"This is not an email at all\xff\xfe\x00\x00just garbage bytes".to_vec(),
        }];

        let output_dir = TempDir::new().unwrap();
        let (exported, skipped) = export_messages(&messages, output_dir.path());

        assert_eq!(
            exported + skipped,
            1,
            "Should account for the single message"
        );
    }

    // ---------------------------------------------------------------
    // End-to-end integration tests
    // ---------------------------------------------------------------

    #[test]
    fn test_full_export_pipeline() {
        let messages = read_mbox(&fixture_path());
        let output_dir = TempDir::new().expect("Failed to create temp dir");

        let (exported, skipped) = export_messages(&messages, output_dir.path());

        assert_eq!(exported, 3);
        assert_eq!(skipped, 0);

        let files: Vec<_> = fs::read_dir(output_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 3, "Should have written 3 .txt files");

        let first_file = output_dir.path().join("00000_Q4 Budget Review.txt");
        assert!(first_file.exists(), "First output file should exist");

        let content = fs::read_to_string(&first_file).unwrap();
        assert!(content.starts_with("From: "), "File should start with From: header");
        assert!(content.contains("Subject: Q4 Budget Review"));
        assert!(content.contains("---\n\n"), "File should contain the separator");
        assert!(content.contains("Revenue up 12%"), "File should contain email body");
    }

    #[test]
    fn test_export_with_custom_config() {
        let messages = read_mbox(&fixture_path());
        let output_dir = TempDir::new().unwrap();

        let mut config = Config::default();
        config.output.file_extension = "md".to_string();
        config.output.filename_pattern = "email-{index}-{subject}.{ext}".to_string();
        config.output.index_padding = 3;
        config.output.file_template =
            "# {subject}\\nFrom: {from}\\nDate: {date}\\n\\n{body}".to_string();

        let (exported, _) = export_messages_with_config(&messages, output_dir.path(), &config);
        assert_eq!(exported, 3);

        // Check that custom filename pattern was used
        let first_file = output_dir.path().join("email-000-Q4 Budget Review.md");
        assert!(
            first_file.exists(),
            "File should use custom pattern: {:?}",
            fs::read_dir(output_dir.path())
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect::<Vec<_>>()
        );

        // Check that custom template was used
        let content = fs::read_to_string(&first_file).unwrap();
        assert!(content.starts_with("# Q4 Budget Review\n"), "Should use markdown heading template");
    }

    #[test]
    fn test_output_files_are_valid_for_rag_upload() {
        let messages = read_mbox(&fixture_path());
        let output_dir = TempDir::new().unwrap();

        export_messages(&messages, output_dir.path());

        for entry in fs::read_dir(output_dir.path()).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            assert!(
                path.extension().map_or(false, |ext| ext == "txt"),
                "All output files should be .txt"
            );

            let content = fs::read_to_string(&path).unwrap();
            let lines: Vec<&str> = content.lines().collect();

            assert!(
                lines.len() >= 4,
                "Each file needs at least From, Date, Subject, and separator lines"
            );
            assert!(lines[0].starts_with("From: "), "Line 1 should be From:");
            assert!(lines[1].starts_with("Date: "), "Line 2 should be Date:");
            assert!(lines[2].starts_with("Subject: "), "Line 3 should be Subject:");
            assert_eq!(lines[3], "---", "Line 4 should be the separator");
        }
    }
}
