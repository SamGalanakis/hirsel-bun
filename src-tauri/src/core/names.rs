//! Name generators for Hirsel
//!
//! Generates memorable names for:
//! - Runs: adjective-animal format (curious-fox, swift-eagle)
//! - Workers: adjective-sheep-breed format (bonnie-cheviot, braw-merino)
//! - Evals: title-adjective-number format (inspector-keen-1)

use rand::prelude::IndexedRandom;
use rand::seq::SliceRandom;

// =============================================================================
// Run Names (adjective-animal format)
// =============================================================================

/// Adjectives for random run names
const RUN_ADJECTIVES: &[&str] = &[
    "curious", "swift", "bright", "calm", "bold", "eager", "gentle", "happy", "clever", "brave",
    "kind", "quick", "quiet", "wise", "warm", "keen", "noble", "merry", "fair", "steady", "agile",
    "witty", "lively", "earnest",
];

/// Animal nouns for random run names
const RUN_NOUNS: &[&str] = &[
    "fox", "eagle", "wolf", "owl", "bear", "hawk", "deer", "hare", "otter", "raven", "falcon",
    "lynx", "crane", "swan", "finch", "sparrow", "badger", "heron", "robin", "wren", "thrush",
    "lark", "dove", "jay",
];

/// Generate a random friendly run name like "curious-fox" or "swift-eagle"
///
/// Uses system time for pseudo-randomness to avoid requiring the full rand RNG.
pub fn generate_run_name() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Simple pseudo-random based on system time
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as usize;

    let adj_idx = seed % RUN_ADJECTIVES.len();
    let noun_idx = (seed / RUN_ADJECTIVES.len()) % RUN_NOUNS.len();

    format!("{}-{}", RUN_ADJECTIVES[adj_idx], RUN_NOUNS[noun_idx])
}

// =============================================================================
// Worker Names (adjective-sheep-breed format)
// =============================================================================

/// Nice-sounding sheep breed names (curated for memorability)
const BREEDS: &[&str] = &[
    // British/Scottish breeds
    "cheviot",
    "clun",
    "dorset",
    "herdwick",
    "jacob",
    "kerry",
    "lincoln",
    "lleyn",
    "lonk",
    "manx",
    "masham",
    "romney",
    "ryeland",
    "soay",
    "suffolk",
    "texel",
    "wensleydale",
    // European breeds
    "cormo",
    "finn",
    "gotland",
    "gute",
    "lacaune",
    "merino",
    "romanov",
    "skudde",
    "valais",
    // Other nice-sounding breeds
    "oxford",
    "panama",
    "targhee",
    "tunis",
    "columbia",
    "polwarth",
    "rambouillet",
    "corriedale",
    "coopworth",
    "perendale",
];

/// Pleasant adjectives (many Gaelic/Scottish inspired)
const ADJECTIVES: &[&str] = &[
    // Scottish/Gaelic inspired
    "braw",   // Scottish: fine, good
    "bonnie", // Scottish: beautiful
    "canny",  // Scottish: clever, careful
    "blithe", // cheerful, carefree
    "dour",   // Scottish: stern, severe (but sounds cool)
    // Nature/pastoral
    "misty", "heather", "bramble", "meadow", "glen", "moor", "fern", "moss", "briar", "thistle",
    "rowan", "aspen", "willow", "hazel", // Pleasant qualities
    "swift", "nimble", "keen", "fleet", "hale", "brave", "true", "fair", "bold", "wise", "gentle",
    "steady", "quiet", "bright", "warm", "calm", "kind",
];

/// Generate a random worker name (adjective-breed format)
pub fn generate_worker_name() -> String {
    let mut rng = rand::rng();
    let adj = ADJECTIVES.choose(&mut rng).unwrap_or(&"swift");
    let breed = BREEDS.choose(&mut rng).unwrap_or(&"cheviot");
    format!("{}-{}", adj, breed)
}

/// Generate a worker name with a specific index suffix
pub fn generate_worker_name_indexed(index: usize) -> String {
    let mut rng = rand::rng();
    let adj = ADJECTIVES.choose(&mut rng).unwrap_or(&"swift");
    let breed = BREEDS.choose(&mut rng).unwrap_or(&"cheviot");
    format!("{}-{}-{}", adj, breed, index)
}

/// Generate a deterministic name based on a seed (for consistent naming)
pub fn generate_worker_name_seeded(seed: u64) -> String {
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let adj = ADJECTIVES.choose(&mut rng).unwrap_or(&"swift");
    let breed = BREEDS.choose(&mut rng).unwrap_or(&"cheviot");
    format!("{}-{}", adj, breed)
}

/// Get all available breed names
pub fn get_breeds() -> &'static [&'static str] {
    BREEDS
}

/// Get all available adjectives
pub fn get_adjectives() -> &'static [&'static str] {
    ADJECTIVES
}

// =============================================================================
// Eval Agent Names (Detective/QA themed)
// =============================================================================

/// QA/Detective titles for eval agents
const EVAL_TITLES: &[&str] = &[
    "inspector",
    "detective",
    "auditor",
    "examiner",
    "analyst",
    "reviewer",
    "checker",
    "verifier",
];

/// Adjectives for eval agents (subset that sounds "inspector-like")
/// These are distinct from worker adjectives to avoid name collisions
const EVAL_ADJECTIVES: &[&str] = &[
    "keen",
    "sharp",
    "careful",
    "diligent",
    "thorough",
    "vigilant",
    "watchful",
    "precise",
    "astute",
    "meticulous",
];

/// Generate an eval agent name with sequential number
/// Format: title-adjective-N (e.g., "inspector-keen-1", "detective-sharp-2")
pub fn generate_eval_name(eval_number: usize) -> String {
    let mut rng = rand::rng();
    let title = EVAL_TITLES.choose(&mut rng).unwrap_or(&"inspector");
    let adj = EVAL_ADJECTIVES.choose(&mut rng).unwrap_or(&"keen");
    format!("{}-{}-{}", title, adj, eval_number)
}

/// Generate a deterministic eval name based on eval ID
pub fn generate_eval_name_seeded(eval_id: u64, eval_number: usize) -> String {
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(eval_id);
    let title = EVAL_TITLES.choose(&mut rng).unwrap_or(&"inspector");
    let adj = EVAL_ADJECTIVES.choose(&mut rng).unwrap_or(&"keen");
    format!("{}-{}-{}", title, adj, eval_number)
}

/// Generate multiple unique names
pub fn generate_unique_names(count: usize) -> Vec<String> {
    let mut names = Vec::with_capacity(count);
    let mut rng = rand::rng();

    // Shuffle both lists
    let mut adjectives: Vec<&str> = ADJECTIVES.to_vec();
    let mut breeds: Vec<&str> = BREEDS.to_vec();
    adjectives.shuffle(&mut rng);
    breeds.shuffle(&mut rng);

    // Generate combinations
    for i in 0..count {
        let adj = adjectives[i % adjectives.len()];
        let breed = breeds[i % breeds.len()];
        names.push(format!("{}-{}", adj, breed));
    }

    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_run_name() {
        let name = generate_run_name();
        assert!(name.contains('-'));
        let parts: Vec<&str> = name.split('-').collect();
        assert_eq!(parts.len(), 2);
        // Verify parts are from our word lists
        assert!(RUN_ADJECTIVES.contains(&parts[0]));
        assert!(RUN_NOUNS.contains(&parts[1]));
    }

    #[test]
    fn test_generate_worker_name() {
        let name = generate_worker_name();
        assert!(name.contains('-'));
        let parts: Vec<&str> = name.split('-').collect();
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn test_generate_unique_names() {
        let names = generate_unique_names(10);
        assert_eq!(names.len(), 10);
        // Check all unique
        let mut seen = std::collections::HashSet::new();
        for name in &names {
            assert!(seen.insert(name.clone()));
        }
    }

    #[test]
    fn test_seeded_deterministic() {
        let name1 = generate_worker_name_seeded(12345);
        let name2 = generate_worker_name_seeded(12345);
        assert_eq!(name1, name2);
    }
}
