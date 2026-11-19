//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/tree/v3.3.1/YarnSpinner/Saliency>
//!
//! Saliency strategies decide which item of a line group (`=>`) or which member of a node group (`when:` headers) runs.
//!
//! ## Implementation notes
//!
//! The C# strategies own a reference to the variable storage. Here, the storage is passed to each call instead,
//! so strategies are plain values that can be configured before a [`Dialogue`] exists.

use crate::prelude::*;
use core::fmt::Debug;
use rand::{RngExt as _, SeedableRng, rngs::SmallRng, rngs::SysRng};

/// The kind of content a [`ContentSaliencyOption`] represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "bevy", reflect(Debug, PartialEq, Hash))]
#[cfg_attr(
    all(feature = "bevy", feature = "serde"),
    reflect(Serialize, Deserialize)
)]
pub enum ContentSaliencyContentType {
    /// A node in a node group.
    Node,
    /// A line in a line group.
    Line,
}

/// A piece of content that a [`ContentSaliencyStrategy`] can choose.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "bevy", reflect(Debug, PartialEq, Hash))]
#[cfg_attr(
    all(feature = "bevy", feature = "serde"),
    reflect(Serialize, Deserialize)
)]
pub struct ContentSaliencyOption {
    /// Uniquely identifies the content: the line ID for lines, the node name for nodes.
    pub content_id: String,
    /// The number of conditions of this content that passed.
    pub passing_condition_value_count: usize,
    /// The number of conditions of this content that failed. Content with failing conditions must not be selected.
    pub failing_condition_value_count: usize,
    /// How specific the conditions of this content are: the number of variables, functions and boolean
    /// operators in them. Higher scores are more specific.
    pub complexity_score: i32,
    /// The kind of content this is.
    pub content_type: ContentSaliencyContentType,
    /// The instruction to jump to when this content is selected.
    pub(crate) destination: usize,
}

impl ContentSaliencyOption {
    /// Creates an option for the content with the given ID that has no conditions.
    pub fn new(content_id: impl Into<String>, content_type: ContentSaliencyContentType) -> Self {
        Self {
            content_id: content_id.into(),
            passing_condition_value_count: 0,
            failing_condition_value_count: 0,
            complexity_score: 0,
            content_type,
            destination: 0,
        }
    }

    /// The name of the variable in which the built-in strategies count how often this content was selected.
    pub fn view_count_key(&self) -> String {
        format!("$Yarn.Internal.Content.ViewCount.{}", self.content_id)
    }
}

/// Decides which content of a line group or node group to run.
/// Set it with [`Dialogue::set_content_saliency_strategy`].
pub trait ContentSaliencyStrategy: Debug + Send + Sync {
    /// Chooses an item from `content`, or [`None`] if nothing should run.
    /// Must return one of the passed items (compared by [`ContentSaliencyOption::content_id`]).
    ///
    /// This may be called to query content without running it, so it must not change any state.
    fn query_best_content(
        &self,
        content: &[ContentSaliencyOption],
        variable_storage: &dyn VariableStorage,
    ) -> Option<ContentSaliencyOption>;

    /// Called when the content returned by [`ContentSaliencyStrategy::query_best_content`] is about to run.
    /// Strategies use this to record what was seen.
    fn content_was_selected(
        &mut self,
        content: &ContentSaliencyOption,
        variable_storage: &mut dyn VariableStorage,
    );

    /// Clones this strategy into a box.
    fn clone_box(&self) -> Box<dyn ContentSaliencyStrategy>;
}

impl Clone for Box<dyn ContentSaliencyStrategy> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// Selects the first item whose conditions all pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FirstSaliencyStrategy;

/// Selects the item with the highest complexity whose conditions all pass. Ties go to the earliest item.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct BestSaliencyStrategy;

/// Selects the least recently viewed item whose conditions all pass, preferring higher complexity.
/// Ties go to the earliest item. Tracks view counts in the variable storage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct BestLeastRecentlyViewedSaliencyStrategy;

/// Like [`BestLeastRecentlyViewedSaliencyStrategy`], but ties are broken randomly.
/// This is the default strategy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct RandomBestLeastRecentlyViewedSaliencyStrategy;

fn passing(
    content: &[ContentSaliencyOption],
) -> impl DoubleEndedIterator<Item = &ContentSaliencyOption> {
    content
        .iter()
        .filter(|c| c.failing_condition_value_count == 0)
}

fn view_count(content: &ContentSaliencyOption, variable_storage: &dyn VariableStorage) -> i32 {
    match variable_storage.get(&content.view_count_key()) {
        Ok(YarnValue::Number(count)) => count as i32,
        _ => 0,
    }
}

fn increment_view_count(
    content: &ContentSaliencyOption,
    variable_storage: &mut dyn VariableStorage,
) {
    let count = view_count(content, variable_storage) + 1;
    if let Err(e) = variable_storage.set(content.view_count_key(), (count as f32).into()) {
        log::error!("Failed to store the view count of {}: {e}", content.content_id);
    }
}

/// The candidates with the lowest view count and, among those, the highest complexity. Keeps the input order.
fn best_least_recently_viewed<'a>(
    content: &'a [ContentSaliencyOption],
    variable_storage: &dyn VariableStorage,
) -> Vec<&'a ContentSaliencyOption> {
    let candidates: Vec<_> = passing(content)
        .map(|c| (view_count(c, variable_storage), c))
        .collect();
    let Some(best) = candidates
        .iter()
        .map(|(count, c)| (*count, -c.complexity_score))
        .min()
    else {
        return Vec::new();
    };
    candidates
        .into_iter()
        .filter(|(count, c)| (*count, -c.complexity_score) == best)
        .map(|(_, c)| c)
        .collect()
}

macro_rules! impl_strategy {
    ($strategy:ty, tracks_views: $tracks_views:expr, |$content:ident, $storage:ident| $query:expr) => {
        impl ContentSaliencyStrategy for $strategy {
            fn query_best_content(
                &self,
                $content: &[ContentSaliencyOption],
                $storage: &dyn VariableStorage,
            ) -> Option<ContentSaliencyOption> {
                $query
            }

            fn content_was_selected(
                &mut self,
                content: &ContentSaliencyOption,
                variable_storage: &mut dyn VariableStorage,
            ) {
                if $tracks_views {
                    increment_view_count(content, variable_storage);
                }
            }

            fn clone_box(&self) -> Box<dyn ContentSaliencyStrategy> {
                Box::new(*self)
            }
        }
    };
}

impl_strategy!(FirstSaliencyStrategy, tracks_views: false, |content, _storage| {
    passing(content).next().cloned()
});

impl_strategy!(BestSaliencyStrategy, tracks_views: false, |content, _storage| {
    // `max_by_key` returns the last maximum, but ties must go to the first one.
    passing(content).rev().max_by_key(|c| c.complexity_score).cloned()
});

impl_strategy!(BestLeastRecentlyViewedSaliencyStrategy, tracks_views: true, |content, storage| {
    best_least_recently_viewed(content, storage).first().cloned().cloned()
});

impl_strategy!(RandomBestLeastRecentlyViewedSaliencyStrategy, tracks_views: true, |content, storage| {
    let best = best_least_recently_viewed(content, storage);
    match best.len() {
        0 => None,
        1 => Some(best[0].clone()),
        len => {
            let index = SmallRng::try_from_rng(&mut SysRng).unwrap().random_range(0..len);
            Some(best[index].clone())
        }
    }
});

#[cfg(test)]
mod tests {
    use super::*;

    fn option(id: &str, complexity: i32, failing: usize) -> ContentSaliencyOption {
        ContentSaliencyOption {
            complexity_score: complexity,
            failing_condition_value_count: failing,
            ..ContentSaliencyOption::new(id, ContentSaliencyContentType::Line)
        }
    }

    #[test]
    fn built_in_strategies_select_expected_content() {
        let mut storage = MemoryVariableStorage::new();
        let content = [
            option("failing", 9, 1),
            option("a", 1, 0),
            option("b", 2, 0),
            option("c", 2, 0),
        ];
        let id = |o: Option<ContentSaliencyOption>| o.unwrap().content_id;

        assert_eq!("a", id(FirstSaliencyStrategy.query_best_content(&content, &storage)));
        assert_eq!("b", id(BestSaliencyStrategy.query_best_content(&content, &storage)));
        assert!(BestSaliencyStrategy.query_best_content(&content[..1], &storage).is_none());

        // Least recently viewed: b, then c (same complexity, fewer views), then a, then b again
        let mut strategy = BestLeastRecentlyViewedSaliencyStrategy;
        let mut seen = Vec::new();
        for _ in 0..4 {
            let selected = strategy.query_best_content(&content, &storage).unwrap();
            strategy.content_was_selected(&selected, &mut storage);
            seen.push(selected.content_id);
        }
        assert_eq!(vec!["b", "c", "a", "b"], seen);

        storage.clear();
        let random = RandomBestLeastRecentlyViewedSaliencyStrategy;
        let selected = id(random.query_best_content(&content, &storage));
        assert!(selected == "b" || selected == "c");
    }
}
