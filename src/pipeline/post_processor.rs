//! Post-processing stage for transforming transcribed text before output.
//!
//! Sits between TranscriberStation and SinkStation in the pipeline.
//! The primary use case is voice commands: spoken punctuation and formatting.

use crate::config::Config;
use crate::pipeline::error::StationError;
use crate::pipeline::station::Station;
use crate::pipeline::types::TranscribedText;
use std::collections::HashMap;

/// Trait for text post-processing. Implementations transform transcribed text
/// before it reaches the sink.
pub trait PostProcessor: Send + 'static {
    /// Transform transcribed text. Returns the processed string.
    fn process(&mut self, text: &str) -> String;

    /// Transform transcribed text and produce events.
    /// Default implementation delegates to `process` and returns no events.
    fn process_with_events(
        &mut self,
        text: &str,
    ) -> (String, Vec<crate::pipeline::types::SinkEvent>) {
        (self.process(text), vec![])
    }

    /// Name for logging/diagnostics.
    fn name(&self) -> &'static str;
}

/// Pipeline station that applies a chain of post-processors to transcribed text.
pub struct PostProcessorStation {
    processors: Vec<Box<dyn PostProcessor>>,
}

impl PostProcessorStation {
    pub fn new(processors: Vec<Box<dyn PostProcessor>>) -> Self {
        Self { processors }
    }
}

impl Station for PostProcessorStation {
    type Input = TranscribedText;
    type Output = TranscribedText;

    fn process(
        &mut self,
        mut input: TranscribedText,
    ) -> Result<Option<TranscribedText>, StationError> {
        for processor in &mut self.processors {
            let (new_text, events) = processor.process_with_events(&input.text);
            input.text = new_text;
            input.events.extend(events);
        }

        // Filter out empty results after processing
        if input.text.trim().is_empty() {
            return Ok(None);
        }

        Ok(Some(input))
    }

    fn name(&self) -> &'static str {
        "post-processor"
    }
}

/// Build post-processors from application configuration.
///
/// Returns a `VoiceCommandProcessor` when `config.voice_commands.enabled` is
/// true, or an empty list otherwise.
pub fn build_post_processors(config: &Config) -> Vec<Box<dyn PostProcessor>> {
    let mut processors: Vec<Box<dyn PostProcessor>> = Vec::new();

    if config.voice_commands.enabled {
        processors.push(Box::new(VoiceCommandProcessor::new(
            &config.stt.language,
            config.voice_commands.disable_defaults,
            &config.voice_commands.commands,
        )));
    }

    processors
}

/// Rule-based voice command processor.
///
/// Scans transcribed text for spoken command phrases and replaces them with
/// their corresponding output. Supports punctuation, formatting toggles,
/// and user-configurable overrides.
pub struct VoiceCommandProcessor {
    /// Sorted by descending key length so longer phrases match first.
    commands: Vec<(String, CommandAction)>,
    /// Current caps-lock state toggled by "all caps" / "end caps".
    caps_active: bool,
}

/// Characters that Whisper may append to command words as inferred punctuation.
/// For example, if you say "period" with falling intonation, Whisper may
/// transcribe it as "Period." — we consume the trailing punctuation to prevent
/// double output (e.g., ".." instead of ".").
fn is_whisper_trailing_punct(ch: char) -> bool {
    matches!(ch, '.' | ',' | '?' | '!' | ':' | ';')
}

/// Count consecutive trailing punctuation characters.
/// Handles sequences like "..." (three dots for ellipsis).
fn count_trailing_punct(chars: &[char]) -> usize {
    chars
        .iter()
        .take_while(|ch| is_whisper_trailing_punct(**ch))
        .count()
}

/// Infer attachment behavior from replacement text.
///
/// Detects punctuation, brackets, whitespace to choose the appropriate
/// CommandAction automatically.
fn infer_action(replacement: &str) -> CommandAction {
    if replacement
        .chars()
        .all(|c| matches!(c, '.' | ',' | ':' | ';' | '?' | '!'))
        && !replacement.is_empty()
    {
        return CommandAction::punct(replacement);
    }
    if replacement.len() == 1
        && let Some(ch) = replacement.chars().next()
    {
        match ch {
            '(' | '[' | '{' => return CommandAction::open(replacement),
            ')' | ']' | '}' => return CommandAction::close(replacement),
            _ => {}
        }
    }
    if replacement.chars().all(|c| c.is_whitespace()) && !replacement.is_empty() {
        return CommandAction::whitespace(replacement);
    }
    CommandAction::free(replacement)
}

/// What a voice command does when matched.
#[derive(Debug, Clone, PartialEq)]
enum CommandAction {
    /// Replace the spoken phrase with literal text.
    /// `attach_left`: remove space before replacement (e.g., period, comma)
    /// `attach_right`: remove space after replacement (e.g., open paren, hyphen)
    Insert {
        text: String,
        attach_left: bool,
        attach_right: bool,
    },
    /// Toggle caps mode on.
    CapsOn,
    /// Toggle caps mode off.
    CapsOff,
    /// Emit a keyboard shortcut event (e.g., "ctrl+BackSpace").
    KeyCombo(String),
}

impl CommandAction {
    /// Punctuation that attaches to the left word (period, comma, etc.)
    fn punct(text: &str) -> Self {
        Self::Insert {
            text: text.into(),
            attach_left: true,
            attach_right: false,
        }
    }

    /// Text that attaches to the right word (open paren, open quote)
    fn open(text: &str) -> Self {
        Self::Insert {
            text: text.into(),
            attach_left: false,
            attach_right: true,
        }
    }

    /// Closing delimiter that attaches to the left (close paren, close quote)
    fn close(text: &str) -> Self {
        Self::punct(text)
    }

    /// Text that attaches to both sides (hyphen)
    fn tight(text: &str) -> Self {
        Self::Insert {
            text: text.into(),
            attach_left: true,
            attach_right: true,
        }
    }

    /// Whitespace replacement (newline, tab) — eats surrounding spaces
    fn whitespace(text: &str) -> Self {
        Self::tight(text)
    }

    /// Free-standing text (em-dash with its own spacing)
    fn free(text: &str) -> Self {
        Self::Insert {
            text: text.into(),
            attach_left: false,
            attach_right: false,
        }
    }
}

impl VoiceCommandProcessor {
    /// Build a processor from a language tag and optional user overrides.
    ///
    /// `language` should be an ISO 639-1 code ("en", "de", "es", …) or "auto".
    /// When "auto", English defaults are used.
    ///
    /// `disable_defaults`: when true, built-in commands are not loaded.
    ///
    /// `overrides` are extra mappings from the `[voice_commands]` config section.
    /// They take precedence over built-in defaults and use inferred attachment.
    pub fn new(
        language: &str,
        disable_defaults: bool,
        overrides: &HashMap<String, String>,
    ) -> Self {
        let mut map: HashMap<String, CommandAction> = HashMap::new();

        // Load built-in commands for the language unless disabled
        if !disable_defaults {
            let builtins = builtin_commands(language);
            for (phrase, action) in builtins {
                map.insert(phrase, action);
            }
        }

        // Apply user overrides (these always win) — infer attachment behavior
        for (phrase, replacement) in overrides {
            let lower = phrase.to_lowercase();
            map.insert(lower, infer_action(replacement));
        }

        // Sort by descending key length so "new paragraph" matches before "new"
        let mut commands: Vec<(String, CommandAction)> = map.into_iter().collect();
        commands.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        Self {
            commands,
            caps_active: false,
        }
    }

    /// Apply voice command replacements and produce sink events.
    ///
    /// When KeyCombo commands are present, text is split into events:
    /// - `SinkEvent::Text` for regular text segments
    /// - `SinkEvent::KeyCombo` for keyboard shortcuts
    ///
    /// When no KeyCombo commands are matched, returns empty events for
    /// backward compatibility (pure text path).
    fn apply_with_events(
        &mut self,
        text: &str,
    ) -> (String, Vec<crate::pipeline::types::SinkEvent>) {
        use crate::pipeline::types::SinkEvent;

        let chars: Vec<char> = text.chars().collect();
        let len = chars.len();
        let mut i = 0;
        let mut current_buf = String::with_capacity(text.len());
        let mut events: Vec<SinkEvent> = Vec::new();
        let mut has_key_combo = false;

        while i < len {
            let mut matched = false;

            for (phrase, action) in &self.commands {
                let phrase_chars: Vec<char> = phrase.chars().collect();
                let plen = phrase_chars.len();

                if i + plen > len {
                    continue;
                }

                // Case-insensitive char-by-char comparison, safe for all Unicode
                let chars_match = chars[i..i + plen]
                    .iter()
                    .zip(phrase_chars.iter())
                    .all(|(src, phr)| src.to_lowercase().eq(phr.to_lowercase()));
                if !chars_match {
                    continue;
                }

                // Ensure word boundaries: the character before must be start-of-string
                // or whitespace, and the character after must be end-of-string,
                // whitespace, or Whisper-inferred trailing punctuation.
                let before_ok = i == 0 || chars[i - 1].is_whitespace();
                let after_pos = i + plen;
                let (after_ok, trailing_punct_len) =
                    if after_pos == len || chars[after_pos].is_whitespace() {
                        (true, 0)
                    } else {
                        // Accept trailing punctuation Whisper may have appended
                        // (e.g., "Enter." → match "enter", consume ".")
                        let punct_len = count_trailing_punct(&chars[after_pos..]);
                        if punct_len > 0
                            && (after_pos + punct_len == len
                                || chars[after_pos + punct_len].is_whitespace())
                        {
                            (true, punct_len)
                        } else {
                            (false, 0)
                        }
                    };

                if !before_ok || !after_ok {
                    continue;
                }

                match action {
                    CommandAction::KeyCombo(combo) => {
                        has_key_combo = true;
                        // Flush text buffer
                        if !current_buf.is_empty() {
                            events.push(SinkEvent::Text(std::mem::take(&mut current_buf)));
                        }
                        events.push(SinkEvent::KeyCombo(combo.clone()));
                        // Skip past match
                        i += plen + trailing_punct_len;
                        if i < len && chars[i].is_whitespace() {
                            i += 1;
                        }
                    }
                    CommandAction::Insert {
                        text: replacement,
                        attach_left,
                        attach_right,
                    } => {
                        if *attach_left {
                            while current_buf.ends_with(' ') {
                                current_buf.pop();
                            }
                        }
                        current_buf.push_str(replacement);
                        i += plen + trailing_punct_len;
                        if *attach_right && i < len && chars[i].is_whitespace() {
                            i += 1;
                        }
                    }
                    CommandAction::CapsOn => {
                        self.caps_active = true;
                        i += plen + trailing_punct_len;
                        if i < len && chars[i].is_whitespace() {
                            i += 1;
                        }
                    }
                    CommandAction::CapsOff => {
                        self.caps_active = false;
                        i += plen + trailing_punct_len;
                        if i < len && chars[i].is_whitespace() {
                            i += 1;
                        }
                    }
                }

                matched = true;
                break;
            }

            if !matched {
                let ch = chars[i];
                if self.caps_active {
                    for upper in ch.to_uppercase() {
                        current_buf.push(upper);
                    }
                } else {
                    current_buf.push(ch);
                }
                i += 1;
            }
        }

        // Flush remaining text
        if !current_buf.is_empty() {
            if has_key_combo {
                events.push(SinkEvent::Text(std::mem::take(&mut current_buf)));
            } else {
                // No key combos at all — return text only, events empty (backward compat)
                return (current_buf, vec![]);
            }
        }

        if has_key_combo {
            // Full text is the concatenation of all Text events for backward compat
            let full_text = events
                .iter()
                .filter_map(|e| {
                    if let SinkEvent::Text(t) = e {
                        Some(t.as_str())
                    } else {
                        None
                    }
                })
                .collect::<String>();
            (full_text, events)
        } else {
            (current_buf, vec![])
        }
    }

    /// Apply voice command replacements to a single text segment.
    ///
    /// Uses a simple O(n*m) scan where n = text length and m = number of
    /// commands. With n < 100 chars (typical Whisper segment) and m ~ 20
    /// built-in commands, this runs in sub-millisecond time. Whisper
    /// transcription dominates overall latency by orders of magnitude.
    /// Alternatives considered (aho-corasick, HashMap sliding window) add
    /// complexity for word-boundary validation, caps-lock state, and spacing
    /// rules without meaningful benefit at this scale.
    fn apply(&mut self, text: &str) -> String {
        self.apply_with_events(text).0
    }
}

impl PostProcessor for VoiceCommandProcessor {
    fn process(&mut self, text: &str) -> String {
        self.apply(text)
    }

    fn process_with_events(
        &mut self,
        text: &str,
    ) -> (String, Vec<crate::pipeline::types::SinkEvent>) {
        self.apply_with_events(text)
    }

    fn name(&self) -> &'static str {
        "voice-commands"
    }
}

/// Return built-in commands as phrase → replacement pairs for config display.
///
/// This is a public API used by config template generation. It strips out
/// the internal CommandAction details and returns only the phrase-to-text mapping.
pub fn builtin_commands_display(language: &str) -> Vec<(String, String)> {
    builtin_commands(language)
        .into_iter()
        .filter_map(|(phrase, action)| match action {
            CommandAction::Insert { text, .. } => Some((phrase, text)),
            CommandAction::KeyCombo(combo) => Some((phrase, combo)),
            _ => None,
        })
        .collect()
}

/// Built-in voice command mappings for a given language.
fn builtin_commands(language: &str) -> Vec<(String, CommandAction)> {
    match language {
        "en" | "auto" => english_commands(),
        "de" => german_commands(),
        "es" => spanish_commands(),
        "fr" => french_commands(),
        "pt" => portuguese_commands(),
        "it" => italian_commands(),
        "nl" => dutch_commands(),
        "pl" => polish_commands(),
        "ru" => russian_commands(),
        "ja" => japanese_commands(),
        "zh" => chinese_commands(),
        "ko" => korean_commands(),
        // Fallback: English commands work as a reasonable default since
        // Whisper tends to output English command phrases even for other languages.
        _ => english_commands(),
    }
}

// ── English ──────────────────────────────────────────────────────────────

fn english_commands() -> Vec<(String, CommandAction)> {
    vec![
        // Punctuation (attach left: no space before)
        ("period".into(), CommandAction::punct(".")),
        ("full stop".into(), CommandAction::punct(".")),
        ("dot".into(), CommandAction::punct(".")),
        ("comma".into(), CommandAction::punct(",")),
        ("question mark".into(), CommandAction::punct("?")),
        ("exclamation mark".into(), CommandAction::punct("!")),
        ("exclamation point".into(), CommandAction::punct("!")),
        ("colon".into(), CommandAction::punct(":")),
        ("semicolon".into(), CommandAction::punct(";")),
        ("ellipsis".into(), CommandAction::punct("...")),
        // "dash" is ambiguous: users may mean the literal word (e.g. "100-meter dash").
        // This is inherent to rule-based voice commands. Users can override or disable
        // the mapping via [voice_commands.commands] in config. Future NLP-based
        // post-processing could disambiguate using context.
        ("dash".into(), CommandAction::tight(" — ")),
        ("hyphen".into(), CommandAction::tight("-")),
        // Quotes and brackets
        ("open quote".into(), CommandAction::open("\"")),
        ("close quote".into(), CommandAction::close("\"")),
        ("open parenthesis".into(), CommandAction::open("(")),
        ("close parenthesis".into(), CommandAction::close(")")),
        // Whitespace / formatting
        ("new line".into(), CommandAction::whitespace("\n")),
        ("new paragraph".into(), CommandAction::whitespace("\n\n")),
        ("enter".into(), CommandAction::whitespace("\n")),
        ("tab".into(), CommandAction::whitespace("\t")),
        // Caps toggle
        ("all caps".into(), CommandAction::CapsOn),
        ("end caps".into(), CommandAction::CapsOff),
        // Key combos
        (
            "delete word".into(),
            CommandAction::KeyCombo("ctrl+BackSpace".to_string()),
        ),
    ]
}

// ── German ───────────────────────────────────────────────────────────────

fn german_commands() -> Vec<(String, CommandAction)> {
    vec![
        ("punkt".into(), CommandAction::punct(".")),
        ("komma".into(), CommandAction::punct(",")),
        ("fragezeichen".into(), CommandAction::punct("?")),
        ("ausrufezeichen".into(), CommandAction::punct("!")),
        ("doppelpunkt".into(), CommandAction::punct(":")),
        ("semikolon".into(), CommandAction::punct(";")),
        ("neue zeile".into(), CommandAction::whitespace("\n")),
        ("neuer absatz".into(), CommandAction::whitespace("\n\n")),
        ("enter".into(), CommandAction::whitespace("\n")),
        ("großbuchstaben".into(), CommandAction::CapsOn),
        ("ende großbuchstaben".into(), CommandAction::CapsOff),
        (
            "wort löschen".into(),
            CommandAction::KeyCombo("ctrl+BackSpace".to_string()),
        ),
    ]
}

// ── Spanish ──────────────────────────────────────────────────────────────

fn spanish_commands() -> Vec<(String, CommandAction)> {
    vec![
        ("punto".into(), CommandAction::punct(".")),
        ("coma".into(), CommandAction::punct(",")),
        ("signo de interrogación".into(), CommandAction::punct("?")),
        ("signo de exclamación".into(), CommandAction::punct("!")),
        ("dos puntos".into(), CommandAction::punct(":")),
        ("punto y coma".into(), CommandAction::punct(";")),
        ("nueva línea".into(), CommandAction::whitespace("\n")),
        ("nuevo párrafo".into(), CommandAction::whitespace("\n\n")),
        ("mayúsculas".into(), CommandAction::CapsOn),
        ("fin mayúsculas".into(), CommandAction::CapsOff),
    ]
}

// ── French ───────────────────────────────────────────────────────────────

fn french_commands() -> Vec<(String, CommandAction)> {
    vec![
        ("point".into(), CommandAction::punct(".")),
        ("virgule".into(), CommandAction::punct(",")),
        ("point d'interrogation".into(), CommandAction::punct("?")),
        ("point d'exclamation".into(), CommandAction::punct("!")),
        ("deux points".into(), CommandAction::punct(":")),
        ("point-virgule".into(), CommandAction::punct(";")),
        ("nouvelle ligne".into(), CommandAction::whitespace("\n")),
        (
            "nouveau paragraphe".into(),
            CommandAction::whitespace("\n\n"),
        ),
        ("majuscules".into(), CommandAction::CapsOn),
        ("fin majuscules".into(), CommandAction::CapsOff),
    ]
}

// ── Portuguese ───────────────────────────────────────────────────────────

fn portuguese_commands() -> Vec<(String, CommandAction)> {
    vec![
        ("ponto".into(), CommandAction::punct(".")),
        ("vírgula".into(), CommandAction::punct(",")),
        ("ponto de interrogação".into(), CommandAction::punct("?")),
        ("ponto de exclamação".into(), CommandAction::punct("!")),
        ("dois pontos".into(), CommandAction::punct(":")),
        ("ponto e vírgula".into(), CommandAction::punct(";")),
        ("nova linha".into(), CommandAction::whitespace("\n")),
        ("novo parágrafo".into(), CommandAction::whitespace("\n\n")),
        ("maiúsculas".into(), CommandAction::CapsOn),
        ("fim maiúsculas".into(), CommandAction::CapsOff),
    ]
}

// ── Italian ──────────────────────────────────────────────────────────────

fn italian_commands() -> Vec<(String, CommandAction)> {
    vec![
        ("punto".into(), CommandAction::punct(".")),
        ("virgola".into(), CommandAction::punct(",")),
        ("punto interrogativo".into(), CommandAction::punct("?")),
        ("punto esclamativo".into(), CommandAction::punct("!")),
        ("due punti".into(), CommandAction::punct(":")),
        ("punto e virgola".into(), CommandAction::punct(";")),
        ("nuova riga".into(), CommandAction::whitespace("\n")),
        ("nuovo paragrafo".into(), CommandAction::whitespace("\n\n")),
        ("maiuscole".into(), CommandAction::CapsOn),
        ("fine maiuscole".into(), CommandAction::CapsOff),
    ]
}

// ── Dutch ────────────────────────────────────────────────────────────────

fn dutch_commands() -> Vec<(String, CommandAction)> {
    vec![
        ("punt".into(), CommandAction::punct(".")),
        ("komma".into(), CommandAction::punct(",")),
        ("vraagteken".into(), CommandAction::punct("?")),
        ("uitroepteken".into(), CommandAction::punct("!")),
        ("dubbele punt".into(), CommandAction::punct(":")),
        ("puntkomma".into(), CommandAction::punct(";")),
        ("nieuwe regel".into(), CommandAction::whitespace("\n")),
        ("nieuwe alinea".into(), CommandAction::whitespace("\n\n")),
        ("hoofdletters".into(), CommandAction::CapsOn),
        ("einde hoofdletters".into(), CommandAction::CapsOff),
    ]
}

// ── Polish ───────────────────────────────────────────────────────────────

fn polish_commands() -> Vec<(String, CommandAction)> {
    vec![
        ("kropka".into(), CommandAction::punct(".")),
        ("przecinek".into(), CommandAction::punct(",")),
        ("znak zapytania".into(), CommandAction::punct("?")),
        ("wykrzyknik".into(), CommandAction::punct("!")),
        ("dwukropek".into(), CommandAction::punct(":")),
        ("średnik".into(), CommandAction::punct(";")),
        ("nowa linia".into(), CommandAction::whitespace("\n")),
        ("nowy akapit".into(), CommandAction::whitespace("\n\n")),
        ("wielkie litery".into(), CommandAction::CapsOn),
        ("koniec wielkich liter".into(), CommandAction::CapsOff),
    ]
}

// ── Russian ──────────────────────────────────────────────────────────────

fn russian_commands() -> Vec<(String, CommandAction)> {
    vec![
        ("точка".into(), CommandAction::punct(".")),
        ("запятая".into(), CommandAction::punct(",")),
        ("вопросительный знак".into(), CommandAction::punct("?")),
        ("восклицательный знак".into(), CommandAction::punct("!")),
        ("двоеточие".into(), CommandAction::punct(":")),
        ("точка с запятой".into(), CommandAction::punct(";")),
        ("новая строка".into(), CommandAction::whitespace("\n")),
        ("новый абзац".into(), CommandAction::whitespace("\n\n")),
        ("заглавные".into(), CommandAction::CapsOn),
        ("конец заглавных".into(), CommandAction::CapsOff),
    ]
}

// ── Japanese ─────────────────────────────────────────────────────────────

fn japanese_commands() -> Vec<(String, CommandAction)> {
    vec![
        ("句点".into(), CommandAction::punct("。")),
        ("読点".into(), CommandAction::punct("、")),
        ("疑問符".into(), CommandAction::punct("？")),
        ("感嘆符".into(), CommandAction::punct("！")),
        ("改行".into(), CommandAction::whitespace("\n")),
        ("新段落".into(), CommandAction::whitespace("\n\n")),
        ("大文字".into(), CommandAction::CapsOn),
        ("大文字終了".into(), CommandAction::CapsOff),
    ]
}

// ── Chinese ──────────────────────────────────────────────────────────────

fn chinese_commands() -> Vec<(String, CommandAction)> {
    vec![
        ("句号".into(), CommandAction::punct("。")),
        ("逗号".into(), CommandAction::punct("，")),
        ("问号".into(), CommandAction::punct("？")),
        ("感叹号".into(), CommandAction::punct("！")),
        ("冒号".into(), CommandAction::punct("：")),
        ("分号".into(), CommandAction::punct("；")),
        ("换行".into(), CommandAction::whitespace("\n")),
        ("新段落".into(), CommandAction::whitespace("\n\n")),
        ("大写".into(), CommandAction::CapsOn),
        ("结束大写".into(), CommandAction::CapsOff),
    ]
}

// ── Korean ───────────────────────────────────────────────────────────────

fn korean_commands() -> Vec<(String, CommandAction)> {
    vec![
        ("마침표".into(), CommandAction::punct(".")),
        ("쉼표".into(), CommandAction::punct(",")),
        ("물음표".into(), CommandAction::punct("?")),
        ("느낌표".into(), CommandAction::punct("!")),
        ("줄바꿈".into(), CommandAction::whitespace("\n")),
        ("새 단락".into(), CommandAction::whitespace("\n\n")),
        ("대문자".into(), CommandAction::CapsOn),
        ("대문자 끝".into(), CommandAction::CapsOff),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    // ── VoiceCommandProcessor unit tests ─────────────────────────────────

    fn en_processor() -> VoiceCommandProcessor {
        VoiceCommandProcessor::new("en", false, &HashMap::new())
    }

    #[test]
    fn test_period_replacement() {
        let mut p = en_processor();
        assert_eq!(p.apply("hello world period"), "hello world.");
    }

    #[test]
    fn test_comma_replacement() {
        let mut p = en_processor();
        assert_eq!(p.apply("hello comma world"), "hello, world");
    }

    #[test]
    fn test_question_mark() {
        let mut p = en_processor();
        assert_eq!(p.apply("how are you question mark"), "how are you?");
    }

    #[test]
    fn test_exclamation_mark() {
        let mut p = en_processor();
        assert_eq!(p.apply("wow exclamation mark"), "wow!");
    }

    #[test]
    fn test_new_line() {
        let mut p = en_processor();
        assert_eq!(p.apply("first new line second"), "first\nsecond");
    }

    #[test]
    fn test_new_paragraph() {
        let mut p = en_processor();
        assert_eq!(p.apply("first new paragraph second"), "first\n\nsecond");
    }

    #[test]
    fn test_multiple_commands() {
        let mut p = en_processor();
        assert_eq!(
            p.apply("hello comma world period how are you question mark"),
            "hello, world. how are you?"
        );
    }

    #[test]
    fn test_case_insensitive() {
        let mut p = en_processor();
        assert_eq!(p.apply("hello Period"), "hello.");
    }

    #[test]
    fn test_no_commands_passthrough() {
        let mut p = en_processor();
        assert_eq!(p.apply("just regular text"), "just regular text");
    }

    #[test]
    fn test_empty_input() {
        let mut p = en_processor();
        assert_eq!(p.apply(""), "");
    }

    #[test]
    fn test_all_caps_toggle() {
        let mut p = en_processor();
        assert_eq!(
            p.apply("hello all caps world end caps foo"),
            "hello WORLD foo"
        );
    }

    #[test]
    fn test_caps_persists_across_words() {
        let mut p = en_processor();
        assert_eq!(
            p.apply("all caps hello world end caps done"),
            "HELLO WORLD done"
        );
    }

    #[test]
    fn test_user_overrides() {
        let mut overrides = HashMap::new();
        overrides.insert("smiley".to_string(), ":)".to_string());
        overrides.insert("at sign".to_string(), "@".to_string());

        let mut p = VoiceCommandProcessor::new("en", false, &overrides);
        assert_eq!(p.apply("hello smiley"), "hello :)");
        assert_eq!(p.apply("user at sign example"), "user @ example");
    }

    #[test]
    fn test_override_replaces_builtin() {
        let mut overrides = HashMap::new();
        // Override "period" to produce "!!!" instead of "."
        // "!!!" is inferred as punctuation (all chars are !)
        overrides.insert("period".to_string(), "!!!".to_string());

        let mut p = VoiceCommandProcessor::new("en", false, &overrides);
        // "!!!" is inferred as punctuation so it attaches left
        assert_eq!(p.apply("hello period"), "hello!!!");
    }

    #[test]
    fn test_word_boundary_no_partial_match() {
        let mut p = en_processor();
        // "periodic" should not trigger "period" replacement
        assert_eq!(p.apply("periodic table"), "periodic table");
    }

    #[test]
    fn test_colon_and_semicolon() {
        let mut p = en_processor();
        assert_eq!(p.apply("note colon important"), "note: important");
        assert_eq!(p.apply("done semicolon next"), "done; next");
    }

    #[test]
    fn test_ellipsis() {
        let mut p = en_processor();
        assert_eq!(p.apply("and then ellipsis"), "and then...");
    }

    #[test]
    fn test_quotes() {
        let mut p = en_processor();
        assert_eq!(
            p.apply("he said open quote hello close quote"),
            "he said \"hello\""
        );
    }

    #[test]
    fn test_parentheses() {
        let mut p = en_processor();
        assert_eq!(
            p.apply("note open parenthesis important close parenthesis"),
            "note (important)"
        );
    }

    #[test]
    fn test_tab() {
        let mut p = en_processor();
        assert_eq!(p.apply("indent tab code"), "indent\tcode");
    }

    #[test]
    fn test_dash_and_hyphen() {
        let mut p = en_processor();
        assert_eq!(p.apply("well dash that is it"), "well — that is it");
        assert_eq!(p.apply("self hyphen aware"), "self-aware");
    }

    // ── German language tests ────────────────────────────────────────────

    #[test]
    fn test_german_commands() {
        let mut p = VoiceCommandProcessor::new("de", false, &HashMap::new());
        assert_eq!(p.apply("hallo punkt"), "hallo.");
        assert_eq!(p.apply("hallo komma welt"), "hallo, welt");
        assert_eq!(p.apply("was fragezeichen"), "was?");
    }

    // ── Spanish language tests ───────────────────────────────────────────

    #[test]
    fn test_spanish_commands() {
        let mut p = VoiceCommandProcessor::new("es", false, &HashMap::new());
        assert_eq!(p.apply("hola punto"), "hola.");
        assert_eq!(p.apply("hola coma mundo"), "hola, mundo");
    }

    // ── French language tests ────────────────────────────────────────────

    #[test]
    fn test_french_commands() {
        let mut p = VoiceCommandProcessor::new("fr", false, &HashMap::new());
        assert_eq!(p.apply("bonjour point"), "bonjour.");
        assert_eq!(p.apply("bonjour virgule monde"), "bonjour, monde");
    }

    // ── PostProcessorStation tests ───────────────────────────────────────

    #[test]
    fn test_station_passthrough_no_processors() {
        let mut station = PostProcessorStation::new(vec![]);
        let input = TranscribedText::new("hello world".to_string());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "hello world");
    }

    #[test]
    fn test_station_with_voice_commands() {
        let processor = Box::new(VoiceCommandProcessor::new("en", false, &HashMap::new()));
        let mut station = PostProcessorStation::new(vec![processor]);
        let input = TranscribedText::new("hello comma world period".to_string());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "hello, world.");
    }

    #[test]
    fn test_station_filters_empty_result() {
        // A processor that returns empty string
        struct EmptyProcessor;
        impl PostProcessor for EmptyProcessor {
            fn process(&mut self, _text: &str) -> String {
                String::new()
            }
            fn name(&self) -> &'static str {
                "empty"
            }
        }

        let mut station = PostProcessorStation::new(vec![Box::new(EmptyProcessor)]);
        let input = TranscribedText::new("hello".to_string());
        let result = station.process(input).unwrap();
        assert!(
            result.is_none(),
            "Empty post-processed text should be filtered"
        );
    }

    #[test]
    fn test_station_preserves_timestamp() {
        let processor = Box::new(VoiceCommandProcessor::new("en", false, &HashMap::new()));
        let mut station = PostProcessorStation::new(vec![processor]);
        let ts = Instant::now();
        let input = TranscribedText {
            text: "hello period".to_string(),
            timestamp: ts,
            timing: None,
            events: vec![],
        };
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "hello.");
        assert_eq!(result.timestamp, ts);
    }

    #[test]
    fn test_station_chains_multiple_processors() {
        // First processor: voice commands
        let voice = Box::new(VoiceCommandProcessor::new("en", false, &HashMap::new()));
        // Second processor: uppercase everything
        struct UpperProcessor;
        impl PostProcessor for UpperProcessor {
            fn process(&mut self, text: &str) -> String {
                text.to_uppercase()
            }
            fn name(&self) -> &'static str {
                "upper"
            }
        }

        let mut station = PostProcessorStation::new(vec![voice, Box::new(UpperProcessor)]);
        let input = TranscribedText::new("hello period".to_string());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "HELLO.");
    }

    #[test]
    fn test_station_name() {
        let station = PostProcessorStation::new(vec![]);
        assert_eq!(station.name(), "post-processor");
    }

    #[test]
    fn test_station_whitespace_only_filtered() {
        let mut station = PostProcessorStation::new(vec![]);
        let input = TranscribedText::new("   \n\t  ".to_string());
        let result = station.process(input).unwrap();
        assert!(
            result.is_none(),
            "Whitespace-only text should be filtered out"
        );
    }

    #[test]
    fn test_only_command_input() {
        let mut p = en_processor();
        assert_eq!(p.apply("period"), ".");
    }

    #[test]
    fn test_only_caps_toggle_returns_empty() {
        // "all caps end caps" with nothing between produces empty string
        let mut p = en_processor();
        assert_eq!(p.apply("all caps end caps"), "");
    }

    #[test]
    fn test_multiple_spaces_between_words() {
        let mut p = en_processor();
        // All spaces before attach-left punctuation are consumed
        assert_eq!(p.apply("hello  period"), "hello.");
    }

    #[test]
    fn test_triple_space_before_punctuation() {
        let mut p = en_processor();
        assert_eq!(p.apply("hello   comma world"), "hello, world");
    }

    #[test]
    fn test_multi_space_before_close_quote() {
        let mut p = en_processor();
        assert_eq!(
            p.apply("he said open quote hello  close quote"),
            "he said \"hello\""
        );
    }

    #[test]
    fn test_caps_with_punctuation() {
        let mut p = en_processor();
        assert_eq!(
            p.apply("all caps hello end caps comma world"),
            "HELLO, world"
        );
    }

    #[test]
    fn test_russian_commands() {
        let mut p = VoiceCommandProcessor::new("ru", false, &HashMap::new());
        assert_eq!(p.apply("привет точка"), "привет.");
        assert_eq!(p.apply("привет запятая мир"), "привет, мир");
    }

    #[test]
    fn test_japanese_commands() {
        let mut p = VoiceCommandProcessor::new("ja", false, &HashMap::new());
        assert_eq!(p.apply("こんにちは 句点"), "こんにちは。");
    }

    #[test]
    fn test_chinese_commands() {
        let mut p = VoiceCommandProcessor::new("zh", false, &HashMap::new());
        assert_eq!(p.apply("你好 句号"), "你好。");
        // Chinese comma attaches left (no space before) but keeps space after
        assert_eq!(p.apply("你好 逗号 世界"), "你好， 世界");
    }

    #[test]
    fn test_korean_commands() {
        let mut p = VoiceCommandProcessor::new("ko", false, &HashMap::new());
        assert_eq!(p.apply("안녕 마침표"), "안녕.");
    }

    #[test]
    fn test_portuguese_commands() {
        let mut p = VoiceCommandProcessor::new("pt", false, &HashMap::new());
        assert_eq!(p.apply("olá ponto"), "olá.");
        assert_eq!(p.apply("olá vírgula mundo"), "olá, mundo");
    }

    #[test]
    fn test_italian_commands() {
        let mut p = VoiceCommandProcessor::new("it", false, &HashMap::new());
        assert_eq!(p.apply("ciao punto"), "ciao.");
        assert_eq!(p.apply("ciao virgola mondo"), "ciao, mondo");
    }

    #[test]
    fn test_dutch_commands() {
        let mut p = VoiceCommandProcessor::new("nl", false, &HashMap::new());
        assert_eq!(p.apply("hallo punt"), "hallo.");
        assert_eq!(p.apply("hallo komma wereld"), "hallo, wereld");
    }

    #[test]
    fn test_polish_commands() {
        let mut p = VoiceCommandProcessor::new("pl", false, &HashMap::new());
        assert_eq!(p.apply("cześć kropka"), "cześć.");
    }

    #[test]
    fn test_german_newline_and_caps() {
        let mut p = VoiceCommandProcessor::new("de", false, &HashMap::new());
        assert_eq!(p.apply("hallo neue zeile welt"), "hallo\nwelt");
        assert_eq!(
            p.apply("großbuchstaben hallo ende großbuchstaben welt"),
            "HALLO welt"
        );
    }

    #[test]
    fn test_processor_trait_name() {
        let p = en_processor();
        assert_eq!(PostProcessor::name(&p), "voice-commands");
    }

    #[test]
    fn test_unknown_language_falls_back_to_english() {
        let mut p = VoiceCommandProcessor::new("xx", false, &HashMap::new());
        // Should still recognize English commands as fallback
        assert_eq!(p.apply("hello period"), "hello.");
    }

    #[test]
    fn test_auto_language_uses_english() {
        let mut p = VoiceCommandProcessor::new("auto", false, &HashMap::new());
        assert_eq!(p.apply("hello period"), "hello.");
    }

    #[test]
    fn test_full_stop_alias() {
        let mut p = en_processor();
        assert_eq!(p.apply("hello full stop"), "hello.");
    }

    #[test]
    fn test_exclamation_point_alias() {
        let mut p = en_processor();
        assert_eq!(p.apply("wow exclamation point"), "wow!");
    }

    #[test]
    fn test_longer_phrase_matches_first() {
        // "new paragraph" should match before "new line"
        let mut p = en_processor();
        assert_eq!(p.apply("hello new paragraph world"), "hello\n\nworld");
    }

    #[test]
    fn test_command_at_start() {
        let mut p = en_processor();
        assert_eq!(p.apply("period hello"), ". hello");
    }

    #[test]
    fn test_consecutive_commands() {
        let mut p = en_processor();
        assert_eq!(p.apply("hello period period period"), "hello...");
    }

    #[test]
    fn test_case_insensitive_unicode_safe() {
        // U+0130 (İ) lowercases to two chars in some locales.
        // The char-by-char comparison must not panic or misalign.
        let mut p = en_processor();
        assert_eq!(
            p.apply("hello İ world"),
            "hello İ world",
            "Non-command Unicode text should pass through unchanged"
        );
    }

    // ── build_post_processors tests ─────────────────────────────────────

    #[test]
    fn test_build_post_processors_enabled_returns_one() {
        let mut config = Config::default();
        config.voice_commands.enabled = true;
        let processors = build_post_processors(&config);
        assert_eq!(
            processors.len(),
            1,
            "Enabled voice commands should produce one processor"
        );
        assert_eq!(processors[0].name(), "voice-commands");
    }

    #[test]
    fn test_build_post_processors_disabled_returns_empty() {
        let mut config = Config::default();
        config.voice_commands.enabled = false;
        let processors = build_post_processors(&config);
        assert!(
            processors.is_empty(),
            "Disabled voice commands should produce no processors"
        );
    }

    // ── New alias tests ─────────────────────────────────────────────────

    #[test]
    fn test_dot_alias() {
        let mut p = en_processor();
        assert_eq!(p.apply("hello dot"), "hello.");
    }

    #[test]
    fn test_enter_produces_newline() {
        let mut p = en_processor();
        assert_eq!(p.apply("first enter second"), "first\nsecond");
    }

    #[test]
    fn test_enter_at_end() {
        let mut p = en_processor();
        assert_eq!(p.apply("hello enter"), "hello\n");
    }

    #[test]
    fn test_double_enter() {
        let mut p = en_processor();
        assert_eq!(p.apply("hello enter enter world"), "hello\n\nworld");
    }

    #[test]
    fn test_german_enter() {
        let mut p = VoiceCommandProcessor::new("de", false, &HashMap::new());
        assert_eq!(p.apply("hallo enter welt"), "hallo\nwelt");
    }

    // ── Whisper trailing punctuation tests ───────────────────────────────

    #[test]
    fn test_whisper_enter_with_trailing_period() {
        // Whisper transcribes "Enter." when user says "enter" with falling intonation
        let mut p = en_processor();
        assert_eq!(p.apply("hello Enter."), "hello\n");
    }

    #[test]
    fn test_whisper_period_with_trailing_period() {
        // Whisper outputs "Period." — should not double the dot
        let mut p = en_processor();
        assert_eq!(p.apply("hello Period."), "hello.");
    }

    #[test]
    fn test_whisper_comma_with_trailing_comma() {
        // Whisper outputs "Comma," — should produce single comma
        let mut p = en_processor();
        assert_eq!(p.apply("hello Comma, world"), "hello, world");
    }

    #[test]
    fn test_whisper_punkt_with_trailing_period() {
        // German: Whisper outputs "Punkt."
        let mut p = VoiceCommandProcessor::new("de", false, &HashMap::new());
        assert_eq!(p.apply("hallo Punkt."), "hallo.");
    }

    #[test]
    fn test_whisper_punkt_with_trailing_ellipsis() {
        // German: Whisper outputs "Punkt..."
        let mut p = VoiceCommandProcessor::new("de", false, &HashMap::new());
        assert_eq!(p.apply("hallo Punkt..."), "hallo.");
    }

    #[test]
    fn test_whisper_komma_with_trailing_comma() {
        // German: Whisper outputs "Komma,"
        let mut p = VoiceCommandProcessor::new("de", false, &HashMap::new());
        assert_eq!(p.apply("hallo Komma, welt"), "hallo, welt");
    }

    #[test]
    fn test_whisper_fragezeichen_with_trailing_question() {
        // German: Whisper outputs "Fragezeichen?"
        let mut p = VoiceCommandProcessor::new("de", false, &HashMap::new());
        assert_eq!(p.apply("was Fragezeichen?"), "was?");
    }

    #[test]
    fn test_whisper_question_mark_with_trailing_question() {
        let mut p = en_processor();
        assert_eq!(p.apply("really Question Mark?"), "really?");
    }

    #[test]
    fn test_whisper_exclamation_mark_with_trailing_bang() {
        let mut p = en_processor();
        assert_eq!(p.apply("wow Exclamation Mark!"), "wow!");
    }

    #[test]
    fn test_whisper_trailing_punct_mid_sentence() {
        // "Enter." followed by more text
        let mut p = en_processor();
        assert_eq!(p.apply("hello Enter. world"), "hello\nworld");
    }

    #[test]
    fn test_whisper_trailing_punct_does_not_match_partial_word() {
        // "periodic." should NOT trigger "period" — the "ic" before "." prevents it
        let mut p = en_processor();
        assert_eq!(p.apply("periodic."), "periodic.");
    }

    #[test]
    fn test_whisper_dot_with_trailing_period() {
        let mut p = en_processor();
        assert_eq!(p.apply("hello Dot."), "hello.");
    }

    #[test]
    fn test_whisper_new_line_with_trailing_period() {
        let mut p = en_processor();
        assert_eq!(p.apply("hello New Line. world"), "hello\nworld");
    }

    #[test]
    fn test_trailing_punct_only_consumed_at_word_boundary() {
        // Random punctuation in the middle of a non-command word should not be consumed
        let mut p = en_processor();
        assert_eq!(p.apply("U.S.A. is great"), "U.S.A. is great");
    }

    // ── infer_action tests ───────────────────────────────────────────────

    #[test]
    fn test_infer_action_punctuation() {
        let action = infer_action(".");
        assert_eq!(action, CommandAction::punct("."));

        let action = infer_action(",");
        assert_eq!(action, CommandAction::punct(","));

        let action = infer_action("...");
        assert_eq!(action, CommandAction::punct("..."));

        let action = infer_action("!?");
        assert_eq!(action, CommandAction::punct("!?"));
    }

    #[test]
    fn test_infer_action_open_brackets() {
        assert_eq!(infer_action("("), CommandAction::open("("));
        assert_eq!(infer_action("["), CommandAction::open("["));
        assert_eq!(infer_action("{"), CommandAction::open("{"));
    }

    #[test]
    fn test_infer_action_close_brackets() {
        assert_eq!(infer_action(")"), CommandAction::close(")"));
        assert_eq!(infer_action("]"), CommandAction::close("]"));
        assert_eq!(infer_action("}"), CommandAction::close("}"));
    }

    #[test]
    fn test_infer_action_whitespace() {
        assert_eq!(infer_action("\n"), CommandAction::whitespace("\n"));
        assert_eq!(infer_action("\t"), CommandAction::whitespace("\t"));
        assert_eq!(infer_action("\n\n"), CommandAction::whitespace("\n\n"));
        assert_eq!(infer_action(" "), CommandAction::whitespace(" "));
    }

    #[test]
    fn test_infer_action_free() {
        assert_eq!(infer_action("hello"), CommandAction::free("hello"));
        assert_eq!(infer_action(":)"), CommandAction::free(":)"));
        assert_eq!(infer_action("@"), CommandAction::free("@"));
        assert_eq!(infer_action("(("), CommandAction::free("(("));
    }

    #[test]
    fn test_infer_action_empty_is_free() {
        // Empty string should not crash
        assert_eq!(infer_action(""), CommandAction::free(""));
    }

    // ── disable_defaults tests ───────────────────────────────────────────

    #[test]
    fn test_disable_defaults_no_builtins() {
        let mut p = VoiceCommandProcessor::new("en", true, &HashMap::new());
        // Built-in "period" command should not exist
        assert_eq!(p.apply("hello period"), "hello period");
    }

    #[test]
    fn test_disable_defaults_user_commands_still_work() {
        let mut overrides = HashMap::new();
        overrides.insert("dot".to_string(), ".".to_string());

        let mut p = VoiceCommandProcessor::new("en", true, &overrides);
        // Built-in "period" should not work
        assert_eq!(p.apply("hello period"), "hello period");
        // User-defined "dot" should work
        assert_eq!(p.apply("hello dot"), "hello.");
    }

    #[test]
    fn test_disable_defaults_false_keeps_builtins() {
        let mut p = VoiceCommandProcessor::new("en", false, &HashMap::new());
        // Built-in "period" should still work
        assert_eq!(p.apply("hello period"), "hello.");
    }

    // ── Override with inferred action tests ──────────────────────────────

    #[test]
    fn test_user_override_infers_punct() {
        let mut overrides = HashMap::new();
        overrides.insert("dot".to_string(), ".".to_string());

        let mut p = VoiceCommandProcessor::new("en", false, &overrides);
        // "dot" should attach left like punctuation
        assert_eq!(p.apply("hello dot"), "hello.");
    }

    #[test]
    fn test_user_override_infers_open() {
        let mut overrides = HashMap::new();
        overrides.insert("open paren".to_string(), "(".to_string());

        let mut p = VoiceCommandProcessor::new("en", false, &overrides);
        // Should attach right
        assert_eq!(p.apply("note open paren important"), "note (important");
    }

    #[test]
    fn test_user_override_infers_whitespace() {
        let mut overrides = HashMap::new();
        overrides.insert("break".to_string(), "\n".to_string());

        let mut p = VoiceCommandProcessor::new("en", false, &overrides);
        // Should attach both sides (eat surrounding spaces)
        assert_eq!(p.apply("hello break world"), "hello\nworld");
    }

    // ── builtin_commands_display tests ───────────────────────────────────

    #[test]
    fn test_builtin_commands_display_returns_pairs() {
        let commands = builtin_commands_display("en");
        assert!(
            !commands.is_empty(),
            "English should have built-in commands"
        );

        // Check that we get phrase → replacement pairs
        assert!(
            commands
                .iter()
                .any(|(phrase, replacement)| phrase == "period" && replacement == "."),
            "Should contain 'period' → '.'"
        );
    }

    #[test]
    fn test_builtin_commands_display_no_caps_commands() {
        let commands = builtin_commands_display("en");
        // Caps toggle commands (CapsOn/CapsOff) should be filtered out
        // because they don't have Insert actions
        assert!(
            !commands.iter().any(|(phrase, _)| phrase == "all caps"),
            "Should not include caps toggle commands"
        );
    }

    #[test]
    fn test_builtin_commands_display_german() {
        let commands = builtin_commands_display("de");
        assert!(!commands.is_empty());
        assert!(
            commands
                .iter()
                .any(|(phrase, replacement)| phrase == "punkt" && replacement == "."),
            "German should contain 'punkt' → '.'"
        );
    }

    #[test]
    fn test_builtin_commands_display_roundtrip() {
        use crate::pipeline::types::SinkEvent;
        // All built-in display commands should be present in actual processor
        for lang in ["en", "de", "es", "fr"] {
            let display = builtin_commands_display(lang);
            let mut p = VoiceCommandProcessor::new(lang, false, &HashMap::new());

            for (phrase, replacement) in display {
                let input = format!("test {}", phrase);
                let (output, events) = p.apply_with_events(&input);

                // Check if replacement appears in text output OR in events
                let found_in_text = output.contains(&replacement);
                let found_in_events = events.iter().any(|e| match e {
                    SinkEvent::Text(t) => t.contains(&replacement),
                    SinkEvent::KeyCombo(combo) => combo == &replacement,
                });

                assert!(
                    found_in_text || found_in_events,
                    "Language {}: phrase '{}' should produce '{}' in output '{}' or events {:?}",
                    lang,
                    phrase,
                    replacement,
                    output,
                    events
                );
            }
        }
    }

    // ── KeyCombo event tests ─────────────────────────────────────────────

    #[test]
    fn test_delete_word_produces_key_combo_event() {
        use crate::pipeline::types::SinkEvent;
        let mut p = en_processor();
        let (text, events) = p.apply_with_events("hello delete word world");
        assert_eq!(text, "hello world");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0], SinkEvent::Text("hello ".to_string()));
        assert_eq!(events[1], SinkEvent::KeyCombo("ctrl+BackSpace".to_string()));
        assert_eq!(events[2], SinkEvent::Text("world".to_string()));
    }

    #[test]
    fn test_delete_word_at_start() {
        use crate::pipeline::types::SinkEvent;
        let mut p = en_processor();
        let (text, events) = p.apply_with_events("delete word hello");
        assert_eq!(text, "hello");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], SinkEvent::KeyCombo("ctrl+BackSpace".to_string()));
        assert_eq!(events[1], SinkEvent::Text("hello".to_string()));
    }

    #[test]
    fn test_delete_word_at_end() {
        use crate::pipeline::types::SinkEvent;
        let mut p = en_processor();
        let (text, events) = p.apply_with_events("hello delete word");
        assert_eq!(text, "hello ");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], SinkEvent::Text("hello ".to_string()));
        assert_eq!(events[1], SinkEvent::KeyCombo("ctrl+BackSpace".to_string()));
    }

    #[test]
    fn test_no_key_combo_returns_empty_events() {
        let mut p = en_processor();
        let (text, events) = p.apply_with_events("hello period world");
        assert_eq!(text, "hello. world");
        assert!(
            events.is_empty(),
            "No KeyCombo matched, events should be empty"
        );
    }

    #[test]
    fn test_mixed_text_commands_and_key_combo() {
        use crate::pipeline::types::SinkEvent;
        let mut p = en_processor();
        let (text, events) = p.apply_with_events("hello comma delete word world period");
        assert_eq!(text, "hello, world.");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0], SinkEvent::Text("hello, ".to_string()));
        assert_eq!(events[1], SinkEvent::KeyCombo("ctrl+BackSpace".to_string()));
        assert_eq!(events[2], SinkEvent::Text("world.".to_string()));
    }

    #[test]
    fn test_german_delete_word() {
        use crate::pipeline::types::SinkEvent;
        let mut p = VoiceCommandProcessor::new("de", false, &HashMap::new());
        let (text, events) = p.apply_with_events("hallo wort löschen welt");
        assert_eq!(text, "hallo welt");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0], SinkEvent::Text("hallo ".to_string()));
        assert_eq!(events[1], SinkEvent::KeyCombo("ctrl+BackSpace".to_string()));
        assert_eq!(events[2], SinkEvent::Text("welt".to_string()));
    }

    #[test]
    fn test_process_with_events_default_impl() {
        // Non-VoiceCommandProcessor (default impl) returns empty events
        struct UpperProcessor;
        impl PostProcessor for UpperProcessor {
            fn process(&mut self, text: &str) -> String {
                text.to_uppercase()
            }
            fn name(&self) -> &'static str {
                "upper"
            }
        }
        let mut p = UpperProcessor;
        let (text, events) = p.process_with_events("hello");
        assert_eq!(text, "HELLO");
        assert!(events.is_empty());
    }
}
