//! Keeps the served catalog in sync with its directory, without a restart.
//!
//! A background thread polls the directory's [`catalog::fingerprint`]. A change is applied once
//! it has been stable for one polling interval, so a burst of writes (an editor saving several
//! files, `git pull`) triggers a single reload. Polling, rather than file system notifications,
//! also works on network file systems and Kubernetes ConfigMap volumes.

use std::path::PathBuf;
use std::sync::{Arc, PoisonError, RwLock};
use std::thread;
use std::time::Duration;

use crate::catalog::{self, Catalog, Problem};

/// What the pages are served from. Swapped as a whole, so readers never see a half-loaded
/// catalog.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub catalog: Arc<Catalog>,
    /// Problems of the files on disk. When not empty, the catalog is the last valid one.
    pub problems: Arc<Vec<Problem>>,
}

pub struct LiveCatalog {
    dir: PathBuf,
    current: RwLock<Snapshot>,
}

impl LiveCatalog {
    pub fn new(dir: PathBuf, catalog: Catalog) -> Self {
        Self {
            dir,
            current: RwLock::new(Snapshot {
                catalog: Arc::new(catalog),
                problems: Arc::new(Vec::new()),
            }),
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.current
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Loads the directory again. An invalid catalog keeps the previous one in use and
    /// records the problems; a valid one replaces it and clears them.
    pub fn reload(&self) -> Snapshot {
        let next = match catalog::load(&self.dir) {
            Ok(catalog) => Snapshot {
                catalog: Arc::new(catalog),
                problems: Arc::new(Vec::new()),
            },
            Err(problems) => Snapshot {
                catalog: self.snapshot().catalog,
                problems: Arc::new(problems),
            },
        };
        *self.current.write().unwrap_or_else(PoisonError::into_inner) = next.clone();
        next
    }
}

/// Polls the catalog directory every `interval` and reloads it after each settled change,
/// then calls `on_change` with the new snapshot.
///
/// `loaded` is the fingerprint of the served catalog, taken before it was loaded so that a
/// change made while loading is not missed.
pub fn spawn_watcher(
    live: Arc<LiveCatalog>,
    loaded: u64,
    interval: Duration,
    on_change: impl Fn(&Snapshot) + Send + 'static,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut detector = ChangeDetector::new(loaded);
        loop {
            thread::sleep(interval);
            if detector.observe(catalog::fingerprint(&live.dir)) {
                on_change(&live.reload());
            }
        }
    })
}

/// Debounces fingerprints: a new one is applied when observed twice in a row.
struct ChangeDetector {
    applied: u64,
    candidate: Option<u64>,
}

impl ChangeDetector {
    fn new(applied: u64) -> Self {
        Self {
            applied,
            candidate: None,
        }
    }

    /// Returns true when the files differ from the applied state and have not changed since
    /// the previous observation.
    fn observe(&mut self, fingerprint: u64) -> bool {
        if fingerprint == self.applied {
            self.candidate = None;
            return false;
        }
        if self.candidate == Some(fingerprint) {
            self.applied = fingerprint;
            self.candidate = None;
            return true;
        }
        self.candidate = Some(fingerprint);
        false
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::Mutex;
    use std::time::Instant;

    use tempfile::TempDir;

    use super::*;

    const INGREDIENTS: &str = "ingredients: [jambon, beurre]\n";
    const TAGS: &str = "tags: [rapide]\n";
    const RECIPE: &str = "recipe:\n  name: Jambon beurre\n  ingredients: [jambon, beurre]\n";

    fn catalog_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("ingredients.yml"), INGREDIENTS).unwrap();
        fs::write(dir.path().join("tags.yml"), TAGS).unwrap();
        fs::create_dir(dir.path().join("recipes")).unwrap();
        fs::write(dir.path().join("recipes/jambon-beurre.yml"), RECIPE).unwrap();
        dir
    }

    fn live(dir: &Path) -> LiveCatalog {
        LiveCatalog::new(dir.to_path_buf(), catalog::load(dir).unwrap())
    }

    fn add_recipe(dir: &Path, id: &str, name: &str) {
        let recipe = format!("recipe:\n  name: {name}\n  ingredients: [beurre]\n");
        fs::write(dir.join(format!("recipes/{id}.yml")), recipe).unwrap();
    }

    #[test]
    fn detector_applies_a_change_once_it_is_stable() {
        let mut detector = ChangeDetector::new(1);
        assert!(!detector.observe(1));
        assert!(!detector.observe(2), "first sighting waits");
        assert!(detector.observe(2), "stable: applied");
        assert!(!detector.observe(2), "already applied");
    }

    #[test]
    fn detector_waits_for_a_burst_to_settle() {
        let mut detector = ChangeDetector::new(1);
        assert!(!detector.observe(2));
        assert!(!detector.observe(3));
        assert!(!detector.observe(4));
        assert!(detector.observe(4));
    }

    #[test]
    fn detector_ignores_a_change_that_was_reverted() {
        let mut detector = ChangeDetector::new(1);
        assert!(!detector.observe(2));
        assert!(!detector.observe(1));
        assert!(!detector.observe(1));
    }

    #[test]
    fn valid_change_replaces_the_catalog() {
        let dir = catalog_dir();
        let live = live(dir.path());
        add_recipe(dir.path(), "tartine", "Tartine");
        let snapshot = live.reload();
        assert_eq!(snapshot.catalog.recipes.len(), 2);
        assert!(snapshot.problems.is_empty());
        assert_eq!(live.snapshot().catalog.recipes.len(), 2);
    }

    #[test]
    fn invalid_change_keeps_the_previous_catalog_until_fixed() {
        let dir = catalog_dir();
        let live = live(dir.path());
        let broken = dir.path().join("recipes/casse.yml");
        fs::write(
            &broken,
            "recipe:\n  name: Cassé\n  ingredients: [inconnu]\n",
        )
        .unwrap();

        let snapshot = live.reload();
        assert_eq!(snapshot.catalog.recipes.len(), 1);
        assert_eq!(snapshot.problems.len(), 1);
        assert_eq!(live.snapshot().problems.len(), 1);

        fs::remove_file(&broken).unwrap();
        add_recipe(dir.path(), "tartine", "Tartine");
        let snapshot = live.reload();
        assert_eq!(snapshot.catalog.recipes.len(), 2);
        assert!(snapshot.problems.is_empty());
    }

    #[test]
    fn watcher_reloads_after_a_change() {
        let dir = catalog_dir();
        let loaded = catalog::fingerprint(dir.path());
        let live = Arc::new(live(dir.path()));
        let changes = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&changes);
        spawn_watcher(
            Arc::clone(&live),
            loaded,
            Duration::from_millis(10),
            move |s| {
                seen.lock().unwrap().push(s.catalog.recipes.len());
            },
        );

        add_recipe(dir.path(), "tartine", "Tartine");
        let deadline = Instant::now() + Duration::from_secs(5);
        while changes.lock().unwrap().is_empty() {
            assert!(Instant::now() < deadline, "catalog not reloaded");
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(*changes.lock().unwrap(), [2]);
        assert_eq!(live.snapshot().catalog.recipes.len(), 2);
    }
}
