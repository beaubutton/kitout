//! `json-merge` / `toml-merge`: converge structured config keys without
//! clobbering the rest of the file. Two modes, matching the two philosophies
//! Setup.sh used:
//!   converge (default) — manifest values are enforced (Claude statusLine)
//!   seed — only write keys that are absent; user tweaks survive (codex,
//!          gemini footer)
//! toml-merge uses toml_edit, so comments and formatting in the target file
//! are preserved.

use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::step::{Applied, Change, ConflictPolicy, Status, Step};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MergeMode {
    #[default]
    Converge,
    Seed,
}

// ---------------------------------------------------------------- JSON -----

pub struct JsonMergeStep {
    pub id: String,
    pub needs: Vec<String>,
    pub target: PathBuf,
    pub mode: MergeMode,
    pub value: toml::Value,
}

fn toml_to_json(v: &toml::Value) -> serde_json::Value {
    use serde_json::Value as J;
    match v {
        toml::Value::String(s) => J::String(s.clone()),
        toml::Value::Integer(i) => J::from(*i),
        toml::Value::Float(f) => serde_json::Number::from_f64(*f).map(J::Number).unwrap_or(J::Null),
        toml::Value::Boolean(b) => J::Bool(*b),
        toml::Value::Datetime(d) => J::String(d.to_string()),
        toml::Value::Array(a) => J::Array(a.iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => {
            J::Object(t.iter().map(|(k, v)| (k.clone(), toml_to_json(v))).collect())
        }
    }
}

/// Deep-merge `src` into `dst`. Returns true when `dst` changed.
fn merge_json(dst: &mut serde_json::Value, src: &serde_json::Value, seed: bool) -> bool {
    use serde_json::Value as J;
    match (dst, src) {
        (J::Object(d), J::Object(s)) => {
            let mut changed = false;
            for (k, v) in s {
                match d.get_mut(k) {
                    Some(existing) if existing.is_object() && v.is_object() => {
                        changed |= merge_json(existing, v, seed);
                    }
                    Some(existing) => {
                        if !seed && existing != v {
                            *existing = v.clone();
                            changed = true;
                        }
                    }
                    None => {
                        d.insert(k.clone(), v.clone());
                        changed = true;
                    }
                }
            }
            changed
        }
        (dst, src) => {
            if !seed && dst != src {
                *dst = src.clone();
                true
            } else {
                false
            }
        }
    }
}

impl JsonMergeStep {
    /// (current-doc, merged-doc, changed)
    fn compute(&self) -> Result<(serde_json::Value, serde_json::Value, bool)> {
        let current: serde_json::Value = match fs::read_to_string(&self.target) {
            Ok(raw) => serde_json::from_str(&raw)
                .with_context(|| format!("{} is not valid JSON", self.target.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
            Err(e) => return Err(e).context("reading target")?,
        };
        let mut merged = current.clone();
        let changed = merge_json(&mut merged, &toml_to_json(&self.value), self.mode == MergeMode::Seed);
        Ok((current, merged, changed))
    }
}

impl Step for JsonMergeStep {
    fn id(&self) -> &str {
        &self.id
    }
    fn needs(&self) -> &[String] {
        &self.needs
    }
    fn resource(&self) -> Option<String> {
        Some(self.target.display().to_string())
    }

    fn check(&self) -> Result<Status> {
        Ok(if self.compute()?.2 {
            Status::Pending("keys missing or diverged".into())
        } else {
            Status::Satisfied
        })
    }

    fn plan(&self) -> Result<Vec<Change>> {
        let (current, merged, changed) = self.compute()?;
        if !changed {
            return Ok(vec![]);
        }
        let before = serde_json::to_string_pretty(&current)?;
        let after = serde_json::to_string_pretty(&merged)?;
        let diff = similar::TextDiff::from_lines(&before, &after)
            .unified_diff()
            .header(&self.target.display().to_string(), "merged")
            .to_string();
        Ok(vec![Change {
            summary: format!("merge keys into {}", self.target.display()),
            diff: Some(diff),
        }])
    }

    fn apply(&self, _policy: ConflictPolicy) -> Result<Applied> {
        let (_, merged, changed) = self.compute()?;
        if !changed {
            return Ok(Applied::Unchanged("keys already present".into()));
        }
        if let Some(parent) = self.target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.target, serde_json::to_string_pretty(&merged)? + "\n")?;
        Ok(Applied::Changed(format!("merged keys into {}", self.target.display())))
    }
}

// ---------------------------------------------------------------- TOML -----

pub struct TomlMergeStep {
    pub id: String,
    pub needs: Vec<String>,
    pub target: PathBuf,
    pub mode: MergeMode,
    pub value: toml::Value,
}

fn to_item(v: &toml::Value) -> toml_edit::Item {
    match v {
        toml::Value::String(s) => toml_edit::value(s.clone()),
        toml::Value::Integer(i) => toml_edit::value(*i),
        toml::Value::Float(f) => toml_edit::value(*f),
        toml::Value::Boolean(b) => toml_edit::value(*b),
        toml::Value::Datetime(d) => toml_edit::value(d.to_string()),
        toml::Value::Array(a) => {
            let mut arr = toml_edit::Array::new();
            for x in a {
                match to_item(x) {
                    toml_edit::Item::Value(val) => arr.push(val),
                    _ => {}
                }
            }
            toml_edit::value(arr)
        }
        toml::Value::Table(t) => {
            let mut table = toml_edit::Table::new();
            for (k, v) in t {
                table.insert(k, to_item(v));
            }
            toml_edit::Item::Table(table)
        }
    }
}

/// Compare an existing toml_edit item to a plain toml value.
fn item_eq(item: &toml_edit::Item, v: &toml::Value) -> bool {
    // Round-trip through string→toml for a structural (format-free) compare.
    let rendered = match item {
        toml_edit::Item::Value(val) => format!("x = {val}"),
        toml_edit::Item::Table(t) => format!("[x]\n{t}"),
        _ => return false,
    };
    let Ok(parsed) = rendered.parse::<toml::Table>() else { return false };
    parsed.get("x") == Some(v)
}

fn merge_toml(dst: &mut toml_edit::Table, src: &toml::map::Map<String, toml::Value>, seed: bool) -> bool {
    let mut changed = false;
    for (k, v) in src {
        match v {
            toml::Value::Table(sub) => {
                if dst.get(k).is_none() {
                    let mut t = toml_edit::Table::new();
                    t.set_implicit(true);
                    dst.insert(k, toml_edit::Item::Table(t));
                    changed = true;
                }
                match dst.get_mut(k).and_then(|i| i.as_table_mut()) {
                    Some(t) => changed |= merge_toml(t, sub, seed),
                    None => {
                        if !seed {
                            dst.insert(k, to_item(v));
                            changed = true;
                        }
                    }
                }
            }
            _ => match dst.get(k) {
                Some(existing) => {
                    if !seed && !item_eq(existing, v) {
                        dst.insert(k, to_item(v));
                        changed = true;
                    }
                }
                None => {
                    dst.insert(k, to_item(v));
                    changed = true;
                }
            },
        }
    }
    changed
}

impl TomlMergeStep {
    fn compute(&self) -> Result<(String, toml_edit::DocumentMut, bool)> {
        let raw = match fs::read_to_string(&self.target) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e).context("reading target")?,
        };
        let mut doc: toml_edit::DocumentMut = raw
            .parse()
            .with_context(|| format!("{} is not valid TOML", self.target.display()))?;
        let toml::Value::Table(src) = &self.value else {
            bail!("toml-merge value must be a table");
        };
        let changed = merge_toml(doc.as_table_mut(), src, self.mode == MergeMode::Seed);
        Ok((raw, doc, changed))
    }
}

impl Step for TomlMergeStep {
    fn id(&self) -> &str {
        &self.id
    }
    fn needs(&self) -> &[String] {
        &self.needs
    }
    fn resource(&self) -> Option<String> {
        Some(self.target.display().to_string())
    }

    fn check(&self) -> Result<Status> {
        Ok(if self.compute()?.2 {
            Status::Pending("keys missing or diverged".into())
        } else {
            Status::Satisfied
        })
    }

    fn plan(&self) -> Result<Vec<Change>> {
        let (before, doc, changed) = self.compute()?;
        if !changed {
            return Ok(vec![]);
        }
        let after = doc.to_string();
        let diff = similar::TextDiff::from_lines(&before, &after)
            .unified_diff()
            .header(&self.target.display().to_string(), "merged")
            .to_string();
        Ok(vec![Change {
            summary: format!("merge keys into {}", self.target.display()),
            diff: Some(diff),
        }])
    }

    fn apply(&self, _policy: ConflictPolicy) -> Result<Applied> {
        let (_, doc, changed) = self.compute()?;
        if !changed {
            return Ok(Applied::Unchanged("keys already present".into()));
        }
        if let Some(parent) = self.target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.target, doc.to_string())?;
        Ok(Applied::Changed(format!("merged keys into {}", self.target.display())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tval(s: &str) -> toml::Value {
        toml::Value::Table(s.parse::<toml::Table>().unwrap())
    }

    #[test]
    fn json_converge_enforces_and_preserves_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("settings.json");
        fs::write(&target, r#"{"theme":"dark","statusLine":{"type":"old"}}"#).unwrap();
        let s = JsonMergeStep {
            id: "t".into(),
            needs: vec![],
            target: target.clone(),
            mode: MergeMode::Converge,
            value: tval(r#"statusLine = { type = "command", padding = 0 }"#),
        };
        s.apply(ConflictPolicy::KeepLocal).unwrap();
        let out: serde_json::Value = serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(out["theme"], "dark");
        assert_eq!(out["statusLine"]["type"], "command");
        assert_eq!(out["statusLine"]["padding"], 0);
        assert_eq!(s.check().unwrap(), Status::Satisfied);
    }

    #[test]
    fn json_seed_preserves_user_values() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("settings.json");
        fs::write(&target, r#"{"ui":{"footer":{"items":["custom"]}}}"#).unwrap();
        let s = JsonMergeStep {
            id: "t".into(),
            needs: vec![],
            target: target.clone(),
            mode: MergeMode::Seed,
            value: tval(r#"ui = { footer = { items = ["a", "b"], showLabels = true } }"#),
        };
        s.apply(ConflictPolicy::KeepLocal).unwrap();
        let out: serde_json::Value = serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(out["ui"]["footer"]["items"][0], "custom"); // kept
        assert_eq!(out["ui"]["footer"]["showLabels"], true); // seeded
    }

    #[test]
    fn toml_seed_skips_existing_and_preserves_comments() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("config.toml");
        fs::write(&target, "# my precious comment\n[tui]\nstatus_line = [\"mine\"]\n").unwrap();
        let s = TomlMergeStep {
            id: "t".into(),
            needs: vec![],
            target: target.clone(),
            mode: MergeMode::Seed,
            value: tval(r#"tui = { status_line = ["a", "b"] }"#),
        };
        assert_eq!(s.check().unwrap(), Status::Satisfied); // nothing to seed
        s.apply(ConflictPolicy::KeepLocal).unwrap();
        let raw = fs::read_to_string(&target).unwrap();
        assert!(raw.contains("# my precious comment"));
        assert!(raw.contains("status_line = [\"mine\"]"));
    }

    #[test]
    fn toml_merge_creates_missing_keys() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("config.toml");
        fs::write(&target, "[projects]\nx = 1\n").unwrap();
        let s = TomlMergeStep {
            id: "t".into(),
            needs: vec![],
            target: target.clone(),
            mode: MergeMode::Seed,
            value: tval(r#"tui = { status_line = ["a"] }"#),
        };
        s.apply(ConflictPolicy::KeepLocal).unwrap();
        let raw = fs::read_to_string(&target).unwrap();
        assert!(raw.contains("status_line = [\"a\"]"));
        assert!(raw.contains("[projects]"));
        assert_eq!(s.check().unwrap(), Status::Satisfied);
    }
}
