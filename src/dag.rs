use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};

use crate::step::Step;

/// Topologically sort steps into parallel waves: every step in wave N depends
/// only on steps in waves < N. Manifest order breaks ties, so output is
/// deterministic. Unknown `needs` ids and cycles are hard errors.
pub fn waves(steps: &[Box<dyn Step>]) -> Result<Vec<Vec<usize>>> {
    let index: HashMap<&str, usize> = steps
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id(), i))
        .collect();
    if index.len() != steps.len() {
        bail!("duplicate step ids in manifest");
    }

    for s in steps.iter() {
        for need in s.needs() {
            if !index.contains_key(need.as_str()) {
                bail!("step '{}' needs unknown step '{}'", s.id(), need);
            }
        }
    }

    // Implicit dependencies: steps mutating the same resource (same target
    // file) are chained in manifest order so waves can never race on it.
    let mut extra_needs: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut last_for_resource: HashMap<String, usize> = HashMap::new();
    for (i, s) in steps.iter().enumerate() {
        if let Some(res) = s.resource() {
            if let Some(&prev) = last_for_resource.get(&res) {
                extra_needs.entry(i).or_default().push(prev);
            }
            last_for_resource.insert(res, i);
        }
    }

    let mut placed: HashSet<usize> = HashSet::new();
    let mut result: Vec<Vec<usize>> = Vec::new();
    while placed.len() < steps.len() {
        let mut wave: Vec<usize> = Vec::new();
        for (i, s) in steps.iter().enumerate() {
            if placed.contains(&i) {
                continue;
            }
            let explicit_ok = s.needs().iter().all(|n| placed.contains(&index[n.as_str()]));
            let implicit_ok = extra_needs
                .get(&i)
                .map(|deps| deps.iter().all(|d| placed.contains(d)))
                .unwrap_or(true);
            if explicit_ok && implicit_ok {
                wave.push(i);
            }
        }
        if wave.is_empty() {
            let stuck: Vec<&str> = steps
                .iter()
                .enumerate()
                .filter(|(i, _)| !placed.contains(i))
                .map(|(_, s)| s.id())
                .collect();
            bail!("dependency cycle among steps: {}", stuck.join(", "));
        }
        placed.extend(wave.iter().copied());
        result.push(wave);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::step::{Applied, Change, ConflictPolicy, Status, Step};
    use anyhow::Result;

    struct Fake {
        id: String,
        needs: Vec<String>,
        resource: Option<String>,
    }
    impl Step for Fake {
        fn id(&self) -> &str {
            &self.id
        }
        fn needs(&self) -> &[String] {
            &self.needs
        }
        fn resource(&self) -> Option<String> {
            self.resource.clone()
        }
        fn check(&self) -> Result<Status> {
            Ok(Status::Satisfied)
        }
        fn plan(&self) -> Result<Vec<Change>> {
            Ok(vec![])
        }
        fn apply(&self, _: ConflictPolicy) -> Result<Applied> {
            Ok(Applied::Unchanged(String::new()))
        }
    }

    fn fake(id: &str, needs: &[&str]) -> Box<dyn Step> {
        Box::new(Fake {
            id: id.into(),
            needs: needs.iter().map(|s| s.to_string()).collect(),
            resource: None,
        })
    }

    fn fake_res(id: &str, res: &str) -> Box<dyn Step> {
        Box::new(Fake {
            id: id.into(),
            needs: vec![],
            resource: Some(res.into()),
        })
    }

    #[test]
    fn waves_respect_needs() {
        let steps = vec![fake("a", &[]), fake("b", &["a"]), fake("c", &["a"]), fake("d", &["b", "c"])];
        let w = waves(&steps).unwrap();
        assert_eq!(w, vec![vec![0], vec![1, 2], vec![3]]);
    }

    #[test]
    fn cycle_is_error() {
        let steps = vec![fake("a", &["b"]), fake("b", &["a"])];
        assert!(waves(&steps).is_err());
    }

    #[test]
    fn unknown_need_is_error() {
        let steps = vec![fake("a", &["ghost"])];
        assert!(waves(&steps).is_err());
    }

    #[test]
    fn shared_resource_serializes_in_manifest_order() {
        // Regression: five block-in-file steps on ~/.zshrc once raced in one
        // wave and lost writes. Same-resource steps must land in distinct
        // waves, ordered as written.
        let steps = vec![
            fake_res("b1", "zshrc"),
            fake("other", &[]),
            fake_res("b2", "zshrc"),
            fake_res("b3", "zshrc"),
        ];
        let w = waves(&steps).unwrap();
        assert_eq!(w, vec![vec![0, 1], vec![2], vec![3]]);
    }
}
