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

    let mut placed: HashSet<usize> = HashSet::new();
    let mut result: Vec<Vec<usize>> = Vec::new();
    while placed.len() < steps.len() {
        let mut wave: Vec<usize> = Vec::new();
        for (i, s) in steps.iter().enumerate() {
            if placed.contains(&i) {
                continue;
            }
            if s.needs().iter().all(|n| placed.contains(&index[n.as_str()])) {
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
    use crate::step::{Change, ConflictPolicy, Status, Step};
    use anyhow::Result;

    struct Fake {
        id: String,
        needs: Vec<String>,
    }
    impl Step for Fake {
        fn id(&self) -> &str {
            &self.id
        }
        fn needs(&self) -> &[String] {
            &self.needs
        }
        fn check(&self) -> Result<Status> {
            Ok(Status::Satisfied)
        }
        fn plan(&self) -> Result<Vec<Change>> {
            Ok(vec![])
        }
        fn apply(&self, _: ConflictPolicy) -> Result<()> {
            Ok(())
        }
    }

    fn fake(id: &str, needs: &[&str]) -> Box<dyn Step> {
        Box::new(Fake {
            id: id.into(),
            needs: needs.iter().map(|s| s.to_string()).collect(),
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
}
