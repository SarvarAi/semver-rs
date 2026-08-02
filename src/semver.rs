//! Port of `classes/semver.js`.

use std::cmp::Ordering;

use crate::constants::{MAX_LENGTH, MAX_SAFE_INTEGER};
use crate::identifiers::{compare_identifiers, Identifier};
use crate::options::Options;
use crate::re::{self, RE};
use crate::util::{js_number_to_string, js_to_number};
use crate::{Error, Result};

/// A parsed semantic version.
///
/// `major`/`minor`/`patch` are `f64` rather than `u64` on purpose: upstream
/// stores them as JavaScript numbers and range-checks them against
/// `MAX_SAFE_INTEGER`, and an over-long numeric component coerces to `Infinity`
/// there rather than failing to parse. Keeping the same representation keeps the
/// same boundary behaviour. Valid values are always integers in
/// `0..=MAX_SAFE_INTEGER`, which `f64` represents exactly.
#[derive(Debug, Clone)]
pub struct SemVer {
    /// The input string as supplied, before trimming.
    pub raw: String,
    /// Canonical `major.minor.patch[-prerelease]`. Build metadata is excluded,
    /// exactly as upstream's `format()` does.
    pub version: String,
    pub major: f64,
    pub minor: f64,
    pub patch: f64,
    pub prerelease: Vec<Identifier>,
    pub build: Vec<String>,
    pub options: Options,
    pub loose: bool,
    pub include_prerelease: bool,
}

impl SemVer {
    /// Port of `new SemVer(version, options)`. Throws in upstream, `Err` here.
    pub fn new(version: &str, options: impl Into<Options>) -> Result<Self> {
        let options = options.into();

        if version.chars().count() > MAX_LENGTH {
            return Err(Error::new(format!(
                "version is longer than {MAX_LENGTH} characters"
            )));
        }

        let trimmed = js_trim(version);
        let token = if options.loose { re::LOOSE } else { re::FULL };
        let caps = RE
            .safe(token)
            .captures(trimmed)
            .ok_or_else(|| Error::new(format!("Invalid Version: {version}")))?;

        let major = js_to_number(caps.get(1).map_or("", |m| m.as_str()));
        let minor = js_to_number(caps.get(2).map_or("", |m| m.as_str()));
        let patch = js_to_number(caps.get(3).map_or("", |m| m.as_str()));

        if major > MAX_SAFE_INTEGER || major < 0.0 {
            return Err(Error::new("Invalid major version"));
        }
        if minor > MAX_SAFE_INTEGER || minor < 0.0 {
            return Err(Error::new("Invalid minor version"));
        }
        if patch > MAX_SAFE_INTEGER || patch < 0.0 {
            return Err(Error::new("Invalid patch version"));
        }

        // Numberify any prerelease numeric ids. Note the strict `<`: an
        // identifier equal to MAX_SAFE_INTEGER stays a *string*, and therefore
        // later takes the lossy string-comparison path (DECISIONS.md D7).
        let prerelease = match caps.get(4) {
            None => Vec::new(),
            Some(m) => m
                .as_str()
                .split('.')
                .map(|id| {
                    if id.bytes().all(|b| b.is_ascii_digit()) && !id.is_empty() {
                        let num = js_to_number(id);
                        if num >= 0.0 && num < MAX_SAFE_INTEGER {
                            return Identifier::Num(num);
                        }
                    }
                    Identifier::Str(id.to_string())
                })
                .collect(),
        };

        let build = caps
            .get(5)
            .map(|m| m.as_str().split('.').map(str::to_string).collect())
            .unwrap_or_default();

        let mut sv = SemVer {
            raw: version.to_string(),
            version: String::new(),
            major,
            minor,
            patch,
            prerelease,
            build,
            options,
            loose: options.loose,
            include_prerelease: options.include_prerelease,
        };
        sv.format();
        Ok(sv)
    }

    /// Port of `format()`. Recomputes and returns `version`.
    pub fn format(&mut self) -> &str {
        let mut v = format!(
            "{}.{}.{}",
            js_number_to_string(self.major),
            js_number_to_string(self.minor),
            js_number_to_string(self.patch)
        );
        if !self.prerelease.is_empty() {
            v.push('-');
            v.push_str(
                &self
                    .prerelease
                    .iter()
                    .map(|i| i.as_js_string())
                    .collect::<Vec<_>>()
                    .join("."),
            );
        }
        self.version = v;
        &self.version
    }

    /// Port of `compare(other)`.
    pub fn compare(&self, other: &SemVer) -> i32 {
        if other.version == self.version {
            return 0;
        }
        let main = self.compare_main(other);
        if main != 0 {
            return main;
        }
        self.compare_pre(other)
    }

    /// Port of `compareMain(other)` — major/minor/patch only.
    pub fn compare_main(&self, other: &SemVer) -> i32 {
        cmp_f64(self.major, other.major)
            .then_with(|| cmp_f64(self.minor, other.minor))
            .then_with(|| cmp_f64(self.patch, other.patch)) as i32
    }

    /// Port of `comparePre(other)`.
    ///
    /// Having no prerelease sorts *above* having one.
    pub fn compare_pre(&self, other: &SemVer) -> i32 {
        if !self.prerelease.is_empty() && other.prerelease.is_empty() {
            return -1;
        }
        if self.prerelease.is_empty() && !other.prerelease.is_empty() {
            return 1;
        }
        if self.prerelease.is_empty() && other.prerelease.is_empty() {
            return 0;
        }
        compare_identifier_lists(&self.prerelease, &other.prerelease)
    }

    /// Port of `compareBuild(other)`.
    pub fn compare_build(&self, other: &SemVer) -> i32 {
        let a: Vec<Identifier> =
            self.build.iter().map(|s| Identifier::Str(s.clone())).collect();
        let b: Vec<Identifier> =
            other.build.iter().map(|s| Identifier::Str(s.clone())).collect();
        compare_identifier_lists(&a, &b)
    }

    /// Port of `inc(release, identifier, identifierBase)`.
    ///
    /// `premajor`/`preminor`/`prepatch`/`prerelease` bump the version and then
    /// recurse into the internal `pre` release, exactly as upstream does.
    pub fn inc(
        &mut self,
        release: &str,
        identifier: Option<&str>,
        identifier_base: &IdentifierBase,
    ) -> Result<&mut Self> {
        if release.starts_with("pre") {
            if identifier.map_or(true, str::is_empty) && *identifier_base == IdentifierBase::False {
                return Err(Error::new("invalid increment argument: identifier is empty"));
            }
            if let Some(id) = identifier.filter(|s| !s.is_empty()) {
                // Upstream validates by matching `-{identifier}` against the
                // prerelease token and requiring group 1 to round-trip.
                let token = if self.options.loose { re::PRERELEASELOOSE } else { re::PRERELEASE };
                let probe = format!("-{id}");
                let ok = RE
                    .safe(token)
                    .captures(&probe)
                    .and_then(|c| c.get(1).map(|m| m.as_str() == id))
                    .unwrap_or(false);
                if !ok {
                    return Err(Error::new(format!("invalid identifier: {id}")));
                }
            }
        }

        match release {
            "premajor" => {
                self.prerelease.clear();
                self.patch = 0.0;
                self.minor = 0.0;
                self.major += 1.0;
                self.inc("pre", identifier, identifier_base)?;
            }
            "preminor" => {
                self.prerelease.clear();
                self.patch = 0.0;
                self.minor += 1.0;
                self.inc("pre", identifier, identifier_base)?;
            }
            "prepatch" => {
                // Drop any existing prerelease first; it is not relevant here.
                self.prerelease.clear();
                self.inc("patch", identifier, identifier_base)?;
                self.inc("pre", identifier, identifier_base)?;
            }
            "prerelease" => {
                if self.prerelease.is_empty() {
                    self.inc("patch", identifier, identifier_base)?;
                }
                self.inc("pre", identifier, identifier_base)?;
            }
            "release" => {
                if self.prerelease.is_empty() {
                    return Err(Error::new(format!("version {} is not a prerelease", self.raw)));
                }
                self.prerelease.clear();
            }
            "major" => {
                // 1.0.0-5 bumps to 1.0.0; 1.1.0 bumps to 2.0.0.
                if self.minor != 0.0 || self.patch != 0.0 || self.prerelease.is_empty() {
                    self.major += 1.0;
                }
                self.minor = 0.0;
                self.patch = 0.0;
                self.prerelease.clear();
            }
            "minor" => {
                if self.patch != 0.0 || self.prerelease.is_empty() {
                    self.minor += 1.0;
                }
                self.patch = 0.0;
                self.prerelease.clear();
            }
            "patch" => {
                if self.prerelease.is_empty() {
                    self.patch += 1.0;
                }
                self.prerelease.clear();
            }
            "pre" => {
                let base = if identifier_base.to_js_number() != 0.0
                    && !identifier_base.to_js_number().is_nan()
                {
                    1.0
                } else {
                    0.0
                };

                if self.prerelease.is_empty() {
                    self.prerelease = vec![Identifier::Num(base)];
                } else {
                    // Upstream scans right-to-left for the first numeric part
                    // and bumps it; `i === -1` afterwards means none was found.
                    let mut found = false;
                    for i in (0..self.prerelease.len()).rev() {
                        if let Identifier::Num(n) = &mut self.prerelease[i] {
                            *n += 1.0;
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        let joined = self
                            .prerelease
                            .iter()
                            .map(|i| i.as_js_string())
                            .collect::<Vec<_>>()
                            .join(".");
                        if identifier == Some(joined.as_str())
                            && *identifier_base == IdentifierBase::False
                        {
                            return Err(Error::new(
                                "invalid increment argument: identifier already exists",
                            ));
                        }
                        self.prerelease.push(Identifier::Num(base));
                    }
                }

                if let Some(id) = identifier.filter(|s| !s.is_empty()) {
                    // 1.2.0-beta.1 bumps to 1.2.0-beta.2;
                    // 1.2.0-beta.fooblz or 1.2.0-beta bumps to 1.2.0-beta.0.
                    let replacement = if *identifier_base == IdentifierBase::False {
                        vec![Identifier::Str(id.to_string())]
                    } else {
                        vec![Identifier::Str(id.to_string()), Identifier::Num(base)]
                    };

                    if is_prerelease_identifier(&self.prerelease, id) {
                        let k = id.split('.').count();
                        if js_is_nan(self.prerelease.get(k)) {
                            self.prerelease = replacement;
                        }
                    } else {
                        self.prerelease = replacement;
                    }
                }
            }
            other => {
                return Err(Error::new(format!("invalid increment argument: {other}")));
            }
        }

        self.format();
        self.raw = self.version.clone();
        if !self.build.is_empty() {
            self.raw.push('+');
            self.raw.push_str(&self.build.join("."));
        }
        Ok(self)
    }
}

/// The `identifierBase` argument to `inc`, which is tri-state in upstream:
/// absent, the literal `false`, or a value that gets `Number()`-coerced.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum IdentifierBase {
    /// Not supplied. `Number(undefined)` is `NaN`, so the base is 0.
    #[default]
    Undefined,
    /// Literal `false`. Suppresses the trailing numeric identifier entirely.
    False,
    Value(String),
}

impl IdentifierBase {
    fn to_js_number(&self) -> f64 {
        match self {
            IdentifierBase::Undefined => f64::NAN,
            IdentifierBase::False => 0.0,
            IdentifierBase::Value(s) => {
                if s.is_empty() {
                    0.0
                } else {
                    js_to_number(s)
                }
            }
        }
    }
}

/// Port of the module-private `isPrereleaseIdentifier`.
fn is_prerelease_identifier(prerelease: &[Identifier], identifier: &str) -> bool {
    let parts: Vec<&str> = identifier.split('.').collect();
    if parts.len() > prerelease.len() {
        return false;
    }
    for (i, part) in parts.iter().enumerate() {
        if compare_identifiers(&prerelease[i], &Identifier::Str((*part).to_string())) != 0 {
            return false;
        }
    }
    true
}

/// JavaScript `isNaN(x)`, i.e. `Number(x)` is NaN. `undefined` yields `true`;
/// a numeric string such as `"5"` yields `false`.
fn js_is_nan(v: Option<&Identifier>) -> bool {
    match v {
        None => true,
        Some(Identifier::Num(n)) => n.is_nan(),
        Some(Identifier::Str(s)) => {
            if s.is_empty() {
                false
            } else {
                js_to_number(s).is_nan()
            }
        }
    }
}

impl std::fmt::Display for SemVer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.version)
    }
}

impl PartialEq for SemVer {
    fn eq(&self, other: &Self) -> bool {
        self.compare(other) == 0
    }
}
impl Eq for SemVer {}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.compare(other) {
            i32::MIN..=-1 => Ordering::Less,
            0 => Ordering::Equal,
            1..=i32::MAX => Ordering::Greater,
        }
    }
}

/// Walks two identifier lists the way upstream's `do { } while (++i)` loops do:
/// running off the end of one list makes it the lesser value.
fn compare_identifier_lists(a: &[Identifier], b: &[Identifier]) -> i32 {
    let mut i = 0usize;
    loop {
        let x = a.get(i);
        let y = b.get(i);
        match (x, y) {
            (None, None) => return 0,
            (Some(_), None) => return 1,
            (None, Some(_)) => return -1,
            (Some(x), Some(y)) => {
                if x == y {
                    i += 1;
                    continue;
                }
                return compare_identifiers(x, y);
            }
        }
    }
}

fn cmp_f64(a: f64, b: f64) -> Ordering {
    if a < b {
        Ordering::Less
    } else if a > b {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

/// JavaScript's `String.prototype.trim`, whose whitespace set is the same one
/// `\s` matches — notably including U+FEFF, which Rust's `str::trim` does not.
pub fn js_trim(s: &str) -> &str {
    let is_ws = |c: char| {
        matches!(c,
            '\u{9}' | '\u{a}' | '\u{b}' | '\u{c}' | '\u{d}' | '\u{20}'
            | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}'
            | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
            | '\u{feff}')
    };
    s.trim_matches(is_ws)
}
