use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub paths: PathsConfig,
    pub output: OutputConfig,
    pub fallbacks: FallbacksConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct PathsConfig {
    pub default_output_dir: String,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    pub file_extension: String,
    pub max_filename_length: usize,
    pub filename_pattern: String,
    pub index_padding: usize,
    pub file_template: String,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct FallbacksConfig {
    pub default_subject: String,
    pub default_from: String,
    pub default_date: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            paths: PathsConfig::default(),
            output: OutputConfig::default(),
            fallbacks: FallbacksConfig::default(),
        }
    }
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            default_output_dir: "./emails".to_string(),
        }
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            file_extension: "txt".to_string(),
            max_filename_length: 80,
            filename_pattern: "{index}_{subject}.{ext}".to_string(),
            index_padding: 5,
            file_template: "From: {from}\nDate: {date}\nSubject: {subject}\n---\n\n{body}"
                .to_string(),
        }
    }
}

impl Default for FallbacksConfig {
    fn default() -> Self {
        Self {
            default_subject: "No Subject".to_string(),
            default_from: "Unknown".to_string(),
            default_date: "Unknown".to_string(),
        }
    }
}

impl Config {
    /// Loads configuration from a TOML file. Falls back to defaults for any
    /// missing fields. Returns the built-in defaults if the file doesn't exist.
    pub fn load(path: &Path) -> Self {
        if !path.exists() {
            eprintln!(
                "  Info: no config file at {} — using built-in defaults",
                path.display()
            );
            return Self::default();
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "  Warning: could not read config file {}: {} — using defaults",
                    path.display(),
                    e
                );
                return Self::default();
            }
        };

        match toml::from_str(&content) {
            Ok(cfg) => {
                println!("Loaded config from {}", path.display());
                cfg
            }
            Err(e) => {
                eprintln!(
                    "  Warning: failed to parse config file {}: {} — using defaults",
                    path.display(),
                    e
                );
                Self::default()
            }
        }
    }

    /// Formats an output filename using the configured pattern.
    pub fn format_filename(&self, index: usize, sanitised_subject: &str) -> String {
        let padded_index = format!("{:0>width$}", index, width = self.output.index_padding);
        self.output
            .filename_pattern
            .replace("{index}", &padded_index)
            .replace("{subject}", sanitised_subject)
            .replace("{ext}", &self.output.file_extension)
    }

    /// Formats the output file content using the configured template.
    pub fn format_content(&self, from: &str, date: &str, subject: &str, body: &str) -> String {
        self.output
            .file_template
            .replace("{from}", from)
            .replace("{date}", date)
            .replace("{subject}", subject)
            .replace("{body}", body)
            .replace("\\n", "\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_default_config_values() {
        let config = Config::default();
        assert_eq!(config.paths.default_output_dir, "./emails");
        assert_eq!(config.output.file_extension, "txt");
        assert_eq!(config.output.max_filename_length, 80);
        assert_eq!(config.output.index_padding, 5);
        assert_eq!(config.fallbacks.default_subject, "No Subject");
        assert_eq!(config.fallbacks.default_from, "Unknown");
        assert_eq!(config.fallbacks.default_date, "Unknown");
    }

    #[test]
    fn test_format_filename() {
        let config = Config::default();
        let result = config.format_filename(42, "Hello World");
        assert_eq!(result, "00042_Hello World.txt");
    }

    #[test]
    fn test_format_filename_custom_pattern() {
        let mut config = Config::default();
        config.output.filename_pattern = "email-{index}-{subject}.{ext}".to_string();
        config.output.index_padding = 3;
        config.output.file_extension = "md".to_string();

        let result = config.format_filename(7, "Test");
        assert_eq!(result, "email-007-Test.md");
    }

    #[test]
    fn test_format_content() {
        let config = Config::default();
        let result = config.format_content("alice@test.com", "2025-01-01", "Hi", "Hello!");
        assert!(result.starts_with("From: alice@test.com\n"));
        assert!(result.contains("Subject: Hi\n"));
        assert!(result.contains("---\n\n"));
        assert!(result.ends_with("Hello!"));
    }

    #[test]
    fn test_format_content_custom_template() {
        let mut config = Config::default();
        config.output.file_template =
            "# {subject}\\n\\nFrom {from} on {date}\\n\\n{body}".to_string();

        let result = config.format_content("alice@test.com", "2025-01-01", "Hi", "Hello!");
        assert_eq!(result, "# Hi\n\nFrom alice@test.com on 2025-01-01\n\nHello!");
    }

    #[test]
    fn test_load_missing_file_returns_defaults() {
        let config = Config::load(Path::new("/nonexistent/config.toml"));
        assert_eq!(config.paths.default_output_dir, "./emails");
        assert_eq!(config.output.max_filename_length, 80);
    }

    #[test]
    fn test_load_partial_toml() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("partial.toml");
        fs::write(
            &config_path,
            r#"
[output]
file_extension = "md"
max_filename_length = 50
"#,
        )
        .unwrap();

        let config = Config::load(&config_path);
        // Overridden values
        assert_eq!(config.output.file_extension, "md");
        assert_eq!(config.output.max_filename_length, 50);
        // Remaining defaults preserved
        assert_eq!(config.paths.default_output_dir, "./emails");
        assert_eq!(config.fallbacks.default_subject, "No Subject");
        assert_eq!(config.output.index_padding, 5);
    }

    #[test]
    fn test_load_invalid_toml_returns_defaults() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("bad.toml");
        fs::write(&config_path, "this is not valid {{{{ toml").unwrap();

        let config = Config::load(&config_path);
        assert_eq!(config.paths.default_output_dir, "./emails");
        assert_eq!(config.output.file_extension, "txt");
    }

    #[test]
    fn test_format_filename_zero_index() {
        let config = Config::default();
        assert_eq!(config.format_filename(0, "Test"), "00000_Test.txt");
    }

    #[test]
    fn test_format_filename_large_index() {
        let config = Config::default();
        assert_eq!(
            config.format_filename(123456, "Test"),
            "123456_Test.txt"
        );
    }
}
