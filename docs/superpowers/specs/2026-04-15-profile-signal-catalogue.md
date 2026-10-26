# Profile Signal Invariant Catalogue

**Date:** 2026-04-15
**Status:** Draft
**Depends on:** [../../philosophy/06-observation-invariants-and-grounding-eligibility.md](../../philosophy/06-observation-invariants-and-grounding-eligibility.md), [2026-04-14-language-and-profile-catalogue.md](2026-04-14-language-and-profile-catalogue.md)

## Goal

Bind the remaining non-entry-point `ProjectProfile` signals to named invariants so `get_profile` can be promoted honestly. This catalogue covers build-system detection, framework detection, language-distribution aggregation, and module mapping. It does not change runtime behavior.

## Invariant Catalogue

### Build System Detection

| ID | Property | Evidence | Tier |
|---|---|---|---|
| `I-PROFILE-BUILD-SYSTEM-MAVEN` | `pom.xml` yields `buildSystem.name == "Maven"` | `crates/repolens-profile/src/profile.rs::detects_maven_build_system` | `confirmOnly` |
| `I-PROFILE-BUILD-SYSTEM-GRADLE` | `build.gradle` and `build.gradle.kts` both yield `buildSystem.name == "Gradle"` and preserve the matched manifest path | `crates/repolens-profile/src/profile.rs::detects_gradle_build_system_from_supported_manifests` | `confirmOnly` |
| `I-PROFILE-BUILD-SYSTEM-NPM` | `package.json` yields `buildSystem.name == "npm"` | `crates/repolens-profile/src/profile.rs::detects_npm_build_system` | `confirmOnly` |
| `I-PROFILE-BUILD-SYSTEM-CARGO` | `Cargo.toml` yields `buildSystem.name == "Cargo"` | `crates/repolens-profile/src/profile.rs::detects_cargo_build_system` | `confirmOnly` |
| `I-PROFILE-BUILD-SYSTEM-GO` | `go.mod` yields `buildSystem.name == "Go"` | `crates/repolens-profile/src/profile.rs::detects_go_build_system` | `confirmOnly` |
| `I-PROFILE-BUILD-SYSTEM-PYTHON` | `pyproject.toml` and `setup.py` both yield `buildSystem.name == "Python"` and preserve the matched manifest path | `crates/repolens-profile/src/profile.rs::detects_python_build_system_from_supported_manifests` | `confirmOnly` |
| `I-PROFILE-BUILD-SYSTEM-NONE` | Repos without a recognized build manifest return `buildSystem == null` | `crates/repolens-profile/src/profile.rs::no_build_system_returns_none` | `confirmOnly` |

### Framework Detection

| ID | Property | Evidence | Tier |
|---|---|---|---|
| `I-PROFILE-FRAMEWORK-REACT` | `package.json` dependency on `react` yields a `React` framework entry and `ProjectType::WebApp` | `crates/repolens-profile/src/profile.rs::detects_react_framework_from_package_json` | `confirmOnly` |
| `I-PROFILE-FRAMEWORK-EXPRESS` | `package.json` dependency on `express` yields an `Express` framework entry and `ProjectType::WebApp` | `crates/repolens-profile/src/profile.rs::detects_express_framework_from_package_json` | `confirmOnly` |
| `I-PROFILE-FRAMEWORK-SPRING-BOOT` | `pom.xml` containing `spring-boot` yields a `Spring Boot` framework entry and `ProjectType::WebApp` | `crates/repolens-profile/src/profile.rs::detects_spring_boot_framework_from_pom_xml` | `confirmOnly` |
| `I-PROFILE-FRAMEWORK-SPRING` | `pom.xml` containing `org.springframework` yields a `Spring` framework entry and `ProjectType::WebApp` | `crates/repolens-profile/src/profile.rs::detects_spring_framework_from_pom_xml` | `confirmOnly` |
| `I-PROFILE-FRAMEWORK-DJANGO` | Python content containing `from django` or `import django` yields a `Django` framework entry and `ProjectType::WebApp` | `crates/repolens-profile/src/profile.rs::detects_django_framework_from_python_import` | `confirmOnly` |
| `I-PROFILE-FRAMEWORK-FLASK` | Python content containing `from flask` or `import flask` yields a `Flask` framework entry and `ProjectType::WebApp` | `crates/repolens-profile/src/profile.rs::detects_flask_framework_from_python_import` | `confirmOnly` |

### LOC-Backed Profile Aggregations

| ID | Property | Evidence | Tier |
|---|---|---|---|
| `I-PROFILE-LANGUAGE-DISTRIBUTION-SUMS-LOC` | `languageDistribution` sums `FileEntry.loc` by language and preserves exact totals | `crates/repolens-profile/src/profile.rs::computes_language_distribution` | `confirmOnly` |
| `I-PROFILE-MODULES-AGGREGATE-TOP-LEVEL-LOC` | `modules` group files by top-level path component, carry exact file counts, and sum LOC per module | `crates/repolens-profile/src/profile.rs::computes_modules` | `confirmOnly` |
| `I-PROFILE-MODULES-SKIP-DOTFOLDERS` | Top-level dot-directories do not become modules | `crates/repolens-profile/src/profile.rs::skips_dotfolder_top_level_modules` | `confirmOnly` |

## Maintenance Rule

If any cited test here is deleted, renamed, `#[ignore]`d, or weakened, the corresponding family row in doc 06 §5 must be demoted in the same change unless equivalent coverage is added simultaneously.
