//! Port of `classes/comparator.js`.

use crate::functions::cmp_semver;
use crate::options::Options;
use crate::re::{self, RE};
use crate::semver::{compare_with, SemVer};
use crate::{Error, Result};

/// A single comparator such as `>=1.2.3`.
///
/// Upstream marks "matches anything" with a `Symbol('SemVer ANY')` stored in
/// `this.semver`; here that is `semver: None`.
#[derive(Debug, Clone)]
pub struct Comparator {
    pub operator: String,
    /// `None` is upstream's `Comparator.ANY`.
    pub semver: Option<SemVer>,
    pub value: String,
    pub options: Options,
    pub loose: bool,
}

impl Comparator {
    pub fn new(comp: &str, options: impl Into<Options>) -> Result<Self> {
        let options = options.into();
        let comp = crate::range::js_collapse_whitespace(crate::semver::js_trim(comp));

        let token = if options.loose { re::COMPARATORLOOSE } else { re::COMPARATOR };
        let caps = RE
            .safe(token)
            .captures(&comp)
            .ok_or_else(|| Error::new(format!("Invalid comparator: {comp}")))?;

        let mut operator = caps.get(1).map_or("", |m| m.as_str()).to_string();
        if operator == "=" {
            operator.clear();
        }

        // A bare `>` or `""` allows anything.
        let semver = match caps.get(2) {
            None => None,
            Some(m) if m.as_str().is_empty() => None,
            // Upstream passes `this.options.loose` — the bare boolean — so
            // includePrerelease is deliberately dropped for the inner version.
            Some(m) => Some(SemVer::new(m.as_str(), Options::from(options.loose))?),
        };

        let value = match &semver {
            None => String::new(),
            Some(sv) => format!("{}{}", operator, sv.version),
        };

        Ok(Comparator { operator, semver, value, options, loose: options.loose })
    }

    /// `true` when this comparator is upstream's `ANY`.
    pub fn is_any(&self) -> bool {
        self.semver.is_none()
    }

    /// Port of `test(version)` for an already-parsed version.
    ///
    /// Returns `Result` because upstream routes through
    /// `cmp(version, op, this.semver, this.options)`, and `cmp` reaches
    /// `new SemVer(x, options)` for *both* operands. When the supplied version
    /// carries different flags than this comparator, that constructor re-parses
    /// and can throw — see `SemVer::with_options`.
    pub fn test(&self, version: &SemVer) -> Result<bool> {
        let Some(ref sv) = self.semver else {
            return Ok(true);
        };
        let a = version.with_options(self.options)?;
        let b = sv.with_options(self.options)?;
        cmp_semver(&a, &self.operator, &b)
    }

    /// Port of `test(version)` for a string, where an unparseable version is
    /// `false` rather than an error.
    pub fn test_str(&self, version: &str) -> Result<bool> {
        if self.semver.is_none() {
            return Ok(true);
        }
        match SemVer::new(version, self.options) {
            Ok(v) => self.test(&v),
            Err(_) => Ok(false),
        }
    }

    /// Port of `intersects(comp, options)`.
    pub fn intersects(&self, comp: &Comparator, options: impl Into<Options>) -> Result<bool> {
        let options = options.into();

        if self.operator.is_empty() {
            if self.value.is_empty() {
                return Ok(true);
            }
            return crate::range::Range::new(&comp.value, options)?.test_str(&self.value);
        }
        if comp.operator.is_empty() {
            if comp.value.is_empty() {
                return Ok(true);
            }
            let r = crate::range::Range::new(&self.value, options)?;
            return match &comp.semver {
                Some(sv) => r.test(sv),
                None => Ok(true),
            };
        }

        // Nothing can possibly be lower than the null set.
        if options.include_prerelease
            && (self.value == "<0.0.0-0" || comp.value == "<0.0.0-0")
        {
            return Ok(false);
        }
        if !options.include_prerelease
            && (self.value.starts_with("<0.0.0") || comp.value.starts_with("<0.0.0"))
        {
            return Ok(false);
        }

        // Same direction increasing / decreasing.
        if self.operator.starts_with('>') && comp.operator.starts_with('>') {
            return Ok(true);
        }
        if self.operator.starts_with('<') && comp.operator.starts_with('<') {
            return Ok(true);
        }

        let (Some(a), Some(b)) = (&self.semver, &comp.semver) else {
            return Ok(false);
        };

        // Same version, both sides inclusive.
        if a.version == b.version
            && self.operator.contains('=')
            && comp.operator.contains('=')
        {
            return Ok(true);
        }
        // Opposite directions that still overlap. Upstream routes these through
        // `cmp(this.semver, op, comp.semver, options)`, which re-parses both
        // operands under `options` and can therefore throw.
        let ordering = compare_with(a, b, options)?;
        if ordering < 0 && self.operator.starts_with('>') && comp.operator.starts_with('<') {
            return Ok(true);
        }
        if ordering > 0 && self.operator.starts_with('<') && comp.operator.starts_with('>') {
            return Ok(true);
        }
        Ok(false)
    }
}

impl std::fmt::Display for Comparator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.value)
    }
}
