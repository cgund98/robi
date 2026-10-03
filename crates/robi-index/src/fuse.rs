use crate::store::ChunkHit;

const RRF_K: f64 = 60.0;

/// Reciprocal rank fusion. Rank is 1-based in each list. A chunk in both
/// lists adds both terms. Ties break by path, then start line.
pub fn fuse(vector_hits: &[ChunkHit], fts_hits: &[ChunkHit]) -> Vec<ChunkHit> {
    let mut scores: Vec<(ChunkHit, f64)> = Vec::new();
    add_ranks(&mut scores, vector_hits);
    add_ranks(&mut scores, fts_hits);
    scores.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.path.cmp(&right.0.path))
            .then_with(|| left.0.start_line.cmp(&right.0.start_line))
    });
    scores
        .into_iter()
        .map(|(mut hit, score)| {
            hit.score = score;
            hit
        })
        .collect()
}

fn add_ranks(scores: &mut Vec<(ChunkHit, f64)>, hits: &[ChunkHit]) {
    for (index, hit) in hits.iter().enumerate() {
        let term = 1.0 / (RRF_K + (index as f64 + 1.0));
        if let Some(existing) = scores.iter_mut().find(|(item, _)| item.id == hit.id) {
            existing.1 += term;
        } else {
            scores.push((hit.clone(), term));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::ChunkKind;

    fn hit(id: i64, path: &str) -> ChunkHit {
        ChunkHit {
            id,
            path: path.to_owned(),
            start_line: 1,
            end_line: 2,
            symbol: String::new(),
            language: "rust".into(),
            kind: ChunkKind::Symbol,
            body: String::new(),
            score: 0.0,
        }
    }

    #[test]
    fn an_fts_only_hit_outranks_a_weaker_vector_hit() {
        let vector = vec![hit(1, "a.rs"), hit(2, "b.rs")];
        let fts = vec![hit(3, "c.rs")];
        let fused = fuse(&vector, &fts);
        assert_eq!(fused[0].id, 1);
        assert_eq!(fused[1].id, 3);
        let vector_second = 1.0 / 62.0;
        let fts_first = 1.0 / 61.0;
        assert!(fts_first > vector_second);
        assert!((fused[1].score - fts_first).abs() < 1e-9);
    }
}
