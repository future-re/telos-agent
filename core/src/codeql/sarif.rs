//! SARIF 2.1 parser — converts CodeQL JSON output into structured results and
//! [`MemoryEntry`] records.
//!
//! Handles the subset of SARIF that CodeQL produces: runs → results → locations,
//! with rule metadata from `tool.driver.rules`.

use serde::Serialize;
use serde_json::Value;

use crate::memory::format::{MemoryCategory, MemoryEntry, MemoryStatus};
use crate::memory::index::unix_timestamp;

/// A single finding extracted from a SARIF report.
#[derive(Debug, Clone, Serialize)]
pub struct SarifResult {
    /// The CodeQL rule identifier, e.g. `rust/excessive-parameter-list`.
    pub rule_id: String,
    /// Severity level: `"error"`, `"warning"`, or `"note"`.
    pub severity: String,
    /// Human-readable description of the finding.
    pub message: String,
    /// Relative file path within the project.
    pub file_path: String,
    /// 1-based line number where the finding starts.
    pub start_line: u32,
    /// 1-based column (if available).
    pub start_column: Option<u32>,
    /// 1-based end line (if available).
    pub end_line: Option<u32>,
    /// Code snippet at the finding location, if present in the SARIF.
    pub code_snippet: Option<String>,
}

impl SarifResult {
    /// Produce a stable, deterministic name suitable for use as a
    /// [`MemoryEntry::name`].  Uses the rule id and a hash of the canonical
    /// location so that re-running the same analysis updates the existing entry
    /// rather than creating a duplicate.
    pub fn stable_name(&self) -> String {
        let loc = format!("{}:{}:{}", self.rule_id, self.file_path, self.start_line);
        format!("codeql-{}-{}", self.rule_id.to_lowercase(), stable_id(&loc))
    }

    /// Convert this finding into a [`MemoryEntry`] suitable for persistence.
    pub fn to_memory_entry(&self, source_session: Option<&str>) -> MemoryEntry {
        let ts = unix_timestamp();
        let status =
            if self.severity == "error" { MemoryStatus::NeedsFix } else { MemoryStatus::Working };
        let mut body = format!(
            "## CodeQL Finding: {}\n\n**Severity**: {}\n\n**Location**: `{}`",
            self.rule_id, self.severity, self.file_path
        );
        if let Some(line) = self.start_column {
            body.push_str(&format!(":{}:{}", self.start_line, line));
        } else {
            body.push_str(&format!(":{}", self.start_line));
        }
        if let Some(el) = self.end_line
            && el > self.start_line
        {
            body.push_str(&format!("-{el}"));
        }
        body.push_str("\n\n");
        body.push_str(&self.message);
        if let Some(snippet) = &self.code_snippet {
            body.push_str(&format!("\n\n```\n{}\n```", snippet.trim()));
        }

        MemoryEntry {
            name: self.stable_name(),
            description: truncate(&self.message, 120),
            category: MemoryCategory::Fact,
            tags: vec!["codeql".into(), "analyzed".into(), self.severity.clone()],
            created: ts.clone(),
            updated: ts,
            status,
            times_used: 0,
            confidence: Some("high".into()),
            related: vec![],
            source_session: source_session.map(String::from),
            body,
        }
    }
}

/// Parses SARIF 2.1 JSON and extracts a deduplicated, filtered list of findings.
pub struct SarifParser;

impl SarifParser {
    /// Parse a raw SARIF JSON string and return findings up to `max_results`.
    ///
    /// Results are deduplicated by `(rule_id, file_path, start_line)` before
    /// being truncated.
    pub fn parse(sarif_json: &str, max_results: usize) -> Result<Vec<SarifResult>, String> {
        let root: Value =
            serde_json::from_str(sarif_json).map_err(|e| format!("invalid SARIF JSON: {e}"))?;

        let runs = root
            .get("runs")
            .and_then(|v| v.as_array())
            .ok_or("missing 'runs' array in SARIF output")?;

        let mut results: Vec<SarifResult> = Vec::new();

        for run in runs {
            // Extract rule metadata: rule_index → rule_id
            let rules: Vec<Option<String>> = run
                .get("tool")
                .and_then(|t| t.get("driver"))
                .and_then(|d| d.get("rules"))
                .and_then(|r| r.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(|rule| rule.get("id").and_then(|v| v.as_str()).map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            let run_results = match run.get("results").and_then(|v| v.as_array()) {
                Some(arr) => arr,
                None => continue,
            };

            for result in run_results {
                if results.len() >= max_results {
                    break;
                }

                let rule_id = result
                    .get("ruleId")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .or_else(|| {
                        result
                            .get("ruleIndex")
                            .and_then(|v| v.as_u64())
                            .and_then(|idx| rules.get(idx as usize).cloned().flatten())
                    })
                    .unwrap_or_else(|| "unknown".into());

                let severity = result
                    .get("level")
                    .and_then(|v| v.as_str())
                    .unwrap_or("warning")
                    .to_lowercase();

                let message = result
                    .get("message")
                    .and_then(|m| m.get("text"))
                    .and_then(|v| v.as_str())
                    .or_else(|| result.get("shortMessage").and_then(|v| v.as_str()))
                    .unwrap_or("")
                    .to_string();

                let location = result
                    .get("locations")
                    .and_then(|v| v.as_array())
                    .and_then(|locs| locs.first());

                let (file_path, start_line, start_column, end_line, code_snippet) =
                    if let Some(loc) = location {
                        let phys = loc.get("physicalLocation");
                        let fp = phys
                            .and_then(|p| p.get("artifactLocation"))
                            .and_then(|a| a.get("uri"))
                            .and_then(|v| v.as_str())
                            .map(strip_file_prefix)
                            .unwrap_or_else(|| "<unknown>".into());

                        let region = phys.and_then(|p| p.get("region"));
                        let sl = region
                            .and_then(|r| r.get("startLine"))
                            .and_then(|v| v.as_u64())
                            .map(|v| v as u32)
                            .unwrap_or(1);
                        let sc = region
                            .and_then(|r| r.get("startColumn"))
                            .and_then(|v| v.as_u64())
                            .map(|v| v as u32);
                        let el = region
                            .and_then(|r| r.get("endLine"))
                            .and_then(|v| v.as_u64())
                            .map(|v| v as u32);

                        let snippet = region
                            .and_then(|r| r.get("snippet"))
                            .and_then(|s| s.get("text"))
                            .and_then(|v| v.as_str())
                            .map(String::from);

                        (fp, sl, sc, el, snippet)
                    } else {
                        ("<unknown>".into(), 1, None, None, None)
                    };

                results.push(SarifResult {
                    rule_id,
                    severity,
                    message,
                    file_path,
                    start_line,
                    start_column,
                    end_line,
                    code_snippet,
                });
            }
        }

        Ok(Self::deduplicate(results))
    }

    /// Deduplicate results by composite key `(rule_id, file_path, start_line)`.
    /// When duplicates are found, keeps the one with higher severity.
    pub fn deduplicate(mut results: Vec<SarifResult>) -> Vec<SarifResult> {
        let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut deduped: Vec<SarifResult> = Vec::new();

        for result in results.drain(..) {
            let key = format!("{}:{}:{}", result.rule_id, result.file_path, result.start_line);
            if let Some(&idx) = seen.get(&key) {
                // Keep the higher-severity finding.
                if severity_rank(&result.severity) < severity_rank(&deduped[idx].severity) {
                    deduped[idx] = result;
                }
            } else {
                seen.insert(key, deduped.len());
                deduped.push(result);
            }
        }

        deduped
    }
}

/// Strip common file URI prefixes that CodeQL may emit.
fn strip_file_prefix(uri: &str) -> String {
    uri.strip_prefix("file://").or_else(|| uri.strip_prefix("file:/")).unwrap_or(uri).to_string()
}

/// Rank severities for deduplication: lower = more important.
fn severity_rank(severity: &str) -> u8 {
    match severity {
        "error" => 0,
        "warning" => 1,
        "note" => 2,
        _ => 3,
    }
}

/// Truncate a string to `max` chars, adding `…` when truncated.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{truncated}…")
    }
}

/// FNV-1a 64-bit hash — produces a stable hex string for deduplicated naming.
fn stable_id(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal valid SARIF with one error finding.
    const SARIF_ONE_ERROR: &str = r#"{
  "version": "2.1.0",
  "runs": [{
    "tool": {
      "driver": {
        "name": "CodeQL",
        "rules": [
          {"id": "rust/path-injection", "shortDescription": {"text": "Path injection vulnerability"}}
        ]
      }
    },
    "results": [{
      "ruleId": "rust/path-injection",
      "ruleIndex": 0,
      "level": "error",
      "message": {"text": "Unsanitized user input flows to file-system access."},
      "locations": [{
        "physicalLocation": {
          "artifactLocation": {"uri": "src/main.rs"},
          "region": {"startLine": 42, "startColumn": 10}
        }
      }]
    }]
  }]
}"#;

    /// Two findings — one error, one warning.
    const SARIF_TWO_MIXED: &str = r#"{
  "version": "2.1.0",
  "runs": [{
    "tool": {
      "driver": {
        "name": "CodeQL",
        "rules": [
          {"id": "rust/path-injection"},
          {"id": "rust/unused-import"}
        ]
      }
    },
    "results": [
      {
        "ruleId": "rust/path-injection",
        "ruleIndex": 0,
        "level": "error",
        "message": {"text": "Path injection."},
        "locations": [{
          "physicalLocation": {
            "artifactLocation": {"uri": "file:///home/user/project/src/main.rs"},
            "region": {"startLine": 10}
          }
        }]
      },
      {
        "ruleId": "rust/unused-import",
        "ruleIndex": 1,
        "level": "warning",
        "message": {"text": "Unused import."},
        "locations": [{
          "physicalLocation": {
            "artifactLocation": {"uri": "src/lib.rs"},
            "region": {"startLine": 5, "startColumn": 1, "endLine": 5}
          }
        }]
      }
    ]
  }]
}"#;

    /// Duplicate findings at the same location — second is higher severity.
    const SARIF_DUPLICATES: &str = r#"{
  "version": "2.1.0",
  "runs": [{
    "tool": {
      "driver": {
        "name": "CodeQL",
        "rules": [{"id": "rust/overflow"}, {"id": "rust/overflow-bis"}]
      }
    },
    "results": [
      {
        "ruleId": "rust/overflow",
        "ruleIndex": 0,
        "level": "warning",
        "message": {"text": "Potential overflow (variant A)."},
        "locations": [{
          "physicalLocation": {
            "artifactLocation": {"uri": "src/math.rs"},
            "region": {"startLine": 20, "startColumn": 5}
          }
        }]
      },
      {
        "ruleId": "rust/overflow-bis",
        "ruleIndex": 1,
        "level": "error",
        "message": {"text": "Potential overflow (variant B, confirmed)."},
        "locations": [{
          "physicalLocation": {
            "artifactLocation": {"uri": "src/math.rs"},
            "region": {"startLine": 20, "startColumn": 5}
          }
        }]
      }
    ]
  }]
}"#;

    #[test]
    fn parse_single_error_finding() {
        let results = SarifParser::parse(SARIF_ONE_ERROR, 50).unwrap();
        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.rule_id, "rust/path-injection");
        assert_eq!(r.severity, "error");
        assert_eq!(r.file_path, "src/main.rs");
        assert_eq!(r.start_line, 42);
        assert_eq!(r.start_column, Some(10));
        assert!(r.message.contains("Unsanitized"));
    }

    #[test]
    fn parse_two_mixed_severities() {
        let results = SarifParser::parse(SARIF_TWO_MIXED, 50).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].severity, "error");
        assert_eq!(results[1].severity, "warning");
    }

    #[test]
    fn file_uri_prefix_is_stripped() {
        let results = SarifParser::parse(SARIF_TWO_MIXED, 50).unwrap();
        assert_eq!(results[0].file_path, "src/main.rs");
    }

    #[test]
    fn deduplicate_keeps_higher_severity() {
        let results = SarifParser::parse(SARIF_DUPLICATES, 50).unwrap();
        // Both findings share the same location; only the error should survive.
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].severity, "error");
        assert_eq!(results[0].rule_id, "rust/overflow-bis");
    }

    #[test]
    fn max_results_truncates() {
        let results = SarifParser::parse(SARIF_TWO_MIXED, 1).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn stable_name_is_deterministic() {
        let a = SarifResult {
            rule_id: "rust/test".into(),
            severity: "error".into(),
            message: "msg".into(),
            file_path: "src/main.rs".into(),
            start_line: 10,
            start_column: Some(5),
            end_line: None,
            code_snippet: None,
        };
        let b = SarifResult {
            rule_id: "rust/test".into(),
            severity: "warning".into(),
            message: "other msg".into(),
            file_path: "src/main.rs".into(),
            start_line: 10,
            start_column: Some(5),
            end_line: None,
            code_snippet: None,
        };
        assert_eq!(a.stable_name(), b.stable_name());
        assert!(a.stable_name().starts_with("codeql-rust/test-"));
    }

    #[test]
    fn to_memory_entry_maps_fields() {
        let result = SarifResult {
            rule_id: "rust/test".into(),
            severity: "error".into(),
            message: "Something is wrong.".into(),
            file_path: "src/lib.rs".into(),
            start_line: 7,
            start_column: Some(3),
            end_line: Some(9),
            code_snippet: Some("let x = 1;".into()),
        };
        let entry = result.to_memory_entry(Some("codeql-startup"));
        assert_eq!(entry.category, MemoryCategory::Fact);
        assert_eq!(entry.status, MemoryStatus::NeedsFix);
        assert_eq!(entry.confidence, Some("high".into()));
        assert_eq!(entry.source_session, Some("codeql-startup".into()));
        assert!(entry.tags.contains(&"codeql".into()));
        assert!(entry.tags.contains(&"error".into()));
        assert!(entry.body.contains("Something is wrong"));
        assert!(entry.body.contains("let x = 1;"));
        assert!(entry.body.contains("src/lib.rs:7:3-9"));
    }

    #[test]
    fn to_memory_entry_warning_is_working_status() {
        let result = SarifResult {
            rule_id: "rust/warn".into(),
            severity: "warning".into(),
            message: "Minor issue.".into(),
            file_path: "src/mod.rs".into(),
            start_line: 1,
            start_column: None,
            end_line: None,
            code_snippet: None,
        };
        let entry = result.to_memory_entry(None);
        assert_eq!(entry.status, MemoryStatus::Working);
    }

    #[test]
    fn malformed_json_returns_error() {
        assert!(SarifParser::parse("not json", 50).is_err());
    }

    #[test]
    fn empty_runs_returns_empty() {
        let results = SarifParser::parse(r#"{"version": "2.1.0", "runs": []}"#, 50).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn missing_locations_is_handled() {
        let sarif = r#"{
  "version": "2.1.0",
  "runs": [{
    "tool": {"driver": {"name": "C", "rules": [{"id": "test/rule"}]}},
    "results": [{
      "ruleId": "test/rule",
      "ruleIndex": 0,
      "level": "note",
      "message": {"text": "A note."}
    }]
  }]
}"#;
        let results = SarifParser::parse(sarif, 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "<unknown>");
        assert_eq!(results[0].start_line, 1);
    }
}
