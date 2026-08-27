//! The Host port: every effect Perch has outside its own process.
//!
//! Commands take a `&dyn Host` and nothing else, so behavior tests drive the
//! real command code against [`fake::FakeHost`] and assert on outcomes rather
//! than on mocks. One port, two implementations.
//!
//! [`Host`] declares nothing itself: it is the sum of nine traits, one per kind
//! of effect. No consumer narrows to one of them, because anything that touches
//! the machine touches several of its surfaces at once — the port is 43 methods
//! wide because the machine is (ADR the-port-fits-the-machine).

use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::keychain::KeychainError;
use crate::secret::Secret;
use zeroize::Zeroizing;

/// Behind a feature so it stays out of the binary somebody downloads. It is
/// only ever reached from a test, and the tests are integration tests — they
/// link this library as any other crate would, so `#[cfg(test)]` could not
/// carry it (see the `fakes` feature in `Cargo.toml`).
#[cfg(any(test, feature = "fakes"))]
pub mod fake;
pub mod real;

#[cfg(any(test, feature = "fakes"))]
pub use fake::FakeHost;
pub use real::RealHost;

/// The result of running another program.
#[derive(Clone, PartialEq, Eq)]
pub struct Execution {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl std::fmt::Debug for Execution {
    /// The status and the sizes, never the bytes. This is the *first* shape a
    /// Credential takes: `security find-generic-password -w` answers with one on
    /// stdout, so an `Execution` returning from a keychain read holds a refresh
    /// token in a public field, and a derived `Debug` prints it.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Execution {{ status: {}, stdout: <{} bytes>, stderr: <{} bytes> }}",
            self.status,
            self.stdout.len(),
            self.stderr.len()
        )
    }
}

impl Execution {
    pub fn succeeded(&self) -> bool {
        self.status == 0
    }
}

impl From<std::process::Output> for Execution {
    /// A killed process has no exit code, and `-1` is what Perch reads it as
    /// everywhere — said here so the three programs Perch runs cannot come to
    /// disagree about it.
    fn from(output: std::process::Output) -> Execution {
        Execution {
            status: output.status.code().unwrap_or(-1),
            stdout: taken_over(output.stdout),
            stderr: taken_over(output.stderr),
        }
    }
}

/// A child's output as a `String`, moving the buffer where it can rather than
/// through `from_utf8_lossy(..).into_owned()`, which copies even for bytes that
/// are already UTF-8. `security find-generic-password` answers with a Credential
/// on stdout, so that copy is a second buffer holding a refresh token nothing
/// wipes and nothing reads again.
fn taken_over(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(not_utf8) => String::from_utf8_lossy(not_utf8.as_bytes()).into_owned(),
    }
}

/// One HTTP request, whole — rather than a URL and some arguments beside it,
/// because every part of one has to travel the same way. An access token is a
/// Credential, and a header passed on a command line sits in `argv` where any
/// process on the machine reads it off the process table.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest<'a> {
    pub url: &'a str,
    pub headers: &'a [(&'a str, &'a str)],
    /// The body to send, which is what makes the request a POST. `None` is a
    /// GET.
    pub body: Option<&'a str>,
    /// How long the whole request may take, or `None` for the ordinary bound.
    /// Part of the request rather than a setting on the Host, because the two
    /// callers want different answers: a Refresh is a figure somebody asked for
    /// and worth waiting on, and the upgrade check on `perch version` is a
    /// line nobody asked for.
    pub within_millis: Option<u64>,
}

impl std::fmt::Debug for HttpRequest<'_> {
    /// The url and the header *names*, never a header value and never the body.
    /// An access token travels as a header and the renewal's body is a refresh
    /// token outright ([`crate::anthropic::renew`]).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(name, _)| *name).collect();
        write!(
            formatter,
            "HttpRequest {{ url: {:?}, headers: {:?}, body: <{} bytes>, within_millis: {:?} }}",
            self.url,
            names,
            self.body.map_or(0, str::len),
            self.within_millis
        )
    }
}

/// The bound on a request that carries none of its own.
///
/// At the port rather than in the real adapter, because a fake that answered a
/// request after longer than this would be answering one no machine answers.
pub const ORDINARY_BOUND_MILLIS: u64 = 30_000;

impl<'a> HttpRequest<'a> {
    /// How long this request may take, whether or not it says. Rounded up to
    /// whole seconds — `curl` takes no finer unit for `max-time` — and to at
    /// least one, a bound rounding down to zero meaning *no* bound. At the port
    /// rather than in the real adapter, so a fake holds a request to the bound a
    /// machine holds it to rather than to the one that was asked for.
    pub fn bound_millis(&self) -> u64 {
        match self.within_millis {
            Some(millis) => millis.div_ceil(1_000).max(1) * 1_000,
            None => ORDINARY_BOUND_MILLIS,
        }
    }

    pub fn get(url: &'a str, headers: &'a [(&'a str, &'a str)]) -> Self {
        HttpRequest {
            url,
            headers,
            body: None,
            within_millis: None,
        }
    }

    pub fn post(url: &'a str, headers: &'a [(&'a str, &'a str)], body: &'a str) -> Self {
        HttpRequest {
            url,
            headers,
            body: Some(body),
            within_millis: None,
        }
    }

    /// The same request, given at most this long to finish.
    pub fn within(mut self, millis: u64) -> Self {
        self.within_millis = Some(millis);
        self
    }
}

/// A reply from an HTTP request.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    /// Wiped when it is freed, for the reason [`HttpResponse`]'s `Debug` is
    /// redacted: the token endpoint answers a Renewal with the rotated refresh
    /// token in here, and every other buffer on that path is already covered.
    pub body: Zeroizing<String>,
}

impl std::fmt::Debug for HttpResponse {
    /// The status and the size, never the body. The token endpoint answers a
    /// renewal with the new access token and the rotated refresh token in that
    /// body ([`crate::anthropic::renew`]), so this is the shape a Credential
    /// arrives in — redacted for the same reason the request that asked for it
    /// is.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "HttpResponse {{ status: {}, body: <{} bytes> }}",
            self.status,
            self.body.len()
        )
    }
}

/// The machine, to the resolution Perch cares about: macOS keeps secrets in a
/// keychain and no other platform does, and Windows finds programs through
/// `PATHEXT` where everything else marks them executable. An effect rather than
/// a `cfg!`, so behavior tests drive every platform's Credential Store and
/// program search from whatever machine they run on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Windows,
    Other,
}

/// How one path is made to stand for another, which is the whole of how Shared
/// State reaches the Profile a Run launches (ADR everything-but-the-account).
/// Three kinds, because only symbolic links need Developer Mode or elevation on
/// Windows. Which kind is used where is [`crate::reconcile`]'s decision; making
/// one is the Host's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// A path that names another path. What everything but Windows uses, and
    /// the kind that survives its target being replaced rather than merely
    /// edited — a new file at the same name is still the file it points at.
    Symbolic,
    /// Windows' link for a directory. Works without Developer Mode and without
    /// elevation, and exists nowhere else.
    Junction,
    /// A second name for the same file, which is why it is a share rather than
    /// a copy — and why it stops being one the moment the file it names is
    /// replaced rather than written through.
    Hard,
}

impl Link {
    /// How to say what could not be made, in a refusal a person has to act on.
    pub fn describe(self) -> &'static str {
        match self {
            Link::Symbolic => "a symbolic link",
            Link::Junction => "a directory junction",
            Link::Hard => "a hard link",
        }
    }
}

/// What `remove_link` says about a path that is not one. Both adapters spelled
/// it, twice each, and `tests/conformance.rs` asserts a substring of it — so
/// four copies had to stay in step by hand or a case quietly stopped proving
/// anything about the one somebody edited.
pub fn not_a_link(path: &Path) -> HostError {
    HostError::Other(format!(
        "{} is not a link, so it is not Perch's to remove",
        path.display()
    ))
}

/// The same for the link kind only one platform makes.
pub fn junctions_are_windows_only() -> HostError {
    HostError::Other("a directory junction is a Windows link, and this is not Windows".to_string())
}

/// How a wait ended: on its own, or because the loop was asked to stop. Its own
/// type rather than a `bool`, which two callers would read in opposite
/// directions — a wait that "returned true" is either one that completed or one
/// that was cut short.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waited {
    /// The whole of the wait passed.
    Fully,
    /// Ctrl-C arrived. Whatever the loop was doing is finished; it must not
    /// start anything else.
    Interrupted,
}

/// The permissions a file holding a Credential is created with, and the mode
/// anything looser is tightened to: the owner, and nobody else.
pub const PRIVATE_FILE_MODE: u32 = 0o600;

/// The same for the directory it sits in. A directory others may enter is a
/// directory whose contents others may open.
pub const PRIVATE_DIR_MODE: u32 = 0o700;

/// A value as a double-quoted token, written into a buffer the caller owns, for
/// the two line-oriented protocols Perch writes: `curl`'s configuration file and
/// `security -i`'s command lines. One copy, because two is how one of them gets
/// fixed and the other does not. It makes a value a *token* and not inert —
/// neither protocol escapes a newline, so one is refused where it enters.
pub fn write_double_quoted(out: &mut Secret, value: &str) {
    out.push('"');
    write_escaped(out, value);
    out.push('"');
}

/// The escaping alone, for a token assembled out of more than one piece: a
/// `curl` `header` line is one quoted value holding a name, `: ` and a value,
/// and quoting the three separately would put quotes inside the token.
pub fn write_escaped(out: &mut Secret, value: &str) {
    for c in value.chars() {
        if c == '\\' || c == '"' {
            out.push('\\');
        }
        out.push(c);
    }
}

/// The refusal [`write_double_quoted`] says has to happen somewhere else. A newline in
/// a `curl` configuration ends the option it was in and begins another, and
/// `output =` writes a file while a second `url =` fetches one. Perch does not
/// author most of what goes through here: an access token is read out of a JSON
/// file it does not own, where `\n` is an ordinary escape.
pub fn inert(what: impl std::fmt::Display, value: &str) -> Result<(), HostError> {
    match control_character_in(value) {
        Some(said) => Err(HostError::Other(format!(
            "{what} carries {said}, which the line it would be written on has \
             no way to hold as part of a value"
        ))),
        None => Ok(()),
    }
}

/// What no adapter may send, whichever one is answering.
///
/// The URL, every header value and the body are lines of a `curl` configuration
/// file, and a `@` opening the body is a filename to it. Here rather than in the
/// real adapter, so the fake cannot answer a request no machine would make.
pub fn sendable(request: &HttpRequest<'_>) -> Result<(), HostError> {
    inert("the URL", request.url)?;
    for (name, value) in request.headers {
        // The name as well as the value: `curl_config` writes the pair onto one
        // line, so a newline in either ends the `header` option and starts a
        // second one of the adapter's choosing.
        inert(format_args!("the header name `{name}`"), name)?;
        inert(format_args!("the {name} header"), value)?;
    }
    if let Some(body) = request.body {
        inert("the request body", body)?;
        if body.starts_with('@') {
            return Err(HostError::Other(
                "the request body begins with `@`, which curl reads as a \
                 filename rather than as data"
                    .to_string(),
            ));
        }
    }
    Ok(())
}

/// Which characters a terminal acts on rather than draws. `Cc` is the first two
/// ranges; the rest is Unicode's `Default_Ignorable_Code_Point`, taken whole
/// because picked by hand it grew holes: `U+FE00` after a letter with no variant
/// form, `U+3164`, and `U+2065` in the gap between two ranges written as two.
/// Data, so a past version of the name rules can hold the set it enforced.
pub const UNSHOWABLE: &[(char, char)] = &[
    ('\u{0000}', '\u{001F}'),
    ('\u{007F}', '\u{009F}'),
    ('\u{00AD}', '\u{00AD}'),
    ('\u{034F}', '\u{034F}'),
    ('\u{061C}', '\u{061C}'),
    ('\u{115F}', '\u{1160}'),
    ('\u{17B4}', '\u{17B5}'),
    ('\u{180B}', '\u{180F}'),
    ('\u{200B}', '\u{200F}'),
    ('\u{202A}', '\u{202E}'),
    ('\u{2060}', '\u{206F}'),
    ('\u{3164}', '\u{3164}'),
    ('\u{FE00}', '\u{FE0F}'),
    ('\u{FEFF}', '\u{FEFF}'),
    ('\u{FFA0}', '\u{FFA0}'),
    ('\u{FFF0}', '\u{FFF8}'),
    ('\u{1BCA0}', '\u{1BCA3}'),
    ('\u{1D173}', '\u{1D17A}'),
    ('\u{E0000}', '\u{E0FFF}'),
];

/// Whether a character is in a set of ranges. Public because a set older than
/// this one is asked the same question, from `name`.
pub fn within(set: &[(char, char)], c: char) -> bool {
    set.iter().any(|(from, to)| c >= *from && c <= *to)
}

/// Whether a terminal acts on this character rather than drawing it.
pub fn is_unshowable(c: char) -> bool {
    within(UNSHOWABLE, c)
}

/// Whether a character's effect is on the character beside it, composing with
/// it into one glyph rather than rearranging, hiding or mirroring text. Kept by
/// [`Shown`], refused by [`unshowable_character_in`] all the same. Carved out of
/// [`is_unshowable`] by hand, as that set is.
pub fn composes_with_its_neighbor(c: char) -> bool {
    matches!(c,
        // The combining grapheme joiner, and the Mongolian free variation
        // selectors, which the vowel separator `U+180E` sits in the middle of
        // without being one of.
        '\u{034F}' | '\u{180B}'..='\u{180D}' | '\u{180F}'
        // The conjoining jamo fillers, which hold an empty slot in a Hangul
        // syllable block. `U+3164` and `U+FFA0` stand alone, and go.
        | '\u{115F}'..='\u{1160}'
        // The Khmer inherent vowels, drawn as part of the consonant they follow.
        | '\u{17B4}'..='\u{17B5}'
        // The zero width joiner, which makes one glyph of the emoji on either
        // side of it. Its non-joiner `U+200C` breaks one apart instead.
        | '\u{200D}'
        // The variation selectors and their supplement: which form of the
        // character before them a font draws.
        | '\u{FE00}'..='\u{FE0F}'
        | '\u{E0100}'..='\u{E01EF}')
}

/// Text on its way to a terminal, with what a terminal acts on rather than
/// draws taken out: [`is_unshowable`], less [`composes_with_its_neighbor`].
///
/// A type because the rule had six call sites and no reader: three refused
/// their own copy of it and three drew a value (ADR nothing-drawn-is-obeyed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown(String);

impl Shown {
    /// Whatever a terminal would draw of `text`, which is all of it for
    /// everything Perch authors. Nothing to refuse with, on purpose.
    pub fn of(text: &str) -> Shown {
        Shown::keeping(text, |_| false)
    }

    /// The same for a paragraph rather than a cell, where a newline is Perch's
    /// own rather than something a terminal acts on.
    ///
    /// Two constructors because the two *writers* differ rather than because a
    /// caller decides: a cell is one line and a refusal is several.
    pub fn in_prose(text: &str) -> Shown {
        Shown::keeping(text, |c| c == '\n')
    }

    fn keeping(text: &str, kept: impl Fn(char) -> bool) -> Shown {
        // Beside `kept` rather than inside it: `kept` is what one writer holds
        // on to, and this is true of every writer.
        let goes = |c: char| is_unshowable(c) && !composes_with_its_neighbor(c) && !kept(c);
        match text.chars().any(goes) {
            // Filtering unconditionally would walk and rebuild every cell of
            // every table, and nearly none of them holds one of these.
            false => Shown(text.to_string()),
            true => Shown(text.chars().filter(|c| !goes(*c)).collect()),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Shown {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A character's code point, parenthesized, as every refusal naming one spells
/// it. One copy: three callers name a character to a person, and two spellings
/// of a code point is two answers to one question.
pub fn code_point_of(c: char) -> String {
    format!("(U+{:04X})", c as u32)
}

/// The first character in a value that a line cannot hold, named the way a
/// refusal names one.
///
/// `Cc` alone: the harm at all three callers is framing, and a character that
/// only fails to draw holds fine on a line.
pub fn control_character_in(value: &str) -> Option<String> {
    value
        .chars()
        .find(|c| c.is_control())
        .map(|found| format!("a control character {}", code_point_of(found)))
}

/// The first character in a value that a terminal will not draw as itself,
/// named the way a refusal names one.
///
/// The wider question ([`is_unshowable`]), for the one caller whose harm is
/// drawing rather than framing and whose value somebody chose and can change.
pub fn unshowable_character_in(value: &str) -> Option<String> {
    value.chars().find(|c| is_unshowable(*c)).map(|found| {
        let kind = match found.is_control() {
            true => "a control character",
            // Not "draws as nothing": `U+202E` draws as nothing *and* reverses
            // what follows it, and a filler is a letter with no glyph. What
            // both have is that the terminal does not draw them as themselves.
            false => "a character a terminal does not draw as itself",
        };
        format!("{kind} {}", code_point_of(found))
    })
}

/// A command as somebody would have typed it, for the line printed before it is
/// run or the one that says what failed. One copy: `perch upgrade` and
/// `perch watcher install` are read side by side, so two spellings of "as
/// somebody would have typed it" is two answers to one question.
pub fn as_typed(program: &Path, args: &[String]) -> String {
    let mut line = program.to_string_lossy().into_owned();
    for arg in args {
        line.push(' ');
        line.push_str(arg);
    }
    line
}

/// Whether a mode lets anybody but the owner near the file.
pub fn is_private(mode: u32) -> bool {
    mode & 0o077 == 0
}

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("{path} does not exist")]
    NotFound { path: PathBuf },

    /// A directory that was to be created by whoever got there first, and
    /// somebody else did. The whole of what makes a lock a lock.
    #[error("{path} already exists")]
    AlreadyExists { path: PathBuf },

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

/// What time it is, as an effect like any other so that a test can arrange the
/// age of an observation rather than wait for one.
pub trait Clock {
    /// The current instant. Utilization is displayed as an observation with an
    /// age (ADR a-figure-carries-its-age).
    fn now(&self) -> DateTime<Utc>;
}

/// Where this Perch is running: whose machine, which platform, and what it was
/// told by the environment it was started in.
pub trait Environment {
    /// The home directory, from `USERPROFILE` on Windows and `HOME` elsewhere
    /// — or an error when the platform's variable is unset, because a machine
    /// that cannot say where home is must be refused rather than quietly
    /// worked under the filesystem root.
    fn home_dir(&self) -> Result<PathBuf, HostError>;

    /// The directory this command was typed in, which is the project a Run is
    /// about: Claude Code keys the trust it was given and the tools it was
    /// allowed by exactly this path.
    fn current_dir(&self) -> Result<PathBuf, HostError>;

    /// What a variable is set to, or `None` for one that is unset, empty, or
    /// set to something that is not text. The last two fold in with "unset"
    /// because there is nothing usable to hand back either way, and a value that
    /// was *there* and could not be read is a remark on the way past.
    fn env_var(&self, key: &str) -> Option<String>;

    /// Which platform this is, which is what decides where a Credential is
    /// written.
    fn platform(&self) -> Platform;

    /// Where this Perch's own binary sits, with links followed — the only
    /// evidence there is about which Channel installed it, since every Channel
    /// installs the same bytes (ADR an-upgrade-asks-its-channel). Homebrew is
    /// why the links are followed: what a person runs is `<prefix>/bin/perch`,
    /// and the Cellar it points into is the half that names the Channel.
    fn current_exe(&self) -> Result<PathBuf, HostError>;

    /// Which user this process is running as, or `None` on Windows, where a
    /// logon task names the user it runs as and there is neither a uid to quote
    /// nor a root to refuse. Two things need it and both are `perch watcher`'s
    /// (ADR the-machine-runs-the-watcher): the root refusal, and the `gui/<uid>`
    /// domain a LaunchAgent is bootstrapped into.
    fn user_id(&self) -> Option<u32>;
}

/// The filesystem, to the resolution a Credential's mode and a lock's
/// atomicity need of it.
pub trait Files {
    fn read_file(&self, path: &Path) -> Result<String, HostError>;

    /// Writes a file created with exactly this mode, rather than with whatever
    /// the process umask happens to be: a file opened and then `chmod`ed is a
    /// file that was briefly readable. Where a mode means nothing this is an
    /// ordinary write.
    fn create_file_with_mode(
        &self,
        path: &Path,
        contents: &str,
        mode: u32,
    ) -> Result<(), HostError>;

    /// Writes a file nobody but its owner can read, creating it — and any
    /// directory above it — with that mode rather than tightening it afterwards.
    /// A `chmod` after the fact leaves the secret readable for as long as the
    /// two calls take, which is the whole of what the mode is for.
    fn write_private_file(&self, path: &Path, contents: &str) -> Result<(), HostError>;

    /// Creates a directory, and any above it, that nobody but its owner may
    /// enter. Like `mkdir -p`, a directory that is already there keeps the mode
    /// it has: this sets a mode at creation and is not a `chmod` in disguise.
    fn create_private_dir_all(&self, path: &Path) -> Result<(), HostError>;

    /// A file's permission bits, or `None` on a platform that does not answer
    /// in those terms.
    fn file_mode(&self, path: &Path) -> Result<Option<u32>, HostError>;

    /// Narrows an existing file to its owner. The one `chmod` Perch performs:
    /// files it creates get their mode at creation.
    fn make_private(&self, path: &Path) -> Result<(), HostError>;
    fn create_dir_all(&self, path: &Path) -> Result<(), HostError>;
    fn path_exists(&self, path: &Path) -> bool;

    /// Whether a path is a file, rather than absent or a directory — what a
    /// program search means by "found", where `path_exists` would let a
    /// directory that happens to carry the name win the walk.
    fn is_file(&self, path: &Path) -> bool;
    fn remove_dir_all(&self, path: &Path) -> Result<(), HostError>;

    /// Creates a directory only if nobody else has, reporting
    /// [`HostError::AlreadyExists`] when somebody has. `mkdir` either creates a
    /// directory or fails, with no window in between, which is why Claude Code's
    /// lock artifacts are directories and why Perch's are too.
    fn create_dir_exclusive(&self, path: &Path) -> Result<(), HostError>;

    /// When a path was last written. A lock artifact's age is what says whether
    /// the process that took it is still alive or died holding it.
    fn modified_at(&self, path: &Path) -> Result<DateTime<Utc>, HostError>;

    /// Marks a path as written now, without changing it. How a lock holder says
    /// it is still there within the interval the protocol allows.
    fn touch(&self, path: &Path) -> Result<(), HostError>;

    /// What a directory holds, as full paths. Absent directories report
    /// [`HostError::NotFound`] rather than emptiness: "nothing is running" and
    /// "nowhere to look" are different answers.
    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>, HostError>;

    /// Moves a path over another, replacing it. Within one filesystem this is
    /// the operation that has no half-way state, which is what
    /// [`write_atomically`] is built out of.
    fn rename(&self, from: &Path, to: &Path) -> Result<(), HostError>;

    /// Removes a file. A file that was not there is not a failure.
    fn remove_file(&self, path: &Path) -> Result<(), HostError>;
}

/// How one path is made to stand for another. [`Link`] says which kinds there
/// are and [`crate::reconcile`] chooses between them; making, reading and
/// removing one is the machine's.
pub trait Links {
    /// Makes `at` a link of `kind` standing for `target`. A kind the platform
    /// will not make is an error rather than a quieter kind substituted for it:
    /// which link was made decides what happens when the target is replaced, so
    /// the caller chooses and hears about it.
    fn link(&self, kind: Link, target: &Path, at: &Path) -> Result<(), HostError>;

    /// What a path links to, or `None` when what is there is not a link.
    /// Answers for the link itself, so a link whose target has gone is still a
    /// link and [`HostError::NotFound`] is a third answer. A hard link is not a
    /// link here and cannot be: it is a name for a file, indistinguishable from
    /// the file's first name.
    fn link_target(&self, path: &Path) -> Result<Option<PathBuf>, HostError>;

    /// Removes a link without touching what it points at. Its own operation
    /// because the platforms disagree about which call takes one: a Windows
    /// junction is removed as a directory and a file symlink as a file, and
    /// `remove_dir_all` on either would be a walk into somebody else's
    /// directory.
    fn remove_link(&self, path: &Path) -> Result<(), HostError>;
}

/// The filesystem and the links into it, together — the one narrowing of this
/// port that anything takes. `tests/conformance.rs` drives both adapters through
/// a scratch directory, which is exactly what these two concerns are, so its
/// table takes a `&dyn Filesystem` and a case there reaching for
/// [`Clock::now`] does not compile.
pub trait Filesystem: Files + Links {}

/// The Credential Store macOS keeps, as `/usr/bin/security` presents it — which
/// is the binary Claude Code drives, and so the one Perch has to drive too. It
/// is also what decides where a Credential is written at all, and what a mode
/// on a file holding one is for (ADR claude-code-chooses-the-store).
pub trait Keys {
    fn keychain_get(&self, service: &str, account: &str) -> Result<String, KeychainError>;
    fn keychain_set(&self, service: &str, account: &str, secret: &str)
    -> Result<(), KeychainError>;
    fn keychain_delete(&self, service: &str, account: &str) -> Result<(), KeychainError>;
}

/// Other programs, and the processes Perch reads to find out what is holding
/// what.
pub trait Processes {
    fn exec(&self, program: &str, args: &[&str]) -> Result<Execution, HostError>;

    /// Runs a program with the terminal attached and `env` added to its
    /// environment. The one execution Perch does not capture, because both
    /// callers are somebody's session rather than something Perch reads.
    /// `args` reach the program one word per element and are neither re-quoted
    /// nor split: a Run forwards what somebody typed after `--`.
    fn exec_interactive(
        &self,
        program: &str,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> Result<i32, HostError>;

    /// This process, as the operating system names it. What a Run marks its
    /// Profile Live with (ADR a-run-is-one-shot): Perch waits for the program it
    /// launched, so its own pid is alive for exactly as long as the Run and no
    /// longer — and it is knowable *before* the launch, where the child's is
    /// not.
    fn process_id(&self) -> u32;

    /// Whether a process is still running. A Live Profile's Credential is
    /// untouchable because something else is holding it. Narrowed by
    /// [`is_a_pid`] first, as [`Processes::process_started_at`] is.
    fn process_alive(&self, pid: u32) -> bool;

    /// When a process began, or `None` where there is no saying. What
    /// corroborates a session marker (ADR a-profile-is-live-by-evidence): a
    /// marker is evidence only when the process it names began no later than the
    /// marker says the session did, because a recycled PID necessarily belongs
    /// to a process that began after the marker was written.
    fn process_started_at(&self, pid: u32) -> Option<DateTime<Utc>>;
}

/// Whether a number names a process at all. `probe::clients_in` parses one out
/// of any filename, so a stray `0.json` reaches the port — and `0` is a process
/// group to `kill` and the kernel to macOS's `proc_pidinfo`, which answers it a
/// start time at boot that is older than every session. Both questions ask it:
/// a start time is believed without `process_alive` ever being consulted.
pub fn is_a_pid(pid: u32) -> bool {
    pid != 0 && pid != u32::MAX
}

/// Time passing, and being asked to stop waiting. One trait rather than two,
/// because [`Waited::Interrupted`] means nothing until
/// [`Waiting::listen_for_interrupts`] has been called. `sleep` is filed here
/// rather than among the processes: it is [`crate::lock`]'s contention wait and
/// has nothing to do with a process.
pub trait Waiting {
    /// Waits. Contending for a lock is the only thing Perch waits on, and it is
    /// an effect like any other so that tests do not spend the time.
    fn sleep(&self, millis: u64);

    /// Starts listening for a loop being asked to stop. `perch watcher run`
    /// calls this and nothing else does: every other command is over long
    /// before anybody could ask (ADR a-watcher-knob-is-arithmetic). Ctrl-C is
    /// the person's and `SIGTERM` is a service manager's, and both are the same
    /// request answered in the same place.
    fn listen_for_interrupts(&self);

    /// Whether that request has arrived, asked where there is no wait to end.
    /// A round is bounded by the network rather than by the clock, so a stop
    /// has to be answerable between the steps of one.
    fn asked_to_stop(&self) -> bool;

    /// Waits up to `millis`, and stops the moment that has been asked for. Its
    /// own effect rather than a [`Waiting::sleep`] with a check around it,
    /// because what makes a foreground watcher killable is that the wait ends
    /// when Ctrl-C arrives rather than two and a half minutes later. Nothing
    /// may be held across it, so a process killed here leaves nothing behind.
    fn wait(&self, millis: u64) -> Waited;
}

/// The person at the terminal, where there is one.
pub trait Terminal {
    /// Whether there is someone to answer a question. Every capability is
    /// available non-interactively, so a command that would have asked has to
    /// know when it cannot.
    fn is_interactive(&self) -> bool;

    /// One line of input, or `None` at end of input.
    fn read_line(&self) -> Result<Option<String>, HostError>;

    /// One line of input never shown as it is typed, or `None` at end of input.
    /// Its own effect rather than a flag, because the caller who forgets the
    /// flag writes an export passphrase into somebody's scrollback. A platform
    /// with no way to hide what is typed refuses rather than showing it
    /// (ADR the-holdings-go-out-sealed). `Zeroizing` is the adapter's to owe.
    fn read_secret(&self) -> Result<Option<Zeroizing<String>>, HostError>;

    /// Says something the user should know that is not the answer to what they
    /// asked: a Credential written to the store Perch would rather not have
    /// used, a file found looser than it should be. Said once — these are
    /// remarks about the machine rather than about the command, and the same one
    /// repeated for five Accounts teaches nobody anything.
    fn note(&self, line: &str);
}

/// The way out to Anthropic, and the only one.
pub trait Network {
    /// Sends one request and reads the whole reply. The only way out to
    /// Anthropic, and reached by nothing but `--refresh`.
    fn http(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, HostError>;
}

/// Every effect Perch has outside its own process: the sum of the nine concerns
/// above and nothing of its own. Commands and domain modules take a
/// `&dyn Host`, which is the one port Perch has
/// (ADR a-crate-must-not-cost-a-seam) — the nine are names for its surfaces
/// rather than nine ports, and a `&dyn Host` reaches every one of them.
pub trait Host:
    Clock + Environment + Filesystem + Keys + Processes + Waiting + Terminal + Network
{
}

/// Everything that has to be in scope to reach this port on a **concrete**
/// adapter, which is every test and nothing else: a trait object finds its
/// supertraits' methods unimported, and `host.now()` on a `FakeHost` is
/// [`Clock::now`]. A glob, because which nine-tenths a test touches is not a
/// fact about the test.
pub mod prelude {
    pub use super::{
        Clock, Environment, Files, Filesystem, Host, Keys, Links, Network, Processes, Terminal,
        Waiting,
    };
}

/// Where the replacement for a file is written before it is moved over it,
/// named after the process writing it: a fixed name is one two Perches collide
/// on, and one anybody who can write the directory can pre-plant something at.
/// The pid comes through the port rather than from `std::process::id`, because
/// behind a fake there is a different answer to who this process is.
pub fn temp_beside(host: &dyn Host, path: &Path) -> PathBuf {
    let mut beside = path.as_os_str().to_os_string();
    beside.push(temp_suffix(host.process_id()));
    PathBuf::from(beside)
}

/// What [`temp_beside`] puts after the target's name. Its own function because
/// the fake strips it to decide which file a write beside one is *for*, and a
/// spelling that drifted would leave every fixture aimed at a private write
/// quietly arranging nothing.
pub fn temp_suffix(pid: u32) -> String {
    format!(".perch-tmp.{pid}")
}

/// The mode a replacement for this file should be created with: the one the file
/// already has, and the Credential mode for one that is not there yet. A rename
/// puts a *new* file at the path, so the old mode has to be carried across
/// deliberately — an MCP server entry in `.claude.json` routinely carries an API
/// key, so a file the user had narrowed must not come back at the umask.
fn mode_to_carry_across(host: &dyn Host, path: &Path) -> u32 {
    host.file_mode(path)
        .ok()
        .flatten()
        .unwrap_or(PRIVATE_FILE_MODE)
}

/// Replaces a file's contents in one step, or not at all. Used for the files
/// Perch does not own: `.claude.json` holds everything that belongs to the
/// person rather than the Account, and a Switch that died half way through
/// writing it would cost them all of it.
pub fn write_atomically(host: &dyn Host, path: &Path, contents: &str) -> Result<(), HostError> {
    let path = &through_any_link(host, path);
    // The temp file carries the target's mode from the moment it exists: it
    // holds the whole of the target's contents, so one at the umask would leak
    // everything the mode on the target is protecting.
    replace_via_tmp(host, path, contents, mode_to_carry_across(host, path))
}

/// What a path names, following one symbolic link if the last component is one.
/// `rename` replaces the **link**, so a `~/.claude.json` managed by stow or
/// chezmoi would silently stop being the live copy. One hop, which is what a
/// dotfile manager makes. Deliberately not in [`replace_via_tmp`]: following a
/// link to decide where a *secret* lands is how a planted link redirects one.
pub(crate) fn through_any_link(host: &dyn Host, path: &Path) -> PathBuf {
    let Ok(Some(target)) = host.link_target(path) else {
        return path.to_path_buf();
    };
    against(path, target)
}

/// Where a link's target lands, given where the link sits. `ln -s`, stow and
/// chezmoi all record a relative target unless handed an absolute path, and a
/// relative one is read against the link's own directory.
pub(crate) fn against(link_at: &Path, target: PathBuf) -> PathBuf {
    match target.is_absolute() {
        true => target,
        false => link_at.parent().unwrap_or(Path::new("")).join(target),
    }
}

/// A path with every link on it followed, which is the only shape *is this
/// inside that directory* is asked in. A type rather than a `PathBuf`:
/// `starts_with` matches components, so `~/claude` linked at
/// `~/.config/perch/profiles` is one directory whose two spellings share none
/// (ADR an-invariant-gets-a-door).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Settled(Option<PathBuf>);

/// [`Settled`] for a path, following every link on the way. Bounded, so a loop
/// of links is an answer rather than a walk that never returns.
pub(crate) fn settled(host: &dyn Host, path: &Path) -> Settled {
    const FOLLOWED: usize = 8;

    let mut at = flattened(&from_the_root(host, path));
    for _ in 0..FOLLOWED {
        match deepest_link_on(host, &at) {
            Some(followed) => at = flattened(&followed),
            None => return Settled(Some(at)),
        }
    }
    Settled(None)
}

/// A path read from wherever Perch was run, where it does not name one from the
/// root. `starts_with` matches components, so a bare `backup.age` shares none
/// with the directory it is sitting in and every containment question about it
/// answers "elsewhere".
fn from_the_root(host: &dyn Host, path: &Path) -> PathBuf {
    // Asked of the platform the *Host* reports, and joined with `/` by hand, for
    // `probe::rooted`'s reason: `is_absolute` and `join` read the separator of
    // the platform this build runs on.
    let typed = path.to_string_lossy();
    if typed.is_empty() || crate::probe::rooted(&typed, host.platform() == Platform::Windows) {
        return path.to_path_buf();
    }
    match host.current_dir() {
        Ok(here) => PathBuf::from(format!("{}/{typed}", here.display())),
        Err(_) => path.to_path_buf(),
    }
}

/// `.` and `..` taken out, so what is left is a path of names: the walk below
/// ends at a `..`, leaving links above it unfollowed and the components either
/// side compared as spelling. Lexical, so `link/..` is where the link sits,
/// which is what a shell answers and what somebody typing a path means.
pub(crate) fn flattened(path: &Path) -> PathBuf {
    // Nearly every path Perch handles has neither, and rebuilding one that has
    // neither is a `Vec` of components and a `PathBuf` grown a component at a
    // time to arrive back at the input.
    if !path
        .components()
        .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return path.to_path_buf();
    }
    let mut kept: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match kept.last() {
                Some(Component::Normal(_)) => drop(kept.pop()),
                // `/..` is `/`, and a relative path's leading `..` is part of
                // where it points rather than something to cancel.
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => kept.push(component),
            },
            named => kept.push(named),
        }
    }
    let flat: PathBuf = kept.iter().collect();
    // A path that cancels itself out is this directory, which is a name; the
    // empty path is not one, and `deepest_link_on` would read it as the root.
    if flat.as_os_str().is_empty() && !path.as_os_str().is_empty() {
        return PathBuf::from(".");
    }
    flat
}

impl Settled {
    /// Whether this path is `outer` or sits under it.
    ///
    /// A path that would not settle is inside whatever it is asked about: every
    /// caller refuses or skips on `true`, so the answer a loop of links leaves
    /// undecided is the one that acts.
    #[allow(
        clippy::disallowed_methods,
        reason = "the one containment comparison, on the one shape resolved for it"
    )]
    pub(crate) fn is_inside(&self, outer: &Settled) -> bool {
        match (&self.0, &outer.0) {
            (Some(inner), Some(outer)) => inner.starts_with(outer),
            _ => true,
        }
    }

    /// Whether this path and `other` name one place.
    ///
    /// A path that would not settle is nowhere rather than everywhere, which is
    /// [`Settled::is_inside`]'s answer turned over: both callers of that refuse
    /// or skip on `true`, and this one goes on to act on the two as one.
    pub(crate) fn is_the_same_place_as(&self, other: &Settled) -> bool {
        match (&self.0, &other.0) {
            (Some(here), Some(there)) => here == there,
            _ => false,
        }
    }

    /// Where the walk landed, for the cases that are about the walk itself.
    #[cfg(test)]
    fn at(&self) -> Option<&Path> {
        self.0.as_deref()
    }
}

/// Both halves of the question for a caller that asks it once. A caller asking
/// it of many paths settles the fixed side itself.
pub(crate) fn is_inside(host: &dyn Host, inner: &Path, outer: &Path) -> bool {
    settled(host, inner).is_inside(&settled(host, outer))
}

/// The other question about two paths, asked in the one shape it has an answer
/// in: `==` on two `Path`s compares spellings, and a directory reached through a
/// link has a spelling sharing no component with its own.
pub(crate) fn is_the_same_place(host: &dyn Host, one: &Path, other: &Path) -> bool {
    settled(host, one).is_the_same_place_as(&settled(host, other))
}

/// The deepest component of `path` that is a link, replaced by what it points
/// at and the rest of the path put back on the end. `None` when none of them is
/// a link, which is what ends the walk above.
fn deepest_link_on(host: &dyn Host, path: &Path) -> Option<PathBuf> {
    // `ancestors` yields the path itself first and then each parent, borrowed —
    // where walking with `parent()` built a `PathBuf` per level and a second one
    // re-growing the tail, which is the tail copied once per level.
    for at in path.ancestors().take_while(|at| !at.as_os_str().is_empty()) {
        if let Ok(Some(target)) = host.link_target(at) {
            let beneath = path.strip_prefix(at).ok()?;
            return Some(against(at, target).join(beneath));
        }
    }
    None
}

/// The whole of writing beside a file and moving the result over it, including
/// what is done about a write that did not land. Shared by the secret and
/// non-secret writes: two copies is how the failure cleanup comes to differ
/// between the path that handles Credentials and the path that does not.
pub fn replace_via_tmp(
    host: &dyn Host,
    path: &Path,
    contents: &str,
    mode: u32,
) -> Result<(), HostError> {
    let beside = temp_beside(host, path);
    // Cleared on both failures rather than on the rename alone: a disk that
    // fills between `create_new` and `sync_all` leaves a partially-written file
    // beside the target that nothing mentions and nothing looks at again.
    let written = host.create_file_with_mode(&beside, contents, mode);
    match written.and_then(|()| host.rename(&beside, path)) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = host.remove_file(&beside);
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {

    /// The one difference between the two constructors, and the reason there
    /// are two: every refusal Perch writes is a paragraph, and a cell is a cell.
    #[test]
    fn a_paragraph_keeps_its_newlines_and_a_cell_has_none_to_keep() {
        let refusal = "Nothing was switched.\nRun it again.";

        assert_eq!(Shown::in_prose(refusal).as_str(), refusal);
        assert_eq!(
            Shown::of(refusal).as_str(),
            "Nothing was switched.Run it again."
        );
        assert_eq!(
            Shown::in_prose("a\u{1b}[2Kb\nc\u{202e}d").as_str(),
            "a[2Kb\ncd",
            "and everything else a terminal acts on goes from either"
        );
    }

    /// The strip and the refusal ask one set two questions, and this is the
    /// half where the answers differ: a character deciding how its neighbor
    /// draws is not one a terminal obeys.
    #[test]
    fn what_composes_with_the_character_beside_it_survives_the_strip() {
        for whole in [
            // In order: a presentation selector, a ZWJ sequence, a jungseong
            // filler holding an empty slot, an ideographic variation selector,
            // a Khmer inherent vowel, a Mongolian one, a grapheme joiner.
            "Acme \u{2600}\u{FE0F}",
            "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
            "\u{1100}\u{1160}\u{11A8}",
            "\u{845B}\u{E0100}",
            "\u{17A2}\u{17B4}",
            "\u{1820}\u{180B}",
            "\u{0915}\u{034F}\u{0916}",
        ] {
            assert_eq!(Shown::of(whole).as_str(), whole);
            assert_eq!(Shown::in_prose(whole).as_str(), whole);
        }
    }

    /// `U+202E` is the test this rule has to survive: its effect is on the
    /// characters beside it too, and reversing a line is control rather than
    /// composition.
    #[test]
    fn what_rearranges_or_hides_text_goes_even_where_it_acts_on_its_neighbor() {
        for held in [
            "Acme\u{202E}",
            "Acme\u{200B}",
            "Acme\u{200C}",
            "Acme\u{FEFF}",
            "Acme\u{2060}",
            "Acme\u{180E}",
            "Acme\u{3164}",
            "Acme\u{FFA0}",
            "Acme\u{E0041}",
            "Acme\u{7}",
        ] {
            assert_eq!(Shown::of(held).as_str(), "Acme");
            assert_eq!(Shown::in_prose(held).as_str(), "Acme");
        }
    }

    /// The two questions part company at the carve-out and nowhere else: a
    /// character the strip keeps is one no Group name and no Alias may carry,
    /// and every character the carve-out names is one of the set's.
    #[test]
    fn a_name_is_still_refused_for_a_character_a_value_is_drawn_with() {
        for composing in ['\u{FE0F}', '\u{200D}', '\u{1160}', '\u{E0100}'] {
            assert_eq!(
                unshowable_character_in(&format!("dev{composing}")),
                Some(format!(
                    "a character a terminal does not draw as itself (U+{:04X})",
                    composing as u32
                ))
            );
        }
        for c in ('\0'..=char::MAX).filter(|c| composes_with_its_neighbor(*c)) {
            assert!(
                is_unshowable(c),
                "U+{:04X} is carved out of a set it is not in",
                c as u32
            );
        }
    }

    /// Following every link on a path, which is the question "is this inside
    /// that directory?" needs and [`through_any_link`] does not answer: the
    /// link that makes two spellings of one place is usually a directory
    /// *above* the one named.
    #[test]
    fn every_link_on_the_way_is_followed_and_not_only_the_last() {
        let host = FakeHost::new().with_link(
            Link::Symbolic,
            "/Users/someone/.config/perch/profiles",
            "/Users/someone/claude",
        );

        assert_eq!(
            settled(&host, Path::new("/Users/someone/claude/work")).at(),
            Some(Path::new("/Users/someone/.config/perch/profiles/work")),
            "the link is two components up, and the rest of the path goes back on"
        );
        assert_eq!(
            settled(&host, Path::new("/Users/someone/elsewhere")).at(),
            Some(Path::new("/Users/someone/elsewhere")),
            "a path with no link on it is itself"
        );
        // A path naming no place from the root is read from where Perch was
        // run, since `starts_with` matches components and a bare name shares
        // none with the directory it is sitting in.
        assert_eq!(
            settled(&host, Path::new("neither/is/this")).at(),
            Some(Path::new("/Users/someone/work/neither/is/this")),
        );
    }

    /// `max-time` is seconds, so a bound of 1,500ms is a machine that waits two
    /// whole ones. Read off the port by both adapters, a fixture answering in
    /// 1,800ms is a timeout on neither rather than on the fake alone.
    #[test]
    fn a_bound_is_the_whole_seconds_curl_would_actually_wait() {
        let bound = |millis| {
            HttpRequest::get("https://example.com", &[])
                .within(millis)
                .bound_millis()
        };

        assert_eq!(bound(1_500), 2_000, "rounded up, never down");
        assert_eq!(bound(2_000), 2_000, "and a whole second is itself");
        assert_eq!(
            bound(1),
            1_000,
            "a bound rounding to zero would be no bound"
        );
        assert_eq!(
            HttpRequest::get("https://example.com", &[]).bound_millis(),
            ORDINARY_BOUND_MILLIS,
            "a request that says nothing gets the ceiling every request gets"
        );
    }

    /// The pair goes onto one `curl` configuration line, so a newline in the
    /// *name* ends the `header` option exactly as one in the value does. Every
    /// name Perch sends today is a literal, so this holds the rule rather than
    /// catching a caller — and `HttpRequest` is public.
    #[test]
    fn a_header_name_may_no_more_carry_a_newline_than_its_value_may() {
        for (name, value) in [
            ("anthropic-beta\noutput", "oauth-2025-04-20"),
            ("anthropic-beta", "oauth\noutput"),
        ] {
            let refused = sendable(&HttpRequest {
                url: "https://api.anthropic.com/v1/me",
                headers: &[(name, value)],
                body: None,
                within_millis: None,
            })
            .expect_err("that would write a second option");

            assert!(
                refused.to_string().contains("anthropic-beta"),
                "it names the header: {refused}"
            );
        }
    }

    /// A link's target may be written relative to where the link sits, which is
    /// how `ln -s` records one unless it was given an absolute path.
    #[test]
    fn a_link_written_relative_to_itself_resolves_against_where_it_sits() {
        let host = FakeHost::new().with_link(Link::Symbolic, "elsewhere", "/Users/someone/here");

        assert_eq!(
            settled(&host, Path::new("/Users/someone/here/inside")).at(),
            Some(Path::new("/Users/someone/elsewhere/inside")),
        );
    }

    /// Two links pointing at each other are a loop, and the walk is bounded
    /// rather than trusting the filesystem not to contain one.
    #[test]
    fn a_loop_of_links_is_inside_whatever_it_is_asked_about() {
        let host = FakeHost::new()
            .with_link(Link::Symbolic, "/b", "/a")
            .with_link(Link::Symbolic, "/a", "/b");

        let looping = settled(&host, Path::new("/a"));

        assert_eq!(looping.at(), None, "the walk gives up rather than spinning");
        assert!(
            looping.is_inside(&settled(&host, Path::new("/somewhere/else"))),
            "and what it could not resolve is not ruled out of anywhere"
        );
    }

    /// The tags block is the standard carrier for invisible text: `U+E0020`
    /// upward mirror ASCII, draw as nothing, and are `Cf` rather than `Cc`, so
    /// `is_control` passes over every one. An address is read out of a file
    /// Perch does not own, and this is the rule that refuses one.
    #[test]
    fn a_character_that_draws_as_nothing_is_unshowable_however_it_is_spelled() {
        for hidden in [
            '\u{E0001}',
            '\u{E0020}',
            '\u{E0041}',
            '\u{E007F}',
            '\u{180E}',
        ] {
            assert!(
                is_unshowable(hidden),
                "U+{:04X} draws as nothing and hides the difference between two names",
                hidden as u32
            );
        }
        assert_eq!(
            Shown::of("some\u{E0041}one@example.com").to_string(),
            "someone@example.com",
            "so a listing draws the address without it"
        );
        for ordinary in ['a', '@', '.', 'é', '中'] {
            assert!(!is_unshowable(ordinary), "{ordinary} is drawn");
        }
    }

    /// A `..` with nothing ordinary in front of it to cancel, which is where the
    /// answer stops being "drop the component before". `/..` is the root, a
    /// leading one on a relative path is part of where it points, and a path
    /// that cancels itself out entirely is this directory rather than no path.
    #[test]
    fn a_path_doubling_back_past_its_own_start_keeps_what_it_cannot_cancel() {
        for (given, want) in [
            ("/a/b/../c", "/a/c"),
            ("/..", "/"),
            ("/../..", "/"),
            ("../a", "../a"),
            ("a/../..", ".."),
            ("a/b/..", "a"),
            ("a/..", "."),
            ("./a", "a"),
        ] {
            assert_eq!(flattened(Path::new(given)), PathBuf::from(want), "{given}");
        }
    }

    /// A `..` is not a name, so `Path::file_name` answers `None` on one and the
    /// walk stopped there — leaving links above it unfollowed and the rest to be
    /// compared as spelling. Both directions are wrong, and the Purge reaches
    /// both: it writes the Export at a path a person types at a prompt.
    #[test]
    fn a_path_that_doubles_back_is_resolved_before_it_is_placed() {
        let host = FakeHost::new().with_link(
            Link::Symbolic,
            "/Users/someone/backups",
            "/Users/someone/.config/perch",
        );
        let home = Path::new("/Users/someone/.config/perch");

        assert!(
            is_inside(
                &host,
                Path::new("/Users/someone/Documents/../.config/perch/backup.age"),
                home,
            ),
            "doubling back into the directory lands in it"
        );
        assert!(
            !is_inside(
                &host,
                Path::new("/Users/someone/.config/perch/profiles/../../../x.age"),
                home,
            ),
            "and doubling back out of it does not"
        );
        assert!(
            is_inside(
                &host,
                Path::new("/Users/someone/Documents/../backups/perch.age"),
                home,
            ),
            "a link past the `..` is followed rather than abandoned at it"
        );
    }

    /// Both sides, because either can be the linked one: a guard that resolved
    /// only the path it was handed is one a link on the *directory* walks past.
    #[test]
    fn a_directory_reached_through_a_link_still_holds_what_is_inside_it() {
        let host = FakeHost::new().with_link(
            Link::Symbolic,
            "/Users/someone/.config/perch",
            "/Users/someone/perch",
        );

        assert!(
            is_inside(
                &host,
                Path::new("/Users/someone/.config/perch/profiles/work"),
                Path::new("/Users/someone/perch"),
            ),
            "one spelling of the directory holds the other spelling of the path"
        );
        assert!(
            !is_inside(
                &host,
                Path::new("/Users/someone/backups/perch.age"),
                Path::new("/Users/someone/perch"),
            ),
            "and a path that is genuinely elsewhere is still elsewhere"
        );
    }

    use super::*;
    use crate::host::fake::Effect;

    const PATH: &str = "/Users/someone/.claude.json";

    /// The three shapes a Credential passes through on its way in and out of
    /// this port. Each one is asserted on the secret's absence rather than on
    /// the exact wording, so a `Debug` somebody reformats stays honest.
    #[test]
    fn nothing_that_carries_a_credential_prints_one() {
        const SECRET: &str = "sk-ant-oat01-a-live-refresh-token";

        let read_from_the_keychain = Execution {
            status: 0,
            stdout: SECRET.to_string(),
            stderr: String::new(),
        };
        let authorization = format!("Bearer {SECRET}");
        let headers = [("Authorization", authorization.as_str())];
        let renewal = HttpRequest::post(
            "https://console.anthropic.com/v1/oauth/token",
            &headers,
            SECRET,
        );
        let rotated = HttpResponse {
            status: 200,
            body: Zeroizing::new(format!("{{\"refresh_token\":\"{SECRET}\"}}")),
        };

        for printed in [
            format!("{read_from_the_keychain:?}"),
            format!("{renewal:?}"),
            format!("{rotated:?}"),
        ] {
            assert!(!printed.contains(SECRET), "printed a Credential: {printed}");
        }

        // The header *name* still prints, because knowing a request carried an
        // Authorization is the half of it that is worth having.
        assert!(format!("{renewal:?}").contains("Authorization"));
    }

    #[test]
    fn a_replacement_is_created_with_the_mode_the_file_already_had() {
        for mode in [0o600, 0o644, 0o640] {
            let host = FakeHost::new()
                .with_file(PATH, "{}")
                .with_file_mode(PATH, mode);

            write_atomically(&host, Path::new(PATH), "{\"a\":1}").expect("it is replaced");

            assert_eq!(host.file(PATH).as_deref(), Some("{\"a\":1}"));
            assert_eq!(host.mode_of(PATH), Some(mode));
        }
    }

    /// Nothing is being carried across here, so the file is created closed:
    /// the alternative is deciding to publish, at the umask, a file whose
    /// contents Perch cannot see the end of.
    #[test]
    fn a_file_that_was_not_there_is_created_for_its_owner_alone() {
        let host = FakeHost::new();

        write_atomically(&host, Path::new(PATH), "{}").expect("it is written");

        assert_eq!(host.mode_of(PATH), Some(PRIVATE_FILE_MODE));
    }

    #[test]
    fn a_file_that_is_a_link_is_written_through_rather_than_replaced() {
        let real = "/Users/someone/dotfiles/claude.json";
        let host = FakeHost::new()
            .with_file(real, "{}")
            .with_link(Link::Symbolic, real, PATH);

        write_atomically(&host, Path::new(PATH), "{\"a\":1}").expect("it is written");

        assert_eq!(
            host.file(real).as_deref(),
            Some("{\"a\":1}"),
            "the file the link names is the one that changed"
        );
        assert!(
            host.link_at(PATH).is_some(),
            "and the link is still a link, so what manages it goes on managing it"
        );
    }

    /// The caller asks `link_target` first, but that is a separate syscall from
    /// this one, so the guard belongs at both ends.
    #[test]
    fn removing_a_link_refuses_anything_that_is_not_one() {
        let host = FakeHost::new().with_file(PATH, "{\"mine\":true}");

        host.remove_link(Path::new(PATH))
            .expect_err("a file is not a link");

        assert_eq!(
            host.file(PATH).as_deref(),
            Some("{\"mine\":true}"),
            "and it is still there"
        );
    }

    /// The two ends of the same rule: a link goes, and a path with nothing at it
    /// is not a failure — a share that has already gone is a share that is gone.
    #[test]
    fn removing_a_link_takes_the_link_and_leaves_what_it_points_at() {
        let real = "/Users/someone/dotfiles/claude.json";
        let host = FakeHost::new()
            .with_file(real, "{}")
            .with_link(Link::Symbolic, real, PATH);

        host.remove_link(Path::new(PATH)).expect("a link goes");
        host.remove_link(Path::new(PATH))
            .expect("and one that has already gone is not a failure");

        assert!(host.link_at(PATH).is_none(), "the link is gone");
        assert_eq!(
            host.file(real).as_deref(),
            Some("{}"),
            "and what it pointed at is untouched"
        );
    }

    /// The fake performs the choreography rather than describing it, so a
    /// `RealHost::write_private_file` rewritten as a plain truncate-and-write
    /// fails here rather than passing.
    #[test]
    fn a_private_write_is_created_beside_the_file_and_moved_over_it() {
        let host = FakeHost::new().with_file(PATH, "{}");
        let beside = temp_beside(&host, Path::new(PATH));
        host.forget_effects();

        host.write_private_file(Path::new(PATH), "{\"a\":1}")
            .expect("it is written");

        assert!(
            host.effects().iter().any(|effect| matches!(
                effect,
                Effect::Renamed { from, to } if from == &beside && to == Path::new(PATH)
            )),
            "the file at the path is one that arrived whole: {:?}",
            host.effects()
        );
        assert_eq!(host.file(PATH).as_deref(), Some("{\"a\":1}"));
        assert_eq!(host.mode_of(PATH), Some(PRIVATE_FILE_MODE));
        assert_eq!(host.file(&beside), None, "and nothing is left beside it");
    }

    /// The write dying partway, rather than never starting: the fixture is a
    /// disk that fills between the open and the `sync_all`, which is the one
    /// failure that leaves bytes behind.
    #[test]
    fn a_write_that_dies_partway_leaves_nothing_beside_the_file_either() {
        let host = FakeHost::new()
            .with_file(PATH, "{}")
            .with_a_disk_that_fills_writing(PATH);
        let beside = temp_beside(&host, Path::new(PATH));

        let failed = host
            .write_private_file(Path::new(PATH), "{\"a\":1}")
            .expect_err("the disk fills partway through");

        assert!(failed.to_string().contains("No space left"), "{failed}");
        assert_eq!(
            host.file(&beside),
            None,
            "the bytes that fitted are cleared rather than left beside the file"
        );
        assert_eq!(
            host.file(PATH).as_deref(),
            Some("{}"),
            "and the original is exactly as it was"
        );
    }

    /// A relative target is resolved the way the operating system resolves it:
    /// against the directory the link sits in, not the working directory.
    #[test]
    fn a_link_pointing_somewhere_relative_is_followed_from_where_it_sits() {
        let real = "/Users/someone/dotfiles/claude.json";
        let host = FakeHost::new().with_file(real, "{}").with_link(
            Link::Symbolic,
            "dotfiles/claude.json",
            PATH,
        );

        write_atomically(&host, Path::new(PATH), "{\"a\":1}").expect("it is written");

        assert_eq!(host.file(real).as_deref(), Some("{\"a\":1}"));
    }

    /// The fallback still hands back *something* for a reply that is not UTF-8,
    /// because a caller that got no answer would read it as a store holding
    /// nothing.
    #[test]
    fn the_output_of_a_child_is_taken_over_where_it_is_already_text() {
        assert_eq!(
            taken_over(b"sk-ant-oat01-test".to_vec()),
            "sk-ant-oat01-test"
        );
        assert_eq!(taken_over(Vec::new()), "");
        // A lone continuation byte, which no UTF-8 string holds.
        assert_eq!(taken_over(vec![b'a', 0x80, b'b']), "a\u{fffd}b");
    }
}
