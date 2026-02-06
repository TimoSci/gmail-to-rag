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
fn read_mbox(path: &Path) -> Vec<RawMessage> {
    let file = fs::File::open(path).expect("Failed to open mbox file");
    let reader = BufReader::new(file);

    let mut messages: Vec<RawMessage> = Vec::new();
    let mut current: Vec<u8> = Vec::new();

    for line in reader.split(b'\n') {
        let line = line.expect("Failed to read line");

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

    for (i, raw) in messages.iter().enumerate() {
        let mail = match parse_mail(&raw.bytes) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("  Skipping message {}: parse error: {}", i, e);
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

        if let Err(e) = fs::write(&filepath, &content) {
            eprintln!("  Failed to write {}: {}", filename, e);
            continue;
        }

        exported += 1;
    }

    println!(
        "Done. Exported {} emails to {}",
        exported,
        output_dir.display()
    );
}
