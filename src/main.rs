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
/// Truncates to 80 characters.
fn sanitise_filename(s: &str) -> String {
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

    cleaned.chars().take(80).collect()
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 || args.len() > 3 {
        eprintln!("Usage: mbox-to-text <path/to/mail.mbox> [output_dir]");
        eprintln!();
        eprintln!("Converts a Gmail MBOX export into individual .txt files");
        eprintln!("suitable for uploading to Open WebUI as a RAG knowledge base.");
        eprintln!();
        eprintln!("Arguments:");
        eprintln!("  <mbox_path>    Path to the .mbox file (required)");
        eprintln!("  [output_dir]   Directory for output files (default: ./emails)");
        std::process::exit(1);
    }

    let mbox_path = PathBuf::from(&args[1]);
    let output_dir = if args.len() == 3 {
        PathBuf::from(&args[2])
    } else {
        PathBuf::from("./emails")
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
            .unwrap_or_else(|| "No Subject".to_string());
        let from = headers
            .get_first_value("From")
            .unwrap_or_else(|| "Unknown".to_string());
        let date = headers
            .get_first_value("Date")
            .unwrap_or_else(|| "Unknown".to_string());

        let body = extract_plain_text(&mail);
        if body.is_empty() {
            eprintln!(
                "  Warning: message {} ({}) has no extractable plain-text body — exporting with empty body",
                i, subject
            );
        }

        let safe_subject = sanitise_filename(&subject);
        let filename = format!("{:05}_{}.txt", i, safe_subject);
        let filepath = output_dir.join(&filename);

        let content = format!(
            "From: {}\nDate: {}\nSubject: {}\n---\n\n{}",
            from, date, subject, body
        );

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

    /// Mirrors the main() export logic. Returns (exported, skipped) counts.
    fn export_messages(messages: &[RawMessage], output_dir: &Path) -> (usize, usize) {
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
                .unwrap_or_else(|| "No Subject".to_string());
            let from = headers
                .get_first_value("From")
                .unwrap_or_else(|| "Unknown".to_string());
            let date = headers
                .get_first_value("Date")
                .unwrap_or_else(|| "Unknown".to_string());

            let body = extract_plain_text(&mail);
            let safe_subject = sanitise_filename(&subject);
            let filename = format!("{:05}_{}.txt", i, safe_subject);
            let filepath = output_dir.join(&filename);

            let content = format!(
                "From: {}\nDate: {}\nSubject: {}\n---\n\n{}",
                from, date, subject, body
            );

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
        // The subject is Base64-encoded UTF-8; mailparse should decode it.
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

        // Should get the text/plain part, not the HTML
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
        assert_eq!(sanitise_filename("Q4 Budget Review"), "Q4 Budget Review");
    }

    #[test]
    fn test_sanitise_special_characters() {
        let result = sanitise_filename("[open-webui/open-webui] Issue #4521: Fails!");
        assert_eq!(result, "_open-webui_open-webui_ Issue _4521_ Fails_");
    }

    #[test]
    fn test_sanitise_truncates_at_80_chars() {
        let long_subject = "A".repeat(120);
        let result = sanitise_filename(&long_subject);
        assert_eq!(result.len(), 80);
    }

    #[test]
    fn test_sanitise_empty_string() {
        assert_eq!(sanitise_filename(""), "");
    }

    #[test]
    fn test_sanitise_unicode_emoji() {
        // Emoji like 🎉 should be replaced with underscores
        let result = sanitise_filename("🎉 Holiday Party RSVP - Don't Miss Out!");
        assert!(result.contains("Holiday Party RSVP"));
        assert!(!result.contains("🎉"));
    }

    // ---------------------------------------------------------------
    // Corrupt email handling tests
    // ---------------------------------------------------------------

    #[test]
    fn test_corrupt_mbox_does_not_panic() {
        // The most important test: reading an mbox with corrupt messages
        // must not panic — it should return whatever it can split.
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

        // We expect at least the first and last valid emails to export.
        // The corrupt/minimal ones may or may not parse — the key point
        // is that we don't crash and we do get the good ones.
        assert!(
            exported >= 2,
            "Should export at least the 2 clearly valid emails, got {}",
            exported
        );
    }

    #[test]
    fn test_valid_emails_survive_after_corruption() {
        // Verifies that emails appearing AFTER corrupt ones are still exported.
        let messages = read_mbox(&corrupt_fixture_path());
        let output_dir = TempDir::new().unwrap();
        export_messages(&messages, output_dir.path());

        // Look for the "Valid email after corruption" file
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

        // exported + skipped should equal the total number of raw messages
        assert_eq!(
            exported + skipped,
            messages.len(),
            "exported ({}) + skipped ({}) should equal total messages ({})",
            exported,
            skipped,
            messages.len()
        );

        // The number of files on disk should match the exported count
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
        // Simulate a completely unparseable message
        let messages = vec![RawMessage {
            bytes: b"This is not an email at all\xff\xfe\x00\x00just garbage bytes".to_vec(),
        }];

        let output_dir = TempDir::new().unwrap();
        let (exported, skipped) = export_messages(&messages, output_dir.path());

        // It should either skip it or export it with best-effort headers.
        // The critical thing is it must not panic.
        assert_eq!(
            exported + skipped,
            1,
            "Should account for the single message"
        );
    }

    // ---------------------------------------------------------------
    // End-to-end integration test
    // ---------------------------------------------------------------

    #[test]
    fn test_full_export_pipeline() {
        let messages = read_mbox(&fixture_path());
        let output_dir = TempDir::new().expect("Failed to create temp dir");

        let (exported, skipped) = export_messages(&messages, output_dir.path());

        // Verify correct counts
        assert_eq!(exported, 3);
        assert_eq!(skipped, 0);

        // Verify files exist on disk
        let files: Vec<_> = fs::read_dir(output_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 3, "Should have written 3 .txt files");

        // Verify first file content
        let first_file = output_dir.path().join("00000_Q4 Budget Review.txt");
        assert!(first_file.exists(), "First output file should exist");

        let content = fs::read_to_string(&first_file).unwrap();
        assert!(content.starts_with("From: "), "File should start with From: header");
        assert!(content.contains("Subject: Q4 Budget Review"));
        assert!(content.contains("---\n\n"), "File should contain the separator");
        assert!(content.contains("Revenue up 12%"), "File should contain email body");
    }

    #[test]
    fn test_output_files_are_valid_for_rag_upload() {
        // Ensures the output format matches what Open WebUI expects:
        // structured metadata header followed by body text.
        let messages = read_mbox(&fixture_path());
        let output_dir = TempDir::new().unwrap();

        export_messages(&messages, output_dir.path());

        // Read each output file and verify structure
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
