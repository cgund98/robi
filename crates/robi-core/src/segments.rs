//! Partitioning a turn's calls into execution segments.
//!
//! A turn's calls run in model order. Concurrent calls run together, bounded by a
//! semaphore; an exclusive call runs alone. Two passes over the turn — all
//! concurrent calls, then all exclusive — would reorder execution against model
//! order, so a read meant to verify a write could run before that write while the
//! transcript still read correctly. This walk keeps the two orders equal.

/// A maximal run of calls that may overlap, or one call that may not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment<T> {
    /// Run together, bounded by the in-flight limit.
    Batch(Vec<T>),
    /// Run alone. Nothing is in flight when it starts, and nothing starts until
    /// it finishes.
    Solo(T),
}

/// Split `items` into segments, in the order given.
///
/// `is_exclusive` decides, per item, whether it must run alone. Each exclusive
/// item flushes the open batch and becomes its own solo segment, which is what
/// makes it a barrier.
pub fn segment_by<T, F>(items: Vec<T>, mut is_exclusive: F) -> Vec<Segment<T>>
where
    F: FnMut(&T) -> bool,
{
    let mut segments = Vec::new();
    let mut batch = Vec::new();

    for item in items {
        if is_exclusive(&item) {
            if !batch.is_empty() {
                segments.push(Segment::Batch(std::mem::take(&mut batch)));
            }
            segments.push(Segment::Solo(item));
        } else {
            batch.push(item);
        }
    }

    if !batch.is_empty() {
        segments.push(Segment::Batch(batch));
    }

    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments_of(items: &[i32], exclusive: &[i32]) -> Vec<Segment<i32>> {
        segment_by(items.to_vec(), |item| exclusive.contains(item))
    }

    #[test]
    fn a_turn_with_no_exclusive_calls_is_one_batch() {
        assert_eq!(
            segments_of(&[1, 2, 3], &[]),
            vec![Segment::Batch(vec![1, 2, 3])]
        );
    }

    #[test]
    fn an_exclusive_call_between_reads_splits_around_it() {
        // [read A, write B, read C] must run A, then B, then C.
        assert_eq!(
            segments_of(&[1, 2, 3], &[2]),
            vec![
                Segment::Batch(vec![1]),
                Segment::Solo(2),
                Segment::Batch(vec![3]),
            ]
        );
    }

    #[test]
    fn reads_before_an_exclusive_call_still_batch() {
        // [read A, read C, write B] keeps A and C together.
        assert_eq!(
            segments_of(&[1, 2, 3], &[3]),
            vec![Segment::Batch(vec![1, 2]), Segment::Solo(3)]
        );
    }

    #[test]
    fn consecutive_exclusive_calls_each_run_alone() {
        assert_eq!(
            segments_of(&[1, 2], &[1, 2]),
            vec![Segment::Solo(1), Segment::Solo(2)]
        );
    }

    #[test]
    fn a_leading_exclusive_call_leaves_no_empty_batch() {
        assert_eq!(
            segments_of(&[1, 2], &[1]),
            vec![Segment::Solo(1), Segment::Batch(vec![2])]
        );
    }

    #[test]
    fn segments_preserve_the_given_order() {
        let flat: Vec<i32> = segments_of(&[1, 2, 3, 4, 5], &[2, 4])
            .into_iter()
            .flat_map(|seg| match seg {
                Segment::Batch(items) => items,
                Segment::Solo(item) => vec![item],
            })
            .collect();
        assert_eq!(flat, vec![1, 2, 3, 4, 5]);
    }
}
