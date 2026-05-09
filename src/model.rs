/// Map raw model IDs to human-friendly display names.
pub fn display_name(model_id: &str) -> &str {
    match model_id {
        "claude-opus-4-7" => "Opus 4.7",
        "claude-opus-4-6" => "Opus 4.6",
        "claude-sonnet-4-6" => "Sonnet 4.6",
        "claude-sonnet-4-5-20250514" => "Sonnet 4.5",
        "claude-haiku-4-5-20251001" => "Haiku 4.5",
        "claude-opus-4-20250514" => "Opus 4",
        "claude-sonnet-4-20250514" => "Sonnet 4",
        _ => model_id,
    }
}

/// Context window size for a given model ID.
pub fn context_window(model_id: &str) -> u64 {
    match model_id {
        "claude-opus-4-7" => 1_000_000,
        "claude-opus-4-6" => 1_000_000,
        "claude-sonnet-4-6" => 200_000,
        "claude-sonnet-4-5-20250514" => 200_000,
        "claude-haiku-4-5-20251001" => 200_000,
        "claude-opus-4-20250514" => 200_000,
        "claude-sonnet-4-20250514" => 200_000,
        _ => 200_000,
    }
}

/// Reverse lookup: display name (from /model output) → model ID.
/// Returns None if the display name is not recognized.
pub fn id_from_display_name(display: &str) -> Option<&'static str> {
    match display {
        "Opus 4.7" | "Opus 4.7 (1M context)" => Some("claude-opus-4-7"),
        "Opus 4.6" | "Opus 4.6 (1M context)" => Some("claude-opus-4-6"),
        "Sonnet 4.6" => Some("claude-sonnet-4-6"),
        "Sonnet 4.5" => Some("claude-sonnet-4-5-20250514"),
        "Haiku 4.5" => Some("claude-haiku-4-5-20251001"),
        "Opus 4" => Some("claude-opus-4-20250514"),
        "Sonnet 4" => Some("claude-sonnet-4-20250514"),
        _ => None,
    }
}

/// Format model name with optional effort level.
pub fn format_with_effort(model_id: &str, effort: &str) -> String {
    let name = display_name(model_id);
    if effort.is_empty() || effort == "default" {
        name.to_string()
    } else {
        format!("{name} ({effort})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Regression for commit eb91349: Opus 4.7 was missing from the model
    // table, so token_ratio fell back to the default 200k window and showed
    // 5x the real usage.
    #[test]
    fn opus_4_7_has_one_million_context() {
        assert_eq!(context_window("claude-opus-4-7"), 1_000_000);
    }

    #[test]
    fn opus_4_6_has_one_million_context() {
        assert_eq!(context_window("claude-opus-4-6"), 1_000_000);
    }

    #[test]
    fn unknown_model_falls_back_to_200k() {
        assert_eq!(context_window("claude-something-unknown"), 200_000);
    }

    #[test]
    fn opus_4_7_round_trips_through_display_lookup() {
        let id = "claude-opus-4-7";
        let display = display_name(id);
        assert_eq!(display, "Opus 4.7");
        // /model output may include the "(1M context)" suffix
        assert_eq!(id_from_display_name(display), Some(id));
        assert_eq!(id_from_display_name("Opus 4.7 (1M context)"), Some(id));
    }

    #[test]
    fn format_with_effort_drops_default_marker() {
        assert_eq!(format_with_effort("claude-opus-4-7", ""), "Opus 4.7");
        assert_eq!(format_with_effort("claude-opus-4-7", "default"), "Opus 4.7");
        assert_eq!(format_with_effort("claude-opus-4-7", "max"), "Opus 4.7 (max)");
    }
}
