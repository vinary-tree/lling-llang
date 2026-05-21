//! API migration transducers for automated version migration.
//!
//! This module provides WFST-based transducers for automating API version
//! migrations. Given a set of migration rules, the transducer can transform
//! code from one API version to another.
//!
//! # Architecture
//!
//! The API migration system consists of:
//!
//! 1. **Migration Rules**: Define transformations from old API patterns to new ones
//! 2. **Version Ranges**: Specify which API versions a rule applies to
//! 3. **Migration Transducer**: WFST that applies rules to transform code
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::programming::*;
//!
//! // Define migration rules
//! let migration = ApiMigrationBuilder::new()
//!     .add_rule(ApiMigrationRule::new(
//!         "old_function",
//!         "new_function",
//!         (Version::new(1, 0), Version::new(2, 0)),
//!     ))
//!     .add_rule(ApiMigrationRule::rename_parameter(
//!         "deprecated_param",
//!         "new_param",
//!         (Version::new(1, 0), Version::new(2, 0)),
//!     ))
//!     .build();
//!
//! // Apply migration
//! let migrated = migration.migrate("old_function(deprecated_param)");
//! ```
//!
//! # Use Cases
//!
//! - **Library Version Upgrades**: Migrate user code to new library versions
//! - **Language Version Upgrades**: Update code for new language features
//! - **Deprecation Handling**: Replace deprecated APIs with modern equivalents
//! - **Framework Migrations**: Transform code between framework versions

use std::collections::HashMap;
use std::fmt::{self, Display};

use crate::semiring::Semiring;
#[cfg(test)]
use crate::wfst::Wfst;
use crate::wfst::{MutableWfst, VectorWfst, WeightedTransition};

/// A semantic version number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// Major version (breaking changes).
    pub major: u32,
    /// Minor version (new features).
    pub minor: u32,
    /// Patch version (bug fixes).
    pub patch: u32,
}

impl Version {
    /// Create a new version.
    pub const fn new(major: u32, minor: u32) -> Self {
        Self {
            major,
            minor,
            patch: 0,
        }
    }

    /// Create a version with patch number.
    pub const fn with_patch(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parse a version string (e.g., "1.2.3").
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<_> = s.split('.').collect();
        if parts.is_empty() || parts.len() > 3 {
            return None;
        }

        let major = parts[0].parse().ok()?;
        let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let patch = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);

        Some(Self {
            major,
            minor,
            patch,
        })
    }

    /// Check if this version satisfies a range (inclusive on both ends).
    pub fn satisfies(&self, from: Version, to: Version) -> bool {
        *self >= from && *self <= to
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A version range for migration rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionRange {
    /// Starting version (inclusive).
    pub from: Version,
    /// Ending version (inclusive).
    pub to: Version,
}

impl VersionRange {
    /// Create a new version range.
    pub const fn new(from: Version, to: Version) -> Self {
        Self { from, to }
    }

    /// Check if a version falls within this range.
    pub fn contains(&self, version: Version) -> bool {
        version >= self.from && version <= self.to
    }
}

impl Display for VersionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} - {}", self.from, self.to)
    }
}

/// Type of API migration.
#[derive(Debug, Clone, PartialEq)]
pub enum MigrationType {
    /// Rename a function/method.
    RenameFunction {
        /// Old function name.
        old_name: String,
        /// New function name.
        new_name: String,
    },
    /// Rename a parameter.
    RenameParameter {
        /// Function the parameter belongs to (None for all).
        function: Option<String>,
        /// Old parameter name.
        old_name: String,
        /// New parameter name.
        new_name: String,
    },
    /// Rename a type.
    RenameType {
        /// Old type name.
        old_name: String,
        /// New type name.
        new_name: String,
    },
    /// Change function signature.
    ChangeSignature {
        /// Function name.
        function: String,
        /// Old parameter types/names.
        old_params: Vec<String>,
        /// New parameter types/names.
        new_params: Vec<String>,
    },
    /// Replace one API call with another.
    ReplaceCall {
        /// Old API call pattern (token sequence).
        old_pattern: Vec<String>,
        /// New API call pattern (token sequence).
        new_pattern: Vec<String>,
    },
    /// Remove a deprecated function.
    RemoveFunction {
        /// Function to remove.
        function: String,
        /// Replacement message.
        message: String,
    },
    /// Add required parameter.
    AddParameter {
        /// Function to modify.
        function: String,
        /// Parameter to add.
        param_name: String,
        /// Default value for the parameter.
        default_value: String,
    },
    /// Remove parameter.
    RemoveParameter {
        /// Function to modify.
        function: String,
        /// Parameter to remove.
        param_name: String,
    },
    /// Custom transformation with arbitrary token mapping.
    Custom {
        /// Description of the transformation.
        description: String,
        /// Old token pattern.
        old_tokens: Vec<String>,
        /// New token pattern.
        new_tokens: Vec<String>,
    },
}

/// A single API migration rule.
#[derive(Debug, Clone)]
pub struct ApiMigrationRule {
    /// Unique identifier for this rule.
    pub id: String,
    /// Human-readable description.
    pub description: String,
    /// Type of migration.
    pub migration_type: MigrationType,
    /// Version range this rule applies to.
    pub version_range: VersionRange,
    /// Cost/weight of applying this rule (lower = preferred).
    pub cost: f64,
    /// Whether this is an automatic migration (vs. requires review).
    pub automatic: bool,
}

impl ApiMigrationRule {
    /// Create a function rename rule.
    pub fn rename_function(
        old_name: impl Into<String>,
        new_name: impl Into<String>,
        from: Version,
        to: Version,
    ) -> Self {
        let old_name = old_name.into();
        let new_name = new_name.into();
        Self {
            id: format!("rename_fn_{}_{}", &old_name, &new_name),
            description: format!("Rename function {} to {}", &old_name, &new_name),
            migration_type: MigrationType::RenameFunction { old_name, new_name },
            version_range: VersionRange::new(from, to),
            cost: 0.1,
            automatic: true,
        }
    }

    /// Create a type rename rule.
    pub fn rename_type(
        old_name: impl Into<String>,
        new_name: impl Into<String>,
        from: Version,
        to: Version,
    ) -> Self {
        let old_name = old_name.into();
        let new_name = new_name.into();
        Self {
            id: format!("rename_type_{}_{}", &old_name, &new_name),
            description: format!("Rename type {} to {}", &old_name, &new_name),
            migration_type: MigrationType::RenameType { old_name, new_name },
            version_range: VersionRange::new(from, to),
            cost: 0.1,
            automatic: true,
        }
    }

    /// Create a parameter rename rule.
    pub fn rename_parameter(
        function: Option<impl Into<String>>,
        old_name: impl Into<String>,
        new_name: impl Into<String>,
        from: Version,
        to: Version,
    ) -> Self {
        let function = function.map(Into::into);
        let old_name = old_name.into();
        let new_name = new_name.into();
        let fn_part = function.as_deref().unwrap_or("*");
        Self {
            id: format!("rename_param_{}_{}_{}", fn_part, &old_name, &new_name),
            description: format!(
                "Rename parameter {} to {} in {}",
                &old_name, &new_name, fn_part
            ),
            migration_type: MigrationType::RenameParameter {
                function,
                old_name,
                new_name,
            },
            version_range: VersionRange::new(from, to),
            cost: 0.1,
            automatic: true,
        }
    }

    /// Create a custom replacement rule.
    pub fn replace(
        old_tokens: impl IntoIterator<Item = impl Into<String>>,
        new_tokens: impl IntoIterator<Item = impl Into<String>>,
        from: Version,
        to: Version,
    ) -> Self {
        let old_tokens: Vec<_> = old_tokens.into_iter().map(Into::into).collect();
        let new_tokens: Vec<_> = new_tokens.into_iter().map(Into::into).collect();
        let id = format!("replace_{}", old_tokens.join("_"));
        Self {
            id,
            description: format!(
                "Replace {} with {}",
                old_tokens.join(" "),
                new_tokens.join(" ")
            ),
            migration_type: MigrationType::ReplaceCall {
                old_pattern: old_tokens,
                new_pattern: new_tokens,
            },
            version_range: VersionRange::new(from, to),
            cost: 0.2,
            automatic: true,
        }
    }

    /// Create a deprecation/removal rule.
    pub fn deprecate(
        function: impl Into<String>,
        message: impl Into<String>,
        from: Version,
        to: Version,
    ) -> Self {
        let function = function.into();
        Self {
            id: format!("deprecate_{}", &function),
            description: format!("Mark {} as deprecated", &function),
            migration_type: MigrationType::RemoveFunction {
                function,
                message: message.into(),
            },
            version_range: VersionRange::new(from, to),
            cost: 1.0, // Higher cost - requires manual review
            automatic: false,
        }
    }

    /// Set the cost of this rule.
    pub fn with_cost(mut self, cost: f64) -> Self {
        self.cost = cost;
        self
    }

    /// Mark this rule as requiring manual review.
    pub fn manual_review(mut self) -> Self {
        self.automatic = false;
        self
    }

    /// Get the old pattern tokens for this rule.
    pub fn old_tokens(&self) -> Vec<&str> {
        match &self.migration_type {
            MigrationType::RenameFunction { old_name, .. } => vec![old_name.as_str()],
            MigrationType::RenameParameter { old_name, .. } => vec![old_name.as_str()],
            MigrationType::RenameType { old_name, .. } => vec![old_name.as_str()],
            MigrationType::ReplaceCall { old_pattern, .. } => {
                old_pattern.iter().map(|s| s.as_str()).collect()
            }
            MigrationType::RemoveFunction { function, .. } => vec![function.as_str()],
            MigrationType::ChangeSignature { function, .. } => vec![function.as_str()],
            MigrationType::AddParameter { function, .. } => vec![function.as_str()],
            MigrationType::RemoveParameter { function, .. } => vec![function.as_str()],
            MigrationType::Custom { old_tokens, .. } => {
                old_tokens.iter().map(|s| s.as_str()).collect()
            }
        }
    }

    /// Get the new pattern tokens for this rule.
    pub fn new_tokens(&self) -> Vec<&str> {
        match &self.migration_type {
            MigrationType::RenameFunction { new_name, .. } => vec![new_name.as_str()],
            MigrationType::RenameParameter { new_name, .. } => vec![new_name.as_str()],
            MigrationType::RenameType { new_name, .. } => vec![new_name.as_str()],
            MigrationType::ReplaceCall { new_pattern, .. } => {
                new_pattern.iter().map(|s| s.as_str()).collect()
            }
            MigrationType::RemoveFunction { message, .. } => vec![message.as_str()],
            MigrationType::ChangeSignature { function, .. } => vec![function.as_str()],
            MigrationType::AddParameter { function, .. } => vec![function.as_str()],
            MigrationType::RemoveParameter { function, .. } => vec![function.as_str()],
            MigrationType::Custom { new_tokens, .. } => {
                new_tokens.iter().map(|s| s.as_str()).collect()
            }
        }
    }
}

/// Statistics from a migration operation.
#[derive(Debug, Clone, Default)]
pub struct MigrationStats {
    /// Number of rules applied.
    pub rules_applied: usize,
    /// Number of automatic migrations.
    pub automatic_migrations: usize,
    /// Number of manual review items.
    pub manual_review_items: usize,
    /// Total cost of all migrations.
    pub total_cost: f64,
}

/// Result of a migration operation.
#[derive(Debug, Clone)]
pub struct MigrationResult {
    /// Original tokens.
    pub original: Vec<String>,
    /// Migrated tokens.
    pub migrated: Vec<String>,
    /// Rules that were applied.
    pub applied_rules: Vec<String>,
    /// Statistics.
    pub stats: MigrationStats,
}

/// API migration transducer.
///
/// Applies migration rules to transform code from one API version to another.
#[derive(Debug, Clone)]
pub struct ApiMigrationTransducer<W: Semiring> {
    /// Migration rules indexed by first token.
    rules_by_token: HashMap<String, Vec<ApiMigrationRule>>,
    /// All rules.
    all_rules: Vec<ApiMigrationRule>,
    /// Current source version.
    source_version: Version,
    /// Target version.
    target_version: Version,
    /// Weight type marker.
    _phantom: std::marker::PhantomData<W>,
}

impl<W: Semiring> ApiMigrationTransducer<W> {
    /// Create a new migration transducer.
    pub fn new(source_version: Version, target_version: Version) -> Self {
        Self {
            rules_by_token: HashMap::new(),
            all_rules: Vec::new(),
            source_version,
            target_version,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Add a migration rule.
    pub fn add_rule(&mut self, rule: ApiMigrationRule) {
        // Index by first token of old pattern
        if let Some(first_token) = rule.old_tokens().first() {
            self.rules_by_token
                .entry(first_token.to_string())
                .or_default()
                .push(rule.clone());
        }
        self.all_rules.push(rule);
    }

    /// Get rules applicable to the current version range.
    pub fn applicable_rules(&self) -> impl Iterator<Item = &ApiMigrationRule> {
        self.all_rules.iter().filter(|rule| {
            rule.version_range.contains(self.source_version)
                || rule.version_range.contains(self.target_version)
        })
    }

    /// Apply migration to a sequence of tokens.
    pub fn migrate(&self, tokens: &[String]) -> MigrationResult {
        let mut result = Vec::new();
        let mut applied_rules = Vec::new();
        let mut stats = MigrationStats::default();
        let mut i = 0;

        while i < tokens.len() {
            let token = &tokens[i];

            // Check if any rule matches starting at this position
            if let Some(rules) = self.rules_by_token.get(token) {
                let mut matched = false;

                for rule in rules {
                    // Check version range
                    if !rule.version_range.contains(self.source_version) {
                        continue;
                    }

                    let old_tokens = rule.old_tokens();

                    // Check if pattern matches
                    if tokens.len() >= i + old_tokens.len()
                        && tokens[i..i + old_tokens.len()]
                            .iter()
                            .zip(old_tokens.iter())
                            .all(|(a, b)| a == *b)
                    {
                        // Apply the rule
                        let new_tokens = rule.new_tokens();
                        result.extend(new_tokens.iter().map(|s| s.to_string()));
                        i += old_tokens.len();
                        applied_rules.push(rule.id.clone());

                        stats.rules_applied += 1;
                        stats.total_cost += rule.cost;
                        if rule.automatic {
                            stats.automatic_migrations += 1;
                        } else {
                            stats.manual_review_items += 1;
                        }

                        matched = true;
                        break;
                    }
                }

                if !matched {
                    result.push(token.clone());
                    i += 1;
                }
            } else {
                result.push(token.clone());
                i += 1;
            }
        }

        MigrationResult {
            original: tokens.to_vec(),
            migrated: result,
            applied_rules,
            stats,
        }
    }

    /// Build a WFST representation of the migration rules.
    ///
    /// The WFST maps old API token sequences to new ones.
    pub fn build_wfst(&self, weight_fn: impl Fn(f64) -> W) -> VectorWfst<String, W> {
        let mut fst = VectorWfst::new();
        let start = fst.add_state();
        fst.set_start(start);
        fst.set_final(start, W::one());

        // Add transitions for each rule
        for rule in &self.all_rules {
            if !rule.version_range.contains(self.source_version) {
                continue;
            }

            let old_tokens = rule.old_tokens();
            let new_tokens = rule.new_tokens();
            let weight = weight_fn(rule.cost);

            if old_tokens.is_empty() {
                continue;
            }

            // Build path for old tokens
            let mut current = start;
            for (idx, old_token) in old_tokens.iter().enumerate() {
                let is_last = idx == old_tokens.len() - 1;

                if is_last {
                    // Last token - output the replacement
                    let output = new_tokens.join(" ");
                    fst.add_transition(WeightedTransition::new(
                        current,
                        Some(old_token.to_string()),
                        Some(output),
                        start,
                        weight.clone(),
                    ));
                } else {
                    // Intermediate token - epsilon output
                    let next = fst.add_state();
                    fst.add_transition(WeightedTransition::new(
                        current,
                        Some(old_token.to_string()),
                        None, // epsilon output
                        next,
                        W::one(),
                    ));
                    current = next;
                }
            }
        }

        // Add identity transitions for non-matching tokens
        // (This is a simplification - real implementation would handle unknown tokens)

        fst
    }

    /// Get the source version.
    pub fn source_version(&self) -> Version {
        self.source_version
    }

    /// Get the target version.
    pub fn target_version(&self) -> Version {
        self.target_version
    }

    /// Get all rules.
    pub fn rules(&self) -> &[ApiMigrationRule] {
        &self.all_rules
    }
}

/// Builder for API migration transducers.
#[derive(Debug, Clone)]
pub struct ApiMigrationBuilder<W: Semiring> {
    rules: Vec<ApiMigrationRule>,
    source_version: Version,
    target_version: Version,
    _phantom: std::marker::PhantomData<W>,
}

impl<W: Semiring> ApiMigrationBuilder<W> {
    /// Create a new builder.
    pub fn new(source_version: Version, target_version: Version) -> Self {
        Self {
            rules: Vec::new(),
            source_version,
            target_version,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Add a migration rule.
    pub fn add_rule(mut self, rule: ApiMigrationRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Add multiple rules.
    pub fn add_rules(mut self, rules: impl IntoIterator<Item = ApiMigrationRule>) -> Self {
        self.rules.extend(rules);
        self
    }

    /// Build the transducer.
    pub fn build(self) -> ApiMigrationTransducer<W> {
        let mut transducer = ApiMigrationTransducer::new(self.source_version, self.target_version);
        for rule in self.rules {
            transducer.add_rule(rule);
        }
        transducer
    }
}

/// Common migration patterns for different frameworks/languages.
///
/// These pattern factories are part of the public API; the test
/// `test_python2_to_python3_migration` exercises one of them.
pub mod patterns {
    use super::*;

    /// Create rules for React class component to function component migration.
    pub fn react_class_to_function() -> Vec<ApiMigrationRule> {
        let v16 = Version::new(16, 8);
        let v18 = Version::new(18, 0);

        vec![
            ApiMigrationRule::replace(
                ["componentDidMount", "(", ")"],
                ["useEffect", "(", "(", ")", "=>", "{"],
                v16,
                v18,
            ),
            ApiMigrationRule::replace(
                ["componentWillUnmount", "(", ")"],
                [
                    "useEffect",
                    "(",
                    "(",
                    ")",
                    "=>",
                    "{",
                    "return",
                    "(",
                    ")",
                    "=>",
                    "{",
                ],
                v16,
                v18,
            ),
            ApiMigrationRule::replace(["this", ".", "setState"], ["setState"], v16, v18),
            ApiMigrationRule::replace(["this", ".", "state"], ["state"], v16, v18),
        ]
    }

    /// Create rules for Python 2 to Python 3 migration.
    pub fn python2_to_python3() -> Vec<ApiMigrationRule> {
        let v2 = Version::new(2, 7);
        let v3 = Version::new(3, 0);

        vec![
            ApiMigrationRule::replace(["print", "\""], ["print", "(", "\""], v2, v3),
            ApiMigrationRule::replace(["xrange"], ["range"], v2, v3),
            ApiMigrationRule::replace(["raw_input"], ["input"], v2, v3),
            ApiMigrationRule::replace(["unicode"], ["str"], v2, v3),
            ApiMigrationRule::rename_function("iteritems", "items", v2, v3),
            ApiMigrationRule::rename_function("iterkeys", "keys", v2, v3),
            ApiMigrationRule::rename_function("itervalues", "values", v2, v3),
        ]
    }

    /// Create rules for jQuery to vanilla JavaScript migration.
    pub fn jquery_to_vanilla_js() -> Vec<ApiMigrationRule> {
        let v1 = Version::new(1, 0);
        let v4 = Version::new(4, 0);

        vec![
            ApiMigrationRule::replace(
                ["$", "(", "\"#"],
                ["document", ".", "getElementById", "(", "\""],
                v1,
                v4,
            ),
            ApiMigrationRule::replace(
                ["$", "(", "\"."],
                ["document", ".", "querySelectorAll", "(", "\"."],
                v1,
                v4,
            ),
            ApiMigrationRule::replace(
                [".", "addClass", "("],
                [".", "classList", ".", "add", "("],
                v1,
                v4,
            ),
            ApiMigrationRule::replace(
                [".", "removeClass", "("],
                [".", "classList", ".", "remove", "("],
                v1,
                v4,
            ),
            ApiMigrationRule::replace(
                [".", "toggleClass", "("],
                [".", "classList", ".", "toggle", "("],
                v1,
                v4,
            ),
            ApiMigrationRule::replace([".", "attr", "("], [".", "getAttribute", "("], v1, v4),
        ]
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_version_parsing() {
        let v = Version::parse("1.2.3")
            .expect("programming/api_migration.rs: required value was None/Err");
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);

        let v = Version::parse("2.0")
            .expect("programming/api_migration.rs: required value was None/Err");
        assert_eq!(v.major, 2);
        assert_eq!(v.minor, 0);
        assert_eq!(v.patch, 0);

        let v =
            Version::parse("3").expect("programming/api_migration.rs: required value was None/Err");
        assert_eq!(v.major, 3);
        assert_eq!(v.minor, 0);
        assert_eq!(v.patch, 0);
    }

    #[test]
    fn test_version_comparison() {
        let v1 = Version::new(1, 0);
        let v2 = Version::new(2, 0);
        let v1_5 = Version::with_patch(1, 5, 0);

        assert!(v1 < v2);
        assert!(v1 < v1_5);
        assert!(v1_5 < v2);
    }

    #[test]
    fn test_version_range() {
        let range = VersionRange::new(Version::new(1, 0), Version::new(2, 0));

        assert!(range.contains(Version::new(1, 0)));
        assert!(range.contains(Version::new(1, 5)));
        assert!(range.contains(Version::new(2, 0)));
        assert!(!range.contains(Version::new(0, 9)));
        assert!(!range.contains(Version::new(2, 1)));
    }

    #[test]
    fn test_rename_function_rule() {
        let rule = ApiMigrationRule::rename_function(
            "old_fn",
            "new_fn",
            Version::new(1, 0),
            Version::new(2, 0),
        );

        assert_eq!(rule.old_tokens(), vec!["old_fn"]);
        assert_eq!(rule.new_tokens(), vec!["new_fn"]);
        assert!(rule.automatic);
    }

    #[test]
    fn test_basic_migration() {
        let mut transducer: ApiMigrationTransducer<TropicalWeight> =
            ApiMigrationTransducer::new(Version::new(1, 0), Version::new(2, 0));

        transducer.add_rule(ApiMigrationRule::rename_function(
            "old_fn",
            "new_fn",
            Version::new(1, 0),
            Version::new(2, 0),
        ));

        let tokens = vec![
            "call".to_string(),
            "old_fn".to_string(),
            "(".to_string(),
            ")".to_string(),
        ];
        let result = transducer.migrate(&tokens);

        assert_eq!(result.migrated, vec!["call", "new_fn", "(", ")"]);
        assert_eq!(result.stats.rules_applied, 1);
        assert_eq!(result.stats.automatic_migrations, 1);
    }

    #[test]
    fn test_multi_token_migration() {
        let mut transducer: ApiMigrationTransducer<TropicalWeight> =
            ApiMigrationTransducer::new(Version::new(1, 0), Version::new(2, 0));

        transducer.add_rule(ApiMigrationRule::replace(
            ["old", "method"],
            ["new_method"],
            Version::new(1, 0),
            Version::new(2, 0),
        ));

        let tokens = vec!["obj".to_string(), "old".to_string(), "method".to_string()];
        let result = transducer.migrate(&tokens);

        assert_eq!(result.migrated, vec!["obj", "new_method"]);
        assert_eq!(result.stats.rules_applied, 1);
    }

    #[test]
    fn test_version_filtering() {
        let mut transducer: ApiMigrationTransducer<TropicalWeight> =
            ApiMigrationTransducer::new(Version::new(1, 0), Version::new(2, 0));

        // Rule only applies to v1.0-v1.5
        transducer.add_rule(ApiMigrationRule::rename_function(
            "v1_fn",
            "v1_5_fn",
            Version::new(1, 0),
            Version::with_patch(1, 5, 0),
        ));

        // This should match since source is v1.0
        let tokens = vec!["v1_fn".to_string()];
        let result = transducer.migrate(&tokens);
        assert_eq!(result.migrated, vec!["v1_5_fn"]);
    }

    #[test]
    fn test_no_match() {
        let mut transducer: ApiMigrationTransducer<TropicalWeight> =
            ApiMigrationTransducer::new(Version::new(1, 0), Version::new(2, 0));

        transducer.add_rule(ApiMigrationRule::rename_function(
            "foo",
            "bar",
            Version::new(1, 0),
            Version::new(2, 0),
        ));

        let tokens = vec!["baz".to_string(), "qux".to_string()];
        let result = transducer.migrate(&tokens);

        assert_eq!(result.migrated, vec!["baz", "qux"]);
        assert_eq!(result.stats.rules_applied, 0);
    }

    #[test]
    fn test_builder() {
        let transducer: ApiMigrationTransducer<TropicalWeight> =
            ApiMigrationBuilder::new(Version::new(1, 0), Version::new(2, 0))
                .add_rule(ApiMigrationRule::rename_function(
                    "a",
                    "b",
                    Version::new(1, 0),
                    Version::new(2, 0),
                ))
                .add_rule(ApiMigrationRule::rename_type(
                    "OldType",
                    "NewType",
                    Version::new(1, 0),
                    Version::new(2, 0),
                ))
                .build();

        assert_eq!(transducer.rules().len(), 2);
    }

    #[test]
    fn test_python_migration_rules() {
        let rules = patterns::python2_to_python3();
        assert!(!rules.is_empty());

        // Check xrange -> range exists
        let xrange_rule = rules.iter().find(|r| {
            matches!(&r.migration_type, MigrationType::ReplaceCall { old_pattern, .. }
                if old_pattern == &["xrange"])
        });
        assert!(xrange_rule.is_some());
    }

    #[test]
    fn test_build_wfst() {
        let transducer: ApiMigrationTransducer<TropicalWeight> =
            ApiMigrationBuilder::new(Version::new(1, 0), Version::new(2, 0))
                .add_rule(ApiMigrationRule::rename_function(
                    "old",
                    "new",
                    Version::new(1, 0),
                    Version::new(2, 0),
                ))
                .build();

        let fst = transducer.build_wfst(TropicalWeight::new);
        // Check that start state is valid (not NO_STATE)
        assert_ne!(fst.start(), crate::wfst::NO_STATE);
    }

    #[test]
    fn test_deprecation_rule() {
        let rule = ApiMigrationRule::deprecate(
            "deprecated_fn",
            "Use new_fn instead",
            Version::new(1, 0),
            Version::new(2, 0),
        );

        assert!(!rule.automatic);
        assert!(rule.cost > 0.5); // Higher cost for manual review

        let mut transducer: ApiMigrationTransducer<TropicalWeight> =
            ApiMigrationTransducer::new(Version::new(1, 0), Version::new(2, 0));
        transducer.add_rule(rule);

        let tokens = vec!["deprecated_fn".to_string()];
        let result = transducer.migrate(&tokens);

        assert_eq!(result.stats.manual_review_items, 1);
        assert_eq!(result.stats.automatic_migrations, 0);
    }

    #[test]
    fn test_multiple_rules_in_sequence() {
        let mut transducer: ApiMigrationTransducer<TropicalWeight> =
            ApiMigrationTransducer::new(Version::new(1, 0), Version::new(2, 0));

        transducer.add_rule(ApiMigrationRule::rename_function(
            "foo",
            "bar",
            Version::new(1, 0),
            Version::new(2, 0),
        ));
        transducer.add_rule(ApiMigrationRule::rename_function(
            "baz",
            "qux",
            Version::new(1, 0),
            Version::new(2, 0),
        ));

        let tokens = vec![
            "foo".to_string(),
            "(".to_string(),
            ")".to_string(),
            ";".to_string(),
            "baz".to_string(),
            "(".to_string(),
            ")".to_string(),
        ];
        let result = transducer.migrate(&tokens);

        assert_eq!(result.migrated, vec!["bar", "(", ")", ";", "qux", "(", ")"]);
        assert_eq!(result.stats.rules_applied, 2);
    }

    #[test]
    fn test_version_display() {
        let v = Version::with_patch(1, 2, 3);
        assert_eq!(format!("{}", v), "1.2.3");
    }

    #[test]
    fn test_version_range_display() {
        let range = VersionRange::new(Version::new(1, 0), Version::new(2, 0));
        assert_eq!(format!("{}", range), "1.0.0 - 2.0.0");
    }

    #[test]
    fn test_rule_with_cost() {
        let rule =
            ApiMigrationRule::rename_function("a", "b", Version::new(1, 0), Version::new(2, 0))
                .with_cost(0.5);

        assert_eq!(rule.cost, 0.5);
    }

    mod property_tests {
        use super::*;
        use proptest::prelude::*;

        fn arb_ident() -> impl Strategy<Value = String> {
            proptest::collection::vec(
                proptest::char::range('a', 'z').prop_map(|c| c as char),
                1..=8,
            )
            .prop_map(|v| v.into_iter().collect())
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(48))]

            /// Idempotence: migrating an already-migrated token stream is a
            /// no-op (the renamed identifier is not re-renamed because the
            /// rule's source pattern no longer matches).
            #[test]
            fn migration_is_idempotent(
                old in arb_ident(),
                new in arb_ident(),
            ) {
                prop_assume!(old != new);
                let mut transducer: ApiMigrationTransducer<crate::semiring::TropicalWeight> =
                    ApiMigrationTransducer::new(Version::new(1, 0), Version::new(2, 0));
                transducer.add_rule(ApiMigrationRule::rename_function(
                    &old,
                    &new,
                    Version::new(1, 0),
                    Version::new(2, 0),
                ));

                let tokens = vec![old.clone(), "(".to_string(), ")".to_string()];
                let first = transducer.migrate(&tokens);
                let second = transducer.migrate(&first.migrated);
                prop_assert_eq!(first.migrated, second.migrated);
            }

            /// Migrating an empty token stream yields an empty token stream.
            #[test]
            fn empty_input_yields_empty_output(
                old in arb_ident(),
                new in arb_ident(),
            ) {
                let mut transducer: ApiMigrationTransducer<crate::semiring::TropicalWeight> =
                    ApiMigrationTransducer::new(Version::new(1, 0), Version::new(2, 0));
                transducer.add_rule(ApiMigrationRule::rename_function(
                    &old,
                    &new,
                    Version::new(1, 0),
                    Version::new(2, 0),
                ));
                let result = transducer.migrate(&[]);
                prop_assert!(result.migrated.is_empty());
                prop_assert_eq!(result.stats.rules_applied, 0);
            }

            /// Renaming with no matching tokens leaves the input unchanged.
            #[test]
            fn no_match_preserves_input(
                old in arb_ident(),
                new in arb_ident(),
                input in proptest::collection::vec(arb_ident(), 0..6),
            ) {
                let mut transducer: ApiMigrationTransducer<crate::semiring::TropicalWeight> =
                    ApiMigrationTransducer::new(Version::new(1, 0), Version::new(2, 0));
                transducer.add_rule(ApiMigrationRule::rename_function(
                    &old,
                    &new,
                    Version::new(1, 0),
                    Version::new(2, 0),
                ));
                prop_assume!(!input.contains(&old));
                let result = transducer.migrate(&input);
                prop_assert_eq!(result.migrated, input);
                prop_assert_eq!(result.stats.rules_applied, 0);
            }
        }
    }
}
