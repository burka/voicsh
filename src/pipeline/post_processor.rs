//! Post-processing stage for transforming transcribed text before output.
//!
//! Sits between TranscriberStation and SinkStation in the pipeline.
//! The primary use case is voice commands: spoken punctuation and formatting.

use crate::config::Config;
use crate::ipc::protocol::TextOrigin;
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
            if new_text != input.text {
                if input.raw_text.is_none() {
                    input.raw_text = Some(input.text.clone());
                }
                input.text = new_text;
                input.text_origin = TextOrigin::VoiceCommand;
            }
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
    /// The `Vec<char>` is pre-computed at construction to avoid per-call allocation.
    commands: Vec<(String, Vec<char>, CommandAction)>,
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
        let mut commands: Vec<(String, Vec<char>, CommandAction)> = map
            .into_iter()
            .map(|(phrase, action)| {
                let chars = phrase.chars().collect();
                (phrase, chars, action)
            })
            .collect();
        commands.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        Self {
            commands,
            caps_active: false,
        }
    }

    /// Apply voice command replacements and produce sink events.
    ///
    /// Voice commands only match when the **entire transcribed text** is the
    /// command (possibly with Whisper trailing punctuation). If there's other
    /// text around it, the input is treated as normal speech and returned as-is.
    ///
    /// This prevents commands like "enter", "tab", "period" from eating common
    /// words that appear mid-sentence (e.g., "press enter to continue").
    fn apply_with_events(
        &mut self,
        text: &str,
    ) -> (String, Vec<crate::pipeline::types::SinkEvent>) {
        let trimmed = text.trim();

        // Try exact full-text match (command spoken as standalone utterance)
        if let Some(result) = self.try_exact_match(trimmed) {
            return result;
        }

        // No standalone match — return text as-is, only apply caps
        (self.apply_caps(text), vec![])
    }

    /// Try to match the entire input text against exactly one command.
    ///
    /// Strips Whisper trailing punctuation before comparison. Returns the
    /// command result if matched, or None.
    fn try_exact_match(
        &mut self,
        trimmed: &str,
    ) -> Option<(String, Vec<crate::pipeline::types::SinkEvent>)> {
        use crate::pipeline::types::SinkEvent;

        if trimmed.is_empty() {
            return None;
        }

        // Strip trailing Whisper punctuation (e.g., "Enter." → "Enter")
        let stripped = trimmed.trim_end_matches(is_whisper_trailing_punct);

        if stripped.is_empty() {
            return None;
        }

        let stripped_chars: Vec<char> = stripped.chars().collect();

        for (_phrase, phrase_chars, action) in &self.commands {
            if stripped_chars.len() != phrase_chars.len() {
                continue;
            }

            let chars_match = stripped_chars
                .iter()
                .zip(phrase_chars.iter())
                .all(|(s, p)| s.to_lowercase().eq(p.to_lowercase()));
            if !chars_match {
                continue;
            }

            // Exact match found — execute command
            match action {
                CommandAction::Insert {
                    text: replacement, ..
                } => {
                    return Some((replacement.clone(), vec![]));
                }
                CommandAction::CapsOn => {
                    self.caps_active = true;
                    return Some((String::new(), vec![]));
                }
                CommandAction::CapsOff => {
                    self.caps_active = false;
                    return Some((String::new(), vec![]));
                }
                CommandAction::KeyCombo(combo) => {
                    return Some((String::new(), vec![SinkEvent::KeyCombo(combo.clone())]));
                }
            }
        }

        None
    }

    /// Apply caps-lock transform to text if caps mode is active.
    fn apply_caps(&self, text: &str) -> String {
        if !self.caps_active {
            return text.to_string();
        }
        text.chars().flat_map(|c| c.to_uppercase()).collect()
    }

    /// Apply voice command replacement to text, returning the processed string.
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

/// Languages with built-in voice command support.
pub const SUPPORTED_LANGUAGES: &[&str] = &[
    "en", "de", "es", "fr", "pt", "it", "nl", "pl", "ru", "ja", "zh", "ko",
];

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
        // Symbols
        ("slash".into(), CommandAction::tight("/")),
        ("forward slash".into(), CommandAction::tight("/")),
        ("backslash".into(), CommandAction::tight("\\")),
        ("ampersand".into(), CommandAction::free("&")),
        ("and sign".into(), CommandAction::free("&")),
        ("at sign".into(), CommandAction::tight("@")),
        ("dollar sign".into(), CommandAction::open("$")),
        ("hash".into(), CommandAction::free("#")),
        ("hashtag".into(), CommandAction::open("#")),
        ("percent sign".into(), CommandAction::punct("%")),
        ("asterisk".into(), CommandAction::free("*")),
        ("underscore".into(), CommandAction::tight("_")),
        ("equal sign".into(), CommandAction::free("=")),
        ("plus sign".into(), CommandAction::free("+")),
        ("pipe".into(), CommandAction::free("|")),
        ("tilde".into(), CommandAction::free("~")),
        ("backtick".into(), CommandAction::free("`")),
        // Brackets
        ("open brace".into(), CommandAction::open("{")),
        ("close brace".into(), CommandAction::close("}")),
        ("open bracket".into(), CommandAction::open("[")),
        ("close bracket".into(), CommandAction::close("]")),
        ("less than".into(), CommandAction::open("<")),
        ("greater than".into(), CommandAction::close(">")),
        ("open angle bracket".into(), CommandAction::open("<")),
        ("close angle bracket".into(), CommandAction::close(">")),
        // Key combos
        (
            "backspace".into(),
            CommandAction::KeyCombo("BackSpace".to_string()),
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
        // Symbols
        ("schrägstrich".into(), CommandAction::tight("/")),
        ("rückschrägstrich".into(), CommandAction::tight("\\")),
        ("und zeichen".into(), CommandAction::free("&")),
        ("at zeichen".into(), CommandAction::tight("@")),
        ("dollar zeichen".into(), CommandAction::open("$")),
        ("raute".into(), CommandAction::free("#")),
        ("prozent zeichen".into(), CommandAction::punct("%")),
        ("sternchen".into(), CommandAction::free("*")),
        ("unterstrich".into(), CommandAction::tight("_")),
        ("gleichheitszeichen".into(), CommandAction::free("=")),
        ("plus zeichen".into(), CommandAction::free("+")),
        ("pipe".into(), CommandAction::free("|")),
        ("tilde".into(), CommandAction::free("~")),
        ("backtick".into(), CommandAction::free("`")),
        // Brackets
        ("geschweifte klammer auf".into(), CommandAction::open("{")),
        ("geschweifte klammer zu".into(), CommandAction::close("}")),
        ("eckige klammer auf".into(), CommandAction::open("[")),
        ("eckige klammer zu".into(), CommandAction::close("]")),
        ("spitze klammer auf".into(), CommandAction::open("<")),
        ("spitze klammer zu".into(), CommandAction::close(">")),
        // Key combos
        (
            "rücktaste".into(),
            CommandAction::KeyCombo("BackSpace".to_string()),
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

    // ── helpers ──────────────────────────────────────────────────────────

    fn en_processor() -> VoiceCommandProcessor {
        VoiceCommandProcessor::new("en", false, &HashMap::new())
    }

    fn de_processor() -> VoiceCommandProcessor {
        VoiceCommandProcessor::new("de", false, &HashMap::new())
    }

    // ── standalone command tests ─────────────────────────────────────────

    #[test]
    fn standalone_period() {
        let mut p = en_processor();
        assert_eq!(p.apply("period"), ".");
    }

    #[test]
    fn standalone_comma() {
        let mut p = en_processor();
        assert_eq!(p.apply("comma"), ",");
    }

    #[test]
    fn standalone_enter() {
        let mut p = en_processor();
        assert_eq!(p.apply("enter"), "\n");
    }

    #[test]
    fn standalone_tab() {
        let mut p = en_processor();
        assert_eq!(p.apply("tab"), "\t");
    }

    #[test]
    fn standalone_exclamation_point() {
        let mut p = en_processor();
        assert_eq!(p.apply("exclamation point"), "!");
    }

    #[test]
    fn standalone_question_mark() {
        let mut p = en_processor();
        assert_eq!(p.apply("question mark"), "?");
    }

    #[test]
    fn standalone_colon() {
        let mut p = en_processor();
        assert_eq!(p.apply("colon"), ":");
    }

    #[test]
    fn standalone_semicolon() {
        let mut p = en_processor();
        assert_eq!(p.apply("semicolon"), ";");
    }

    #[test]
    fn standalone_dash() {
        let mut p = en_processor();
        assert_eq!(p.apply("dash"), " — ");
    }

    #[test]
    fn standalone_hyphen() {
        let mut p = en_processor();
        assert_eq!(p.apply("hyphen"), "-");
    }

    #[test]
    fn standalone_new_line() {
        let mut p = en_processor();
        assert_eq!(p.apply("new line"), "\n");
    }

    #[test]
    fn standalone_new_paragraph() {
        let mut p = en_processor();
        assert_eq!(p.apply("new paragraph"), "\n\n");
    }

    #[test]
    fn standalone_open_paren() {
        let mut p = en_processor();
        assert_eq!(p.apply("open parenthesis"), "(");
    }

    #[test]
    fn standalone_close_paren() {
        let mut p = en_processor();
        assert_eq!(p.apply("close parenthesis"), ")");
    }

    #[test]
    fn standalone_open_bracket() {
        let mut p = en_processor();
        assert_eq!(p.apply("open bracket"), "[");
    }

    #[test]
    fn standalone_close_bracket() {
        let mut p = en_processor();
        assert_eq!(p.apply("close bracket"), "]");
    }

    #[test]
    fn standalone_open_quote() {
        let mut p = en_processor();
        assert_eq!(p.apply("open quote"), "\"");
    }

    #[test]
    fn standalone_close_quote() {
        let mut p = en_processor();
        assert_eq!(p.apply("close quote"), "\"");
    }

    // ── case insensitivity ──────────────────────────────────────────────

    #[test]
    fn case_insensitive_period() {
        let mut p = en_processor();
        assert_eq!(p.apply("Period"), ".");
    }

    #[test]
    fn case_insensitive_enter() {
        let mut p = en_processor();
        assert_eq!(p.apply("ENTER"), "\n");
    }

    #[test]
    fn case_insensitive_new_line() {
        let mut p = en_processor();
        assert_eq!(p.apply("New Line"), "\n");
    }

    // ── Whisper trailing punctuation ────────────────────────────────────

    #[test]
    fn whisper_trailing_dot() {
        let mut p = en_processor();
        assert_eq!(p.apply("Enter."), "\n");
    }

    #[test]
    fn whisper_trailing_dot_on_period() {
        let mut p = en_processor();
        assert_eq!(p.apply("Period."), ".");
    }

    #[test]
    fn whisper_trailing_comma() {
        let mut p = en_processor();
        assert_eq!(p.apply("Comma,"), ",");
    }

    #[test]
    fn whisper_trailing_question() {
        let mut p = en_processor();
        assert_eq!(p.apply("Enter?"), "\n");
    }

    #[test]
    fn whisper_trailing_exclamation() {
        let mut p = en_processor();
        assert_eq!(p.apply("Enter!"), "\n");
    }

    // ── commands NOT consumed mid-sentence ──────────────────────────────

    #[test]
    fn mid_sentence_enter_not_consumed() {
        let mut p = en_processor();
        assert_eq!(p.apply("hello enter world"), "hello enter world");
    }

    #[test]
    fn trailing_enter_not_consumed() {
        let mut p = en_processor();
        assert_eq!(p.apply("hello enter"), "hello enter");
    }

    #[test]
    fn mid_sentence_period_not_consumed() {
        let mut p = en_processor();
        assert_eq!(p.apply("the period of time"), "the period of time");
    }

    #[test]
    fn trailing_period_not_consumed() {
        let mut p = en_processor();
        assert_eq!(p.apply("hello period"), "hello period");
    }

    #[test]
    fn press_enter_to_continue_passthrough() {
        let mut p = en_processor();
        assert_eq!(
            p.apply("press enter to continue"),
            "press enter to continue"
        );
    }

    #[test]
    fn mid_sentence_comma_not_consumed() {
        let mut p = en_processor();
        assert_eq!(p.apply("hello comma world"), "hello comma world");
    }

    #[test]
    fn multi_command_words_not_consumed() {
        let mut p = en_processor();
        assert_eq!(
            p.apply("hello comma world period"),
            "hello comma world period"
        );
    }

    // ── passthrough / edge cases ────────────────────────────────────────

    #[test]
    fn plain_text_passthrough() {
        let mut p = en_processor();
        assert_eq!(p.apply("hello world"), "hello world");
    }

    #[test]
    fn empty_string() {
        let mut p = en_processor();
        assert_eq!(p.apply(""), "");
    }

    #[test]
    fn whitespace_only() {
        let mut p = en_processor();
        assert_eq!(p.apply("   "), "   ");
    }

    #[test]
    fn leading_trailing_whitespace_standalone() {
        let mut p = en_processor();
        // " period " trimmed is "period" → matches
        assert_eq!(p.apply(" period "), ".");
    }

    // ── caps toggle across calls ────────────────────────────────────────

    #[test]
    fn caps_on_then_text_uppercased() {
        let mut p = en_processor();
        assert_eq!(p.apply("all caps"), "");
        assert_eq!(p.apply("hello"), "HELLO");
    }

    #[test]
    fn caps_off_restores_normal() {
        let mut p = en_processor();
        assert_eq!(p.apply("all caps"), "");
        assert_eq!(p.apply("hello"), "HELLO");
        assert_eq!(p.apply("end caps"), "");
        assert_eq!(p.apply("world"), "world");
    }

    #[test]
    fn caps_with_standalone_command() {
        let mut p = en_processor();
        assert_eq!(p.apply("all caps"), "");
        // Standalone command still produces its replacement, not uppercased
        assert_eq!(p.apply("period"), ".");
    }

    #[test]
    fn caps_applied_to_passthrough_text() {
        let mut p = en_processor();
        assert_eq!(p.apply("all caps"), "");
        assert_eq!(p.apply("hello world"), "HELLO WORLD");
        assert_eq!(p.apply("end caps"), "");
        assert_eq!(p.apply("hello world"), "hello world");
    }

    // ── user overrides ──────────────────────────────────────────────────

    #[test]
    fn user_override_standalone() {
        let mut overrides = HashMap::new();
        overrides.insert("banana".to_string(), "🍌".to_string());
        let mut p = VoiceCommandProcessor::new("en", false, &overrides);
        assert_eq!(p.apply("banana"), "🍌");
    }

    #[test]
    fn user_override_not_consumed_mid_sentence() {
        let mut overrides = HashMap::new();
        overrides.insert("banana".to_string(), "🍌".to_string());
        let mut p = VoiceCommandProcessor::new("en", false, &overrides);
        assert_eq!(p.apply("I like banana"), "I like banana");
    }

    #[test]
    fn user_override_replaces_builtin() {
        let mut overrides = HashMap::new();
        overrides.insert("period".to_string(), "CUSTOM_PERIOD".to_string());
        let mut p = VoiceCommandProcessor::new("en", false, &overrides);
        assert_eq!(p.apply("period"), "CUSTOM_PERIOD");
    }

    #[test]
    fn disable_defaults_only_overrides_work() {
        let mut overrides = HashMap::new();
        overrides.insert("banana".to_string(), "🍌".to_string());
        let mut p = VoiceCommandProcessor::new("en", true, &overrides);
        // Built-in "period" no longer works
        assert_eq!(p.apply("period"), "period");
        // User override still works
        assert_eq!(p.apply("banana"), "🍌");
    }

    // ── language tests ──────────────────────────────────────────────────

    #[test]
    fn german_standalone_commands() {
        let mut p = de_processor();
        assert_eq!(p.apply("punkt"), ".");
        assert_eq!(p.apply("komma"), ",");
        assert_eq!(p.apply("enter"), "\n");
    }

    #[test]
    fn german_mid_sentence_not_consumed() {
        let mut p = de_processor();
        assert_eq!(p.apply("der punkt ist wichtig"), "der punkt ist wichtig");
    }

    #[test]
    fn spanish_standalone_commands() {
        let mut p = VoiceCommandProcessor::new("es", false, &HashMap::new());
        assert_eq!(p.apply("punto"), ".");
        assert_eq!(p.apply("coma"), ",");
    }

    #[test]
    fn french_standalone_commands() {
        let mut p = VoiceCommandProcessor::new("fr", false, &HashMap::new());
        assert_eq!(p.apply("point"), ".");
        assert_eq!(p.apply("virgule"), ",");
    }

    #[test]
    fn russian_standalone_commands() {
        let mut p = VoiceCommandProcessor::new("ru", false, &HashMap::new());
        assert_eq!(p.apply("точка"), ".");
        assert_eq!(p.apply("запятая"), ",");
    }

    #[test]
    fn japanese_standalone_commands() {
        let mut p = VoiceCommandProcessor::new("ja", false, &HashMap::new());
        assert_eq!(p.apply("句点"), "。");
        assert_eq!(p.apply("読点"), "、");
    }

    #[test]
    fn chinese_standalone_commands() {
        let mut p = VoiceCommandProcessor::new("zh", false, &HashMap::new());
        assert_eq!(p.apply("句号"), "。");
        assert_eq!(p.apply("逗号"), "，");
    }

    #[test]
    fn korean_standalone_commands() {
        let mut p = VoiceCommandProcessor::new("ko", false, &HashMap::new());
        assert_eq!(p.apply("마침표"), ".");
        assert_eq!(p.apply("쉼표"), ",");
    }

    #[test]
    fn portuguese_standalone_commands() {
        let mut p = VoiceCommandProcessor::new("pt", false, &HashMap::new());
        assert_eq!(p.apply("ponto"), ".");
        assert_eq!(p.apply("vírgula"), ",");
    }

    #[test]
    fn italian_standalone_commands() {
        let mut p = VoiceCommandProcessor::new("it", false, &HashMap::new());
        assert_eq!(p.apply("punto"), ".");
        assert_eq!(p.apply("virgola"), ",");
    }

    #[test]
    fn dutch_standalone_commands() {
        let mut p = VoiceCommandProcessor::new("nl", false, &HashMap::new());
        assert_eq!(p.apply("punt"), ".");
        assert_eq!(p.apply("komma"), ",");
    }

    #[test]
    fn polish_standalone_commands() {
        let mut p = VoiceCommandProcessor::new("pl", false, &HashMap::new());
        assert_eq!(p.apply("kropka"), ".");
        assert_eq!(p.apply("przecinek"), ",");
    }

    // ── KeyCombo events (standalone) ────────────────────────────────────

    #[test]
    fn standalone_delete_word_event() {
        let mut p = en_processor();
        let (text, events) = p.apply_with_events("delete word");
        assert_eq!(text, "");
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            crate::pipeline::types::SinkEvent::KeyCombo(_)
        ));
    }

    #[test]
    fn standalone_backspace_event() {
        let mut p = en_processor();
        let (text, events) = p.apply_with_events("backspace");
        assert_eq!(text, "");
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            crate::pipeline::types::SinkEvent::KeyCombo(_)
        ));
    }

    #[test]
    fn delete_word_mid_sentence_not_consumed() {
        let mut p = en_processor();
        let (text, events) = p.apply_with_events("please delete word now");
        assert_eq!(text, "please delete word now");
        assert!(events.is_empty());
    }

    // ── PostProcessorStation ────────────────────────────────────────────

    #[test]
    fn station_standalone_command() {
        let processor = VoiceCommandProcessor::new("en", false, &HashMap::new());
        let mut station = PostProcessorStation::new(vec![Box::new(processor)]);
        let input = TranscribedText::new("period".to_string());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, ".");
    }

    #[test]
    fn station_passthrough_text() {
        let processor = VoiceCommandProcessor::new("en", false, &HashMap::new());
        let mut station = PostProcessorStation::new(vec![Box::new(processor)]);
        let input = TranscribedText::new("hello world".to_string());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "hello world");
    }

    #[test]
    fn station_empty_result_filtered() {
        let processor = VoiceCommandProcessor::new("en", false, &HashMap::new());
        let mut station = PostProcessorStation::new(vec![Box::new(processor)]);
        let input = TranscribedText::new("all caps".to_string());
        // "all caps" returns empty text → station should return None
        let result = station.process(input).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn station_noop_processor() {
        struct NoOpPostProcessor;
        impl PostProcessor for NoOpPostProcessor {
            fn process(&mut self, text: &str) -> String {
                text.to_string()
            }
            fn name(&self) -> &'static str {
                "noop"
            }
        }
        let processor = NoOpPostProcessor;
        let mut station = PostProcessorStation::new(vec![Box::new(processor)]);
        let input = TranscribedText::new("hello period world".to_string());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "hello period world");
    }

    // ── infer_action ────────────────────────────────────────────────────

    #[test]
    fn infer_action_newline() {
        let action = infer_action("\n");
        assert!(
            matches!(action, CommandAction::Insert { text, attach_left: true, attach_right: true } if text == "\n")
        );
    }

    #[test]
    fn infer_action_double_newline() {
        let action = infer_action("\n\n");
        assert!(
            matches!(action, CommandAction::Insert { text, attach_left: true, attach_right: true } if text == "\n\n")
        );
    }

    #[test]
    fn infer_action_tab() {
        let action = infer_action("\t");
        assert!(
            matches!(action, CommandAction::Insert { text, attach_left: true, attach_right: true } if text == "\t")
        );
    }

    #[test]
    fn infer_action_period() {
        let action = infer_action(".");
        assert!(
            matches!(action, CommandAction::Insert { text, attach_left: true, attach_right: false } if text == ".")
        );
    }

    #[test]
    fn infer_action_comma() {
        let action = infer_action(",");
        assert!(
            matches!(action, CommandAction::Insert { text, attach_left: true, attach_right: false } if text == ",")
        );
    }

    #[test]
    fn infer_action_open_paren() {
        let action = infer_action("(");
        assert!(
            matches!(action, CommandAction::Insert { text, attach_left: false, attach_right: true } if text == "(")
        );
    }

    #[test]
    fn infer_action_close_paren() {
        let action = infer_action(")");
        assert!(
            matches!(action, CommandAction::Insert { text, attach_left: true, attach_right: false } if text == ")")
        );
    }

    #[test]
    fn infer_action_plain_word() {
        let action = infer_action("hello");
        assert!(
            matches!(action, CommandAction::Insert { text, attach_left: false, attach_right: false } if text == "hello")
        );
    }

    // ── builtin_commands_display ─────────────────────────────────────────

    #[test]
    fn builtin_commands_roundtrip() {
        // Every built-in command phrase should produce its replacement when used standalone
        for lang in SUPPORTED_LANGUAGES {
            let display = builtin_commands_display(lang);
            let mut processor = VoiceCommandProcessor::new(lang, false, &HashMap::new());
            for (phrase, _replacement) in &display {
                let result = processor.apply(phrase);
                // The result should NOT be the original phrase (it should be transformed)
                assert_ne!(
                    result, *phrase,
                    "Language {lang}: standalone \"{phrase}\" was not recognized as a command"
                );
            }
        }
    }

    // ── build_post_processors ────────────────────────────────────────────

    #[test]
    fn build_post_processors_voice_commands_enabled() {
        let config = Config::default();
        let processors = build_post_processors(&config);
        // Default config has voice_commands enabled
        assert!(!processors.is_empty());
        assert_eq!(processors[0].name(), "voice-commands");
    }

    #[test]
    fn build_post_processors_voice_commands_disabled() {
        let mut config = Config::default();
        config.voice_commands.enabled = false;
        let processors = build_post_processors(&config);
        assert!(processors.is_empty());
    }

    // ── performance ──────────────────────────────────────────────────────

    #[test]
    fn processing_completes_quickly() {
        // Standalone matching is O(n) where n = number of commands.
        // Verify it completes in a reasonable time for a batch.
        use std::time::Instant;
        let mut p = en_processor();
        let start = Instant::now();
        for _ in 0..10_000 {
            let _ = p.apply("hello world this is a normal sentence");
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 1000,
            "10k iterations took {elapsed:?}, expected < 1s"
        );
    }
}
