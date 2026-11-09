//! Test helper: the shaders embedded in Wine's d3d9 conformance tests.
//!
//! Wine's tests (`dlls/d3d9/tests/*.c`) carry hundreds of shaders as C arrays
//! of bytecode tokens. They make a free, realistic corpus for testing the
//! shader pipeline.
//!
//! The Wine sources are research material and are not committed. Tests call
//! [`load_or_skip`], which reads the environment ([`Policy`]) to decide
//! whether a missing corpus fails the test or skips it:
//!
//! - `CF_WINE_D3D9_TESTS=<dir>` reads the corpus from `<dir>`, a Wine
//!   `dlls/d3d9/tests` directory. A directory named this way must be usable.
//! - `CF_REQUIRE_WINE_CORPUS=1` makes a missing corpus a failure, and
//!   `CF_REQUIRE_WINE_CORPUS=0` makes it a skip.
//! - Without `CF_REQUIRE_WINE_CORPUS`, a missing corpus fails when `CI` is
//!   set and is skipped, with a notice on stderr, otherwise.

use std::ffi::OsString;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Default location, relative to the workspace root.
const DEFAULT_DIR: &str = "research/sources/crossover-26.3.0/wine/dlls/d3d9/tests";

/// Names another corpus directory.
const DIR_VAR: &str = "CF_WINE_D3D9_TESTS";
/// Forces the corpus to be required (`1`) or optional (`0`).
const REQUIRE_VAR: &str = "CF_REQUIRE_WINE_CORPUS";
/// Set by CI services; makes the corpus required unless `REQUIRE_VAR` says otherwise.
const CI_VAR: &str = "CI";

/// Where the Wine sources come from.
const SOURCES_URL: &str = "https://media.codeweavers.com/pub/crossover/source/crossover-sources-26.3.0.tar.gz";

const END_TOKEN: u32 = 0x0000_ffff;

/// One shader from the corpus.
#[derive(Clone, Debug)]
pub struct CorpusShader {
    /// Source file name, e.g. `visual.c`.
    pub file: String,
    /// Array name, e.g. `ps_code`.
    pub name: String,
    /// Bytecode tokens.
    pub tokens: Vec<u32>,
}

impl CorpusShader {
    /// `file:name`, for messages.
    pub fn label(&self) -> String {
        format!("{}:{}", self.file, self.name)
    }
}

/// Everything extracted from the corpus directory.
#[derive(Clone, Debug)]
pub struct Corpus {
    /// Directory the shaders were read from.
    pub dir: PathBuf,
    /// Complete shaders, in file and source order.
    pub shaders: Vec<CorpusShader>,
    /// Arrays skipped because an element is not a plain integer literal.
    pub skipped_arrays: usize,
    /// `.c` files that could not be read, with the reason. Their shaders are
    /// missing from [`Corpus::shaders`].
    pub unreadable_files: Vec<(PathBuf, String)>,
}

/// Why a corpus directory could not be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// The directory could not be listed (usually: it does not exist).
    Unreadable {
        /// The directory.
        dir: PathBuf,
        /// The I/O error.
        error: String,
    },
    /// The directory has no readable `.c` files.
    NoSources {
        /// The directory.
        dir: PathBuf,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Unreadable { dir, error } => {
                write!(f, "cannot read {} ({error})", dir.display())
            }
            LoadError::NoSources { dir } => {
                write!(f, "{} has no readable .c files", dir.display())
            }
        }
    }
}

impl std::error::Error for LoadError {}

/// Where to find the corpus and whether a test run must have it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    /// Directory to read.
    pub dir: PathBuf,
    /// The directory was named with `CF_WINE_D3D9_TESTS`, so it must be usable.
    pub explicit: bool,
    /// The environment variable that makes the corpus required, if any.
    pub required_by: Option<&'static str>,
}

/// What a test should do with the result of loading the corpus.
#[derive(Debug)]
pub enum Decision {
    /// Run the test on this corpus.
    Use(Corpus),
    /// Skip the test, for this reason.
    Skip(String),
    /// Fail the test, with this message.
    Fail(String),
}

impl Policy {
    /// The policy the process environment asks for.
    pub fn from_env() -> Self {
        Self::from_vars(|name| std::env::var_os(name))
    }

    /// The policy for the environment variables `var` returns.
    pub fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let get = |name: &str| var(name).filter(|value| !value.is_empty());
        let explicit_dir = get(DIR_VAR).map(PathBuf::from);
        let required_by = match get(REQUIRE_VAR) {
            Some(value) => is_true(&value).then_some(REQUIRE_VAR),
            None => get(CI_VAR).is_some_and(|value| is_true(&value)).then_some(CI_VAR),
        };
        Self {
            explicit: explicit_dir.is_some(),
            dir: explicit_dir.unwrap_or_else(|| workspace_root().join(DEFAULT_DIR)),
            required_by,
        }
    }

    /// Decides what to do with the outcome of loading [`Policy::dir`].
    pub fn decide(&self, loaded: Result<Corpus, LoadError>) -> Decision {
        let error = match loaded {
            Ok(corpus) => return Decision::Use(corpus),
            Err(error) => error,
        };
        if self.explicit {
            Decision::Fail(format!("Wine d3d9 corpus: {DIR_VAR} is set, but {error}.\n{}", how_to_get_it()))
        } else if let Some(var) = self.required_by {
            Decision::Fail(format!(
                "Wine d3d9 corpus: required because {var} is set, but {error}.\n{}\n\
                 Set {REQUIRE_VAR}=0 to skip the corpus tests instead.",
                how_to_get_it()
            ))
        } else {
            Decision::Skip(format!(
                "SKIPPED Wine d3d9 corpus test: {error}.\n{}\n\
                 Set {REQUIRE_VAR}=1 to make a missing corpus fail (it fails on CI).",
                how_to_get_it()
            ))
        }
    }
}

fn is_true(value: &OsString) -> bool {
    let value = value.to_string_lossy().to_ascii_lowercase();
    !matches!(value.trim(), "0" | "false" | "no" | "off")
}

fn how_to_get_it() -> String {
    format!(
        "To get it, download {SOURCES_URL} and extract `sources/wine` to \
         <workspace>/research/sources/crossover-26.3.0/wine \
         (`tar xzf crossover-sources-26.3.0.tar.gz -C research/sources/crossover-26.3.0 \
         --strip-components=1 sources/wine`), or set {DIR_VAR} to a Wine dlls/d3d9/tests directory."
    )
}

/// Loads the corpus for a test, following [`Policy::from_env`].
///
/// Returns `None` when the test should be skipped; the reason is written to
/// stderr past the test harness's output capture, so it shows up in a normal
/// `cargo test` run. Panics, failing the test, when the corpus is required or
/// `CF_WINE_D3D9_TESTS` names an unusable directory. Files that could not be
/// read are reported as warnings.
pub fn load_or_skip() -> Option<Corpus> {
    let policy = Policy::from_env();
    match policy.decide(load_from(&policy.dir)) {
        Decision::Use(corpus) => {
            for (path, error) in &corpus.unreadable_files {
                notice(&format!("WARNING Wine d3d9 corpus: skipped unreadable {} ({error})", path.display()));
            }
            Some(corpus)
        }
        Decision::Skip(reason) => {
            notice(&reason);
            None
        }
        Decision::Fail(message) => panic!("{message}"),
    }
}

/// Writes to stderr directly: `eprintln!` output of a passing test is captured
/// and never shown.
fn notice(message: &str) {
    let _ = writeln!(std::io::stderr().lock(), "{message}");
}

/// Loads the corpus from `dir`.
///
/// A `.c` file that cannot be read is listed in
/// [`Corpus::unreadable_files`] instead of failing the whole load; files that
/// are not valid UTF-8 are read lossily.
pub fn load_from(dir: &Path) -> Result<Corpus, LoadError> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| LoadError::Unreadable { dir: dir.to_path_buf(), error: e.to_string() })?;
    let mut paths: Vec<PathBuf> =
        entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "c")).collect();
    paths.sort();

    let mut shaders = Vec::new();
    let mut skipped_arrays = 0;
    let mut unreadable_files = Vec::new();
    let mut files_read = 0;
    for path in paths {
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                unreadable_files.push((path, e.to_string()));
                continue;
            }
        };
        files_read += 1;
        let source = String::from_utf8_lossy(&bytes);
        let file = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let (arrays, skipped) = arrays_in(&file, &source);
        skipped_arrays += skipped;
        // A complete shader starts with a version token (0xfffe/0xffff in the
        // high half) and contains the end token. This leaves out vertex data
        // and the header fragments some tests splice together at run time.
        shaders.extend(
            arrays
                .into_iter()
                .filter(|a| matches!(a.tokens[0] >> 16, 0xfffe | 0xffff) && a.tokens.contains(&END_TOKEN)),
        );
    }
    if files_read == 0 {
        return Err(LoadError::NoSources { dir: dir.to_path_buf() });
    }
    Ok(Corpus { dir: dir.to_path_buf(), shaders, skipped_arrays, unreadable_files })
}

fn workspace_root() -> PathBuf {
    // This crate lives in <root>/crates/wine-test-corpus.
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.ancestors().nth(2).map_or_else(|| manifest.join("../.."), Path::to_path_buf)
}

fn strip_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("/*") {
            rest = after.find("*/").map_or("", |end| &after[end + 2..]);
            out.push(' ');
        } else if let Some(after) = rest.strip_prefix("//") {
            rest = after.find('\n').map_or("", |end| &after[end..]);
        } else {
            let ch = rest.chars().next().unwrap_or(' ');
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    out
}

fn parse_literal(text: &str) -> Option<u32> {
    let text = text.trim().trim_end_matches(['u', 'U', 'l', 'L']);
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        text.parse().ok()
    }
}

/// Extracts `DWORD name[] = { ... };` arrays whose elements are all plain
/// integer literals. Returns the arrays and how many others were skipped.
pub fn arrays_in(file: &str, source: &str) -> (Vec<CorpusShader>, usize) {
    let code = strip_comments(source);
    let mut arrays = Vec::new();
    let mut skipped = 0;
    let mut rest = code.as_str();
    while let Some(at) = rest.find("DWORD ") {
        rest = &rest[at + "DWORD ".len()..];
        let header_end = match rest.find(['{', ';']) {
            Some(i) if rest.as_bytes()[i] == b'{' => i,
            Some(i) => {
                rest = &rest[i..];
                continue;
            }
            None => break,
        };
        let header = &rest[..header_end];
        if !header.contains("[]") || !header.trim_end().ends_with('=') {
            continue;
        }
        let name = header.split("[]").next().unwrap_or_default().trim();
        let body_start = header_end + 1;
        let Some(body_len) = rest[body_start..].find('}') else {
            break;
        };
        let body = &rest[body_start..body_start + body_len];
        rest = &rest[body_start + body_len..];
        let literals: Option<Vec<u32>> =
            body.split(',').map(str::trim).filter(|s| !s.is_empty()).map(parse_literal).collect();
        match literals {
            Some(tokens) if !tokens.is_empty() => {
                arrays.push(CorpusShader { file: file.to_owned(), name: name.to_owned(), tokens })
            }
            _ => skipped += 1,
        }
    }
    (arrays, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_literal_arrays_from_c_source() {
        let source = "static const DWORD vs[] =\n{\n    0xfffe0101, /* vs_1_1 */\n    0x0000ffff, // end\n};\n\
                      DWORD count;\nstatic const DWORD weird[] = { FOO, 1 };\nconst DWORD dec[] = {1, 2u};";
        let (arrays, skipped) = arrays_in("x.c", source);
        assert_eq!(skipped, 1);
        let names: Vec<_> = arrays.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["vs", "dec"]);
        assert_eq!(arrays[0].tokens, [0xfffe0101, 0x0000ffff]);
        assert_eq!(arrays[1].tokens, [1, 2]);
        assert_eq!(arrays[0].label(), "x.c:vs");
    }

    /// A fresh, empty directory under the system temp directory.
    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wine-test-corpus-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn vars(pairs: &[(&str, &str)]) -> Policy {
        let pairs: Vec<(String, OsString)> = pairs.iter().map(|(k, v)| ((*k).to_owned(), OsString::from(v))).collect();
        Policy::from_vars(|name| pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()))
    }

    fn missing() -> Result<Corpus, LoadError> {
        Err(LoadError::Unreadable { dir: PathBuf::from("/nonexistent"), error: "No such file or directory".into() })
    }

    fn some_corpus() -> Result<Corpus, LoadError> {
        Ok(Corpus {
            dir: PathBuf::from("/corpus"),
            shaders: Vec::new(),
            skipped_arrays: 0,
            unreadable_files: Vec::new(),
        })
    }

    #[test]
    fn missing_directory_is_an_error() {
        let e = load_from(Path::new("/nonexistent/crossfree/corpus")).unwrap_err();
        assert!(matches!(e, LoadError::Unreadable { .. }), "{e:?}");
    }

    #[test]
    fn directory_without_sources_is_an_error() {
        let dir = scratch_dir("empty");
        std::fs::write(dir.join("notes.txt"), "DWORD x[] = {0xfffe0101, 0x0000ffff};").unwrap();
        assert_eq!(load_from(&dir).unwrap_err(), LoadError::NoSources { dir: dir.clone() });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_files_are_skipped_individually() {
        let dir = scratch_dir("bad-files");
        // Not UTF-8: read lossily, the shader is still found.
        let mut latin1 = b"/* caf\xe9 */ static const DWORD vs[] = {0xfffe0101, 0x0000ffff};".to_vec();
        latin1.push(b'\n');
        std::fs::write(dir.join("a.c"), latin1).unwrap();
        // Unreadable as a file: listed, and the rest of the corpus still loads.
        std::fs::create_dir(dir.join("b.c")).unwrap();
        let corpus = load_from(&dir).unwrap();
        let names: Vec<_> = corpus.shaders.iter().map(CorpusShader::label).collect();
        assert_eq!(names, ["a.c:vs"]);
        assert_eq!(corpus.unreadable_files.len(), 1);
        assert_eq!(corpus.unreadable_files[0].0, dir.join("b.c"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_variables_mean_default_directory_and_optional_corpus() {
        let policy = vars(&[]);
        assert!(policy.dir.ends_with(DEFAULT_DIR), "{}", policy.dir.display());
        assert!(!policy.dir.to_string_lossy().contains(".."), "{}", policy.dir.display());
        assert!(!policy.explicit);
        assert_eq!(policy.required_by, None);
        // Empty values count as unset.
        assert_eq!(vars(&[(CI_VAR, ""), (DIR_VAR, "")]), policy);
    }

    #[test]
    fn ci_requires_the_corpus() {
        assert_eq!(vars(&[(CI_VAR, "true")]).required_by, Some(CI_VAR));
        assert_eq!(vars(&[(CI_VAR, "1")]).required_by, Some(CI_VAR));
        assert_eq!(vars(&[(CI_VAR, "false")]).required_by, None);
        assert_eq!(vars(&[(CI_VAR, "0")]).required_by, None);
    }

    #[test]
    fn require_variable_overrides_ci() {
        assert_eq!(vars(&[(REQUIRE_VAR, "1")]).required_by, Some(REQUIRE_VAR));
        assert_eq!(vars(&[(REQUIRE_VAR, "0"), (CI_VAR, "true")]).required_by, None);
        assert_eq!(vars(&[(REQUIRE_VAR, "1"), (CI_VAR, "false")]).required_by, Some(REQUIRE_VAR));
    }

    #[test]
    fn explicit_directory_is_used() {
        let policy = vars(&[(DIR_VAR, "/somewhere/tests")]);
        assert_eq!(policy.dir, PathBuf::from("/somewhere/tests"));
        assert!(policy.explicit);
        assert_eq!(policy.required_by, None);
    }

    #[test]
    fn missing_corpus_is_skipped_locally() {
        match vars(&[]).decide(missing()) {
            Decision::Skip(reason) => {
                assert!(reason.contains("/nonexistent"), "{reason}");
                assert!(reason.contains(SOURCES_URL), "{reason}");
            }
            other => panic!("expected a skip, got {other:?}"),
        }
    }

    #[test]
    fn missing_corpus_fails_when_required() {
        for policy in [vars(&[(CI_VAR, "true")]), vars(&[(REQUIRE_VAR, "1")])] {
            match policy.decide(missing()) {
                Decision::Fail(message) => {
                    assert!(message.contains(policy.required_by.unwrap()), "{message}")
                }
                other => panic!("expected a failure, got {other:?}"),
            }
        }
    }

    #[test]
    fn unusable_explicit_directory_fails_even_when_optional() {
        let policy = vars(&[(DIR_VAR, "/nonexistent"), (REQUIRE_VAR, "0")]);
        let loaded = load_from(&policy.dir);
        match policy.decide(loaded) {
            Decision::Fail(message) => assert!(message.contains(DIR_VAR), "{message}"),
            other => panic!("expected a failure, got {other:?}"),
        }
        let empty = Err(LoadError::NoSources { dir: PathBuf::from("/empty") });
        assert!(matches!(policy.decide(empty), Decision::Fail(_)));
    }

    #[test]
    fn a_loaded_corpus_is_used_whatever_the_policy() {
        for policy in [vars(&[]), vars(&[(CI_VAR, "true")]), vars(&[(DIR_VAR, "/corpus")])] {
            assert!(matches!(policy.decide(some_corpus()), Decision::Use(_)));
        }
    }
}
