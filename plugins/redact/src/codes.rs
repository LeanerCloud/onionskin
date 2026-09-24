//! Redaction codes: the exemption a redacted area cites, written over it as
//! overlay text. Acrobat's two U.S. sets are built in; the user's own sets
//! are kept as text files, one code a line, and can be added, renamed,
//! removed, imported and exported.

use std::path::{Path, PathBuf};

/// A named list of codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeSet {
    pub name: String,
    pub codes: Vec<String>,
    /// A built-in set cannot be changed or removed.
    pub built_in: bool,
}

/// The U.S. Freedom of Information Act exemptions, 5 U.S.C. 552(b).
const FOIA: [&str; 14] = [
    "(b)(1)",
    "(b)(2)",
    "(b)(3)",
    "(b)(4)",
    "(b)(5)",
    "(b)(6)",
    "(b)(7)(A)",
    "(b)(7)(B)",
    "(b)(7)(C)",
    "(b)(7)(D)",
    "(b)(7)(E)",
    "(b)(7)(F)",
    "(b)(8)",
    "(b)(9)",
];

/// The U.S. Privacy Act exemptions, 5 U.S.C. 552a.
const PRIVACY_ACT: [&str; 10] = [
    "(d)(5)", "(j)(1)", "(j)(2)", "(k)(1)", "(k)(2)", "(k)(3)", "(k)(4)", "(k)(5)", "(k)(6)",
    "(k)(7)",
];

pub fn built_in() -> Vec<CodeSet> {
    [
        ("U.S. FOIA", FOIA.as_slice()),
        ("U.S. Privacy Act", PRIVACY_ACT.as_slice()),
    ]
    .into_iter()
    .map(|(name, codes)| CodeSet {
        name: name.to_owned(),
        codes: codes.iter().map(|code| (*code).to_owned()).collect(),
        built_in: true,
    })
    .collect()
}

#[derive(Debug)]
pub enum CodeError {
    /// A name that is empty, a built-in set's, or not usable as a file name.
    BadName(String),
    /// A set by that name exists already.
    Exists(String),
    /// No user set has that name.
    Missing(String),
    Io(std::io::Error),
}

impl std::fmt::Display for CodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadName(name) => write!(f, "{name:?} cannot name a code set"),
            Self::Exists(name) => write!(f, "there is a code set named {name:?} already"),
            Self::Missing(name) => write!(f, "there is no code set of yours named {name:?}"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for CodeError {}

impl From<std::io::Error> for CodeError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Where the user's sets are kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeLibrary {
    dir: PathBuf,
}

const DIR: &str = "redaction-codes";
const EXTENSION: &str = "txt";

impl CodeLibrary {
    pub fn in_data_dir(data_dir: &Path) -> Self {
        CodeLibrary {
            dir: data_dir.join(DIR),
        }
    }

    /// The built-in sets, then the user's by name.
    pub fn sets(&self) -> Vec<CodeSet> {
        let mut own: Vec<CodeSet> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| read_set(&entry.path()))
            .collect();
        own.sort_by(|a, b| a.name.cmp(&b.name));
        built_in().into_iter().chain(own).collect()
    }

    /// Keeps `set` under its name, replacing a set of the user's with it.
    pub fn save(&self, name: &str, codes: &[String]) -> Result<CodeSet, CodeError> {
        let path = self.path(name)?;
        std::fs::create_dir_all(&self.dir)?;
        let kept: Vec<&str> = codes
            .iter()
            .map(|code| code.trim())
            .filter(|code| !code.is_empty())
            .collect();
        std::fs::write(&path, kept.join("\n") + "\n")?;
        read_set(&path).ok_or_else(|| CodeError::BadName(name.to_owned()))
    }

    pub fn rename(&self, from: &str, to: &str) -> Result<(), CodeError> {
        let source = self.existing(from)?;
        let target = self.path(to)?;
        if target.exists() {
            return Err(CodeError::Exists(to.to_owned()));
        }
        std::fs::rename(source, target)?;
        Ok(())
    }

    pub fn remove(&self, name: &str) -> Result<(), CodeError> {
        std::fs::remove_file(self.existing(name)?)?;
        Ok(())
    }

    /// A set from a text file, one code a line, named after the file.
    pub fn import(&self, file: &Path) -> Result<CodeSet, CodeError> {
        let name = file
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        if self.path(&name)?.exists() {
            return Err(CodeError::Exists(name));
        }
        let text = std::fs::read_to_string(file)?;
        let codes: Vec<String> = text.lines().map(str::to_owned).collect();
        self.save(&name, &codes)
    }

    /// `set` written to `file`, one code a line.
    pub fn export(set: &CodeSet, file: &Path) -> Result<(), CodeError> {
        std::fs::write(file, set.codes.join("\n") + "\n")?;
        Ok(())
    }

    fn path(&self, name: &str) -> Result<PathBuf, CodeError> {
        let name = name.trim();
        let usable = !name.is_empty()
            && !name.starts_with('.')
            && !name.contains(['/', '\\', ':'])
            && !built_in().iter().any(|set| set.name == name);
        if !usable {
            return Err(CodeError::BadName(name.to_owned()));
        }
        Ok(self.dir.join(format!("{name}.{EXTENSION}")))
    }

    fn existing(&self, name: &str) -> Result<PathBuf, CodeError> {
        let path = self.path(name)?;
        if path.exists() {
            Ok(path)
        } else {
            Err(CodeError::Missing(name.to_owned()))
        }
    }
}

fn read_set(path: &Path) -> Option<CodeSet> {
    if path.extension().and_then(|extension| extension.to_str()) != Some(EXTENSION) {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    Some(CodeSet {
        name: path.file_stem()?.to_string_lossy().into_owned(),
        codes: text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect(),
        built_in: false,
    })
}
