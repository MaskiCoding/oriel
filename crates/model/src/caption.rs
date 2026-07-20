//! The Pulse: activity a window declares through its title. Agent CLIs and
//! build tools prefix a spinner glyph while they work; the switcher turns
//! that into a first-class indicator and keeps the title clean.

/// The asterisk family working tools cycle through. Braille dots are NOT
/// working: agent CLIs park one in the title while waiting for input.
fn working_glyph(c: char) -> bool {
    matches!(c, '✢' | '✳' | '✶' | '✻' | '✽' | '✺' | '❋' | '∗')
}

/// Quiet leading markers the same tools leave when idle or waiting.
fn idle_glyph(c: char) -> bool {
    c == '·' || ('\u{2800}'..='\u{28FF}').contains(&c)
}

pub struct Caption {
    pub title: String,
    pub working: bool,
}

/// Splits a raw window title into the text worth showing and whether the
/// window says it is busy. Only a lone leading glyph counts — a word that
/// merely starts with one stays untouched.
pub fn decode_title(raw: &str) -> Caption {
    let trimmed = raw.trim();
    let mut chars = trimmed.chars();
    if let Some(first) = chars.next() {
        let rest = chars.as_str();
        if rest.is_empty() || rest.starts_with(' ') {
            if working_glyph(first) {
                return Caption {
                    title: rest.trim_start().to_string(),
                    working: true,
                };
            }
            if idle_glyph(first) {
                return Caption {
                    title: rest.trim_start().to_string(),
                    working: false,
                };
            }
        }
    }
    Caption {
        title: trimmed.to_string(),
        working: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_glyphs_mean_working() {
        for raw in ["✻ Finish M2 tasks", "✳ Finish M2 tasks", "✽ x"] {
            let c = decode_title(raw);
            assert!(c.working, "{raw}");
            assert!(!c.title.contains('✻') && !c.title.contains('✳'));
        }
        assert_eq!(decode_title("✻ Finish M2 tasks").title, "Finish M2 tasks");
    }

    #[test]
    fn idle_markers_are_stripped_but_not_working() {
        // the middle dot, and the braille dot agent CLIs park while waiting
        for raw in ["· Review Slack thread", "⠂ Review Slack thread"] {
            let c = decode_title(raw);
            assert!(!c.working, "{raw}");
            assert_eq!(c.title, "Review Slack thread");
        }
    }

    #[test]
    fn plain_titles_pass_through() {
        let c = decode_title("YouTube - Vivaldi");
        assert!(!c.working);
        assert_eq!(c.title, "YouTube - Vivaldi");
    }

    #[test]
    fn glyph_glued_to_a_word_is_left_alone() {
        let c = decode_title("✳notes.md");
        assert!(!c.working);
        assert_eq!(c.title, "✳notes.md");
    }

    #[test]
    fn lone_glyph_and_empty_titles() {
        assert!(decode_title("✻").working);
        assert_eq!(decode_title("✻").title, "");
        assert!(!decode_title("").working);
    }
}
