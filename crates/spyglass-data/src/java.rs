//! Finding a Java runtime to run the game's data generator.
//!
//! Search order: an explicit path, `JAVA_HOME`, `PATH`, standard install locations, then the
//! runtimes that game launchers download. Versions are read from each installation's `release`
//! file rather than by running it. See `docs/specs/data-sources.md`.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

const JAVA_EXE: &str = if cfg!(windows) { "java.exe" } else { "java" };
const JAVAC_EXE: &str = if cfg!(windows) { "javac.exe" } else { "javac" };

/// How deep to look below a scanned folder for a Java home. The official launcher nests homes as
/// `<component>/<platform>/<component>/`, and macOS homes add `jre.bundle/Contents/Home`.
const SCAN_DEPTH: usize = 6;

/// Where a Java installation was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JavaSource {
    /// A path given explicitly (`--java`).
    Explicit,
    /// The `JAVA_HOME` environment variable.
    JavaHome,
    /// A `java` found on `PATH`.
    Path,
    /// A standard install location (vendor folder, SDK manager), named for display.
    Installed(&'static str),
    /// A runtime downloaded by a game launcher, named for display.
    Launcher(&'static str),
}

impl fmt::Display for JavaSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JavaSource::Explicit => f.write_str("--java"),
            JavaSource::JavaHome => f.write_str("JAVA_HOME"),
            JavaSource::Path => f.write_str("PATH"),
            JavaSource::Installed(name) | JavaSource::Launcher(name) => write!(f, "{name}"),
        }
    }
}

/// A Java installation with a readable version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaInstall {
    /// The Java home: the folder holding `bin/` and `release`.
    pub home: PathBuf,
    /// `bin/java` inside the home.
    pub executable: PathBuf,
    /// `JAVA_VERSION` from the `release` file, e.g. `25.0.3`.
    pub version: String,
    /// The major version, e.g. 25 (or 8 for `1.8.0_51`).
    pub major: u32,
    /// Whether `javac` is present, i.e. this is a JDK and not just a runtime.
    pub is_jdk: bool,
    /// Where it was found.
    pub source: JavaSource,
}

/// No installation matched the required major version.
#[derive(Debug, thiserror::Error)]
#[error("no Java {required} runtime found (searched: {})", display_paths(.searched))]
pub struct JavaNotFound {
    /// The major version that was needed.
    pub required: u32,
    /// Installations that were found but didn't match.
    pub found: Vec<JavaInstall>,
    /// Every location that was searched, found or not.
    pub searched: Vec<PathBuf>,
}

fn display_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A location to search, before it's been looked at.
#[derive(Debug, Clone)]
struct Root {
    path: PathBuf,
    source: JavaSource,
    /// `true` if `path` is a Java home; `false` if it's a folder of homes to scan.
    is_home: bool,
}

/// Every location [`discover`] searches, in priority order.
fn roots(explicit: Option<&Path>) -> Vec<Root> {
    let mut roots = Vec::new();
    if let Some(path) = explicit {
        // Accept either a home or the executable itself.
        let home = if path.is_file() {
            home_of_executable(path)
        } else {
            Some(path.to_path_buf())
        };
        if let Some(home) = home {
            roots.push(Root {
                path: home,
                source: JavaSource::Explicit,
                is_home: true,
            });
        }
    }
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        roots.push(Root {
            path: home.into(),
            source: JavaSource::JavaHome,
            is_home: true,
        });
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let exe = dir.join(JAVA_EXE);
            if let Some(home) = exe.is_file().then(|| home_of_executable(&exe)).flatten() {
                roots.push(Root {
                    path: home,
                    source: JavaSource::Path,
                    is_home: true,
                });
            }
        }
    }
    roots.extend(installed_roots());
    roots.extend(launcher_roots());
    roots
}

/// Folders where JDK installers and SDK managers put Java homes on this platform.
fn installed_roots() -> Vec<Root> {
    let scan = |path: PathBuf, name| Root {
        path,
        source: JavaSource::Installed(name),
        is_home: false,
    };
    let mut roots = Vec::new();
    if cfg!(windows) {
        if let Some(pf) = std::env::var_os("ProgramFiles").map(PathBuf::from) {
            for (dir, name) in [
                ("Eclipse Adoptium", "Eclipse Adoptium"),
                ("Java", "Program Files\\Java"),
                ("Microsoft", "Microsoft Build of OpenJDK"),
                ("Zulu", "Azul Zulu"),
                ("Amazon Corretto", "Amazon Corretto"),
                ("BellSoft", "BellSoft Liberica"),
            ] {
                roots.push(scan(pf.join(dir), name));
            }
        }
    } else if cfg!(target_os = "macos") {
        roots.push(scan(
            PathBuf::from("/Library/Java/JavaVirtualMachines"),
            "JavaVirtualMachines",
        ));
    } else {
        roots.push(scan(PathBuf::from("/usr/lib/jvm"), "/usr/lib/jvm"));
    }
    if let Some(dirs) = directories::BaseDirs::new() {
        let home = dirs.home_dir();
        if cfg!(target_os = "macos") {
            roots.push(scan(
                home.join("Library/Java/JavaVirtualMachines"),
                "JavaVirtualMachines",
            ));
        }
        roots.push(scan(home.join(".sdkman/candidates/java"), "SDKMAN"));
        roots.push(scan(home.join(".jdks"), "~/.jdks"));
    }
    roots
}

/// Runtime folders of known game launchers on this platform.
fn launcher_roots() -> Vec<Root> {
    let Some(dirs) = directories::BaseDirs::new() else {
        return Vec::new();
    };
    let scan = |path: PathBuf, name| Root {
        path,
        source: JavaSource::Launcher(name),
        is_home: false,
    };
    let mut roots = Vec::new();
    if cfg!(windows) {
        roots.push(scan(
            dirs.data_dir().join(".minecraft").join("runtime"),
            "Minecraft Launcher",
        ));
        roots.push(scan(
            dirs.data_local_dir()
                .join("Packages")
                .join("Microsoft.4297127D64EC6_8wekyb3d8bbwe")
                .join("LocalCache")
                .join("Local")
                .join("runtime"),
            "Minecraft Launcher (Microsoft Store)",
        ));
    } else if cfg!(target_os = "macos") {
        roots.push(scan(
            dirs.data_dir().join("minecraft").join("runtime"),
            "Minecraft Launcher",
        ));
    } else {
        roots.push(scan(
            dirs.home_dir().join(".minecraft").join("runtime"),
            "Minecraft Launcher",
        ));
    }
    roots.push(scan(
        dirs.data_dir()
            .join("ModrinthApp")
            .join("meta")
            .join("java_versions"),
        "Modrinth App",
    ));
    roots
}

/// The Java home for an executable at `<home>/bin/java`, following symlinks so that e.g.
/// `/usr/bin/java` resolves to the real installation.
fn home_of_executable(exe: &Path) -> Option<PathBuf> {
    let exe = fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
    Some(exe.parent()?.parent()?.to_path_buf())
}

/// Reads a Java home, or returns `None` if it has no executable or no readable version.
fn read_home(home: &Path, source: JavaSource) -> Option<JavaInstall> {
    let executable = home.join("bin").join(JAVA_EXE);
    if !executable.is_file() {
        return None;
    }
    let release = fs::read_to_string(home.join("release")).ok()?;
    let version = parse_release_version(&release)?;
    let major = parse_major(&version)?;
    Some(JavaInstall {
        home: home.to_path_buf(),
        is_jdk: home.join("bin").join(JAVAC_EXE).is_file(),
        executable,
        version,
        major,
        source,
    })
}

/// Collects Java homes at or below `dir`, up to `depth` levels down.
fn scan_homes(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if dir.join("release").is_file() && dir.join("bin").join(JAVA_EXE).is_file() {
        out.push(dir.to_path_buf());
        return;
    }
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    children.sort();
    for child in children {
        scan_homes(&child, depth - 1, out);
    }
}

fn discover_in(roots: &[Root]) -> Vec<JavaInstall> {
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for root in roots {
        let homes = if root.is_home {
            vec![root.path.clone()]
        } else {
            let mut homes = Vec::new();
            scan_homes(&root.path, SCAN_DEPTH, &mut homes);
            homes
        };
        for home in homes {
            let key = fs::canonicalize(&home).unwrap_or_else(|_| home.clone());
            if !seen.insert(key) {
                continue;
            }
            if let Some(install) = read_home(&home, root.source.clone()) {
                found.push(install);
            }
        }
    }
    found
}

/// Every Java installation that can be found, in search order, without duplicates.
pub fn discover(explicit: Option<&Path>) -> Vec<JavaInstall> {
    discover_in(&roots(explicit))
}

/// The first installation, in search order, whose major version is `required`.
pub fn find(explicit: Option<&Path>, required: u32) -> Result<JavaInstall, JavaNotFound> {
    let roots = roots(explicit);
    let found = discover_in(&roots);
    match found.iter().position(|j| j.major == required) {
        Some(i) => Ok(found[i].clone()),
        None => Err(JavaNotFound {
            required,
            found,
            searched: roots.into_iter().map(|r| r.path).collect(),
        }),
    }
}

/// `JAVA_VERSION` from a `release` file, without quotes.
fn parse_release_version(release: &str) -> Option<String> {
    release.lines().find_map(|line| {
        let value = line.strip_prefix("JAVA_VERSION=")?.trim().trim_matches('"');
        (!value.is_empty()).then(|| value.to_string())
    })
}

/// The major version: `25.0.3` → 25, and the old scheme `1.8.0_51` → 8.
fn parse_major(version: &str) -> Option<u32> {
    let mut parts = version.split(['.', '_', '+', '-']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_home(dir: &Path, version: &str, jdk: bool) {
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::write(dir.join("bin").join(JAVA_EXE), "").unwrap();
        if jdk {
            fs::write(dir.join("bin").join(JAVAC_EXE), "").unwrap();
        }
        fs::write(
            dir.join("release"),
            format!("IMPLEMENTOR=\"Test\"\nJAVA_VERSION=\"{version}\"\n"),
        )
        .unwrap();
    }

    #[test]
    fn parses_release_version() {
        let release =
            "IMPLEMENTOR=\"Azul Systems, Inc.\"\nJAVA_VERSION=\"25.0.3\"\nOS_NAME=\"Windows\"\n";
        assert_eq!(parse_release_version(release).as_deref(), Some("25.0.3"));
        assert_eq!(parse_release_version("OS_NAME=\"Linux\"\n"), None);
    }

    #[test]
    fn parses_major_versions() {
        assert_eq!(parse_major("25.0.3"), Some(25));
        assert_eq!(parse_major("21"), Some(21));
        assert_eq!(parse_major("1.8.0_51"), Some(8));
        assert_eq!(parse_major("17.0.5+8"), Some(17));
        assert_eq!(parse_major("garbage"), None);
    }

    #[test]
    fn scans_launcher_layout_and_respects_priority() {
        let tmp = tempfile::tempdir().unwrap();
        let explicit = tmp.path().join("jdk-17");
        fake_home(&explicit, "17.0.5", true);
        // Official launcher layout: runtime/<component>/<platform>/<component>/
        let runtime = tmp.path().join("runtime");
        fake_home(
            &runtime.join("java-runtime-delta/windows-x64/java-runtime-delta"),
            "21.0.7",
            false,
        );
        fake_home(
            &runtime.join("java-runtime-epsilon/windows-x64/java-runtime-epsilon"),
            "25.0.3",
            false,
        );
        // Not a Java home: no release file.
        fs::create_dir_all(runtime.join("broken/bin")).unwrap();

        let roots = [
            Root {
                path: explicit.clone(),
                source: JavaSource::Explicit,
                is_home: true,
            },
            Root {
                path: runtime.clone(),
                source: JavaSource::Launcher("Test"),
                is_home: false,
            },
            // Same home again; must be deduplicated.
            Root {
                path: explicit.clone(),
                source: JavaSource::JavaHome,
                is_home: true,
            },
        ];
        let found = discover_in(&roots);
        let summary: Vec<_> = found
            .iter()
            .map(|j| (j.major, j.is_jdk, j.source.clone()))
            .collect();
        assert_eq!(
            summary,
            [
                (17, true, JavaSource::Explicit),
                (21, false, JavaSource::Launcher("Test")),
                (25, false, JavaSource::Launcher("Test")),
            ]
        );
    }
}
