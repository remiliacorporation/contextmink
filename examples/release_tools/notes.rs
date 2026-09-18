use anyhow::{Result, ensure};

pub fn render(version: &str, text: &str) -> Result<String> {
    semver::Version::parse(version)?;
    ensure!(
        crate::encoding::scan_encoding_suspects(text, false).is_empty(),
        "CHANGELOG.md contains encoding artifacts; repair its UTF-8 text before release"
    );
    let heading = regex::Regex::new(&format!(
        r"^## \[{}\] - \d{{4}}-\d{{2}}-\d{{2}}$",
        regex::escape(version)
    ))?;
    let lines: Vec<_> = text.lines().collect();
    let mut matches = Vec::new();
    let mut boundaries = Vec::new();
    let mut fence = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(marker) = fence {
            if trimmed == marker {
                fence = None;
            }
        } else if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = Some(if trimmed.starts_with("```") {
                "```"
            } else {
                "~~~"
            });
        } else {
            if heading.is_match(line) {
                matches.push(i);
            }
            if line.starts_with("## ") {
                boundaries.push(i);
            }
        }
    }
    ensure!(
        matches.len() == 1,
        "CHANGELOG.md requires exactly one dated section for {version}"
    );
    let end = boundaries
        .into_iter()
        .find(|i| *i > matches[0])
        .unwrap_or(lines.len());
    let body = &lines[matches[0] + 1..end];
    ensure!(
        body.iter()
            .any(|line| !line.trim().is_empty() && !line.starts_with('#')),
        "CHANGELOG.md has no release notes for {version}; add user-visible changes"
    );
    let mut rendered = String::new();
    let mut paragraph = String::new();
    let mut fence: Option<&str> = None;
    for line in body {
        let trimmed = line.trim();
        if let Some(marker) = fence {
            rendered.push_str(line);
            rendered.push('\n');
            if trimmed == marker {
                fence = None;
            }
            continue;
        }
        let starts_fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        let starts_block = trimmed.is_empty()
            || line.starts_with('#')
            || line.starts_with("- ")
            || line.starts_with("* ")
            || starts_fence;
        if starts_block && !paragraph.is_empty() {
            rendered.push_str(&paragraph);
            rendered.push('\n');
            paragraph.clear();
        }
        if starts_fence {
            fence = Some(if trimmed.starts_with("```") {
                "```"
            } else {
                "~~~"
            });
            rendered.push_str(line);
            rendered.push('\n');
        } else if trimmed.is_empty() || line.starts_with('#') {
            rendered.push_str(line);
            rendered.push('\n');
        } else {
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(trimmed);
        }
    }
    ensure!(
        fence.is_none(),
        "release notes contain an unterminated fenced code block; close it before publishing"
    );
    rendered.push_str(&paragraph);
    Ok(format!(
        "{}\n\nPrebuilt archives are attached for Windows x64, Linux x64, Intel macOS, and Apple Silicon macOS.\nMerge `.agents`, `.claude`, and `tools` into the project root. Skills are ready to discover; executables, documentation, licenses and the source manifest live under `tools/contextmink`.\nVerify the adjacent SHA-256 asset before extraction.\n",
        rendered.trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    const HEADING: &str = "## [1.2.3] - 2026-09-18\n";
    #[test]
    fn selects_exact_version_and_unwraps_prose_without_changing_code() {
        let text = format!(
            "## [Unreleased]\n- Future.\n{HEADING}\n### Added\n\n- Wrapped\n  change — valid.\n\n```sh\n  preserve spacing\n```\n\n## [1.2.2] - 2026-09-17\n- Old."
        );
        let result = render("1.2.3", &text).unwrap();
        assert!(result.contains("- Wrapped change — valid."));
        assert!(result.contains("```sh\n  preserve spacing\n```"));
        assert!(!result.contains("Future") && !result.contains("Old."));
        assert!(result.contains("SHA-256"));
    }
    #[test]
    fn refuses_missing_undated_duplicate_empty_and_broken_sections() {
        for text in [
            "## [1.2.30] - 2026-09-18\n- Wrong".into(),
            "## [1.2.3]\n- Undated".into(),
            HEADING.into(),
            format!("{HEADING}### Added\n"),
            format!("{HEADING}- One\n{HEADING}- Two"),
            format!("{HEADING}```sh\nnever closed"),
        ] {
            assert!(render("1.2.3", &text).is_err(), "{text}");
        }
    }
    #[test]
    fn refuses_encoding_artifacts_without_rejecting_typography() {
        for artifact in ["\u{e2}\u{20ac}\u{201d}", "\u{81}"] {
            assert!(render("1.2.3", &format!("{HEADING}- {artifact}")).is_err());
        }
        assert!(render("1.2.3", &format!("{HEADING}- Text — valid.")).is_ok());
    }
}
