use std::collections::HashMap;

use crate::models::{Activity, Digest, DigestStats, Period};

/// Build a `Digest` from a list of activities for a given period.
///
/// Computes `DigestStats` by counting activities grouped by source and by kind.
pub fn build_digest(activities: Vec<Activity>, period: Period) -> Digest {
    let total_activities = activities.len();
    let mut by_source: HashMap<String, usize> = HashMap::new();
    let mut by_kind: HashMap<String, usize> = HashMap::new();

    for activity in &activities {
        *by_source
            .entry(activity.source.to_string())
            .or_insert(0) += 1;
        *by_kind
            .entry(activity.kind.to_string())
            .or_insert(0) += 1;
    }

    let stats = DigestStats {
        total_activities,
        by_source,
        by_kind,
    };

    Digest {
        period,
        activities,
        stats,
        llm_summary: None,
    }
}
