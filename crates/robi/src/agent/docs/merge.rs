//! Three-way merge of a base, the file on disk, and the client's buffer.
//!
//! The editor's save names a base version. When the disk has moved on since
//! then, both the agent and the user changed the same file. `merge3` combines
//! the two line diffs against the common base. Hunks that do not overlap both
//! apply; where they overlap, the client wins (last edit).

use crate::agent::review::{diff, split_lines, Hunk};

/// Combine `disk` and `client`, both diffs against `base`.
///
/// On an overlapping hunk the client's version wins. When one side did not
/// change, the other side is returned unchanged.
pub fn merge3(base: &str, disk: &str, client: &str) -> String {
    if disk == client {
        return disk.to_owned();
    }
    if client == base {
        return disk.to_owned();
    }
    if disk == base {
        return client.to_owned();
    }

    let base_lines = split_lines(base);
    let disk_hunks = diff("", base, disk).hunks;
    let client_hunks = diff("", base, client).hunks;

    let mut out: Vec<String> = Vec::with_capacity(base_lines.len());
    let mut emitted = 0usize;
    let mut di = 0usize;
    let mut ci = 0usize;

    while di < disk_hunks.len() || ci < client_hunks.len() {
        let next_disk = disk_hunks.get(di).map(|hunk| hunk.old_start);
        let next_client = client_hunks.get(ci).map(|hunk| hunk.old_start);
        let start = match (next_disk, next_client) {
            (Some(disk), Some(client)) => disk.min(client),
            (Some(disk), None) => disk,
            (None, Some(client)) => client,
            (None, None) => break,
        };
        while emitted < start {
            out.push(base_lines[emitted].clone());
            emitted += 1;
        }

        let mut cluster_disk: Vec<Hunk> = Vec::new();
        let mut cluster_client: Vec<Hunk> = Vec::new();
        let mut cluster = Foot { start, len: 0 };

        // Seed with the hunk that starts first; the client wins a tie.
        let take_client = match (next_disk, next_client) {
            (Some(disk), Some(client)) => client <= disk,
            (None, Some(_)) => true,
            _ => false,
        };
        if take_client {
            let hunk = client_hunks[ci].clone();
            cluster.len = hunk.old_start + hunk.old.len() - start;
            cluster_client.push(hunk);
            ci += 1;
        } else {
            let hunk = disk_hunks[di].clone();
            cluster.len = hunk.old_start + hunk.old.len() - start;
            cluster_disk.push(hunk);
            di += 1;
        }

        // Expand the cluster while either side has a hunk that touches it.
        loop {
            let mut advanced = false;
            if let Some(hunk) = disk_hunks.get(di) {
                let foot = Foot {
                    start: hunk.old_start,
                    len: hunk.old.len(),
                };
                if overlaps(foot, cluster) {
                    cluster.len = cluster.end().max(foot.end()) - cluster.start;
                    cluster_disk.push(hunk.clone());
                    di += 1;
                    advanced = true;
                }
            }
            if let Some(hunk) = client_hunks.get(ci) {
                let foot = Foot {
                    start: hunk.old_start,
                    len: hunk.old.len(),
                };
                if overlaps(foot, cluster) {
                    cluster.len = cluster.end().max(foot.end()) - cluster.start;
                    cluster_client.push(hunk.clone());
                    ci += 1;
                    advanced = true;
                }
            }
            if !advanced {
                break;
            }
        }

        let region = &base_lines[cluster.start..cluster.end()];
        let chosen = if cluster_client.is_empty() {
            &cluster_disk
        } else {
            &cluster_client
        };
        out.extend(region_lines(region, cluster.start, chosen));
        emitted = cluster.end();
    }

    while emitted < base_lines.len() {
        out.push(base_lines[emitted].clone());
        emitted += 1;
    }

    join_lines(&out, base)
}

/// A hunk's footprint in the base, in line indexes.
#[derive(Debug, Clone, Copy)]
struct Foot {
    start: usize,
    len: usize,
}

impl Foot {
    fn end(self) -> usize {
        self.start + self.len
    }
}

/// Whether two footprints conflict in the base.
///
/// A replace conflicts with anything its half-open interval `[start, end)`
/// intersects. An insert has no width: it conflicts with another insert at the
/// same point, and with a replace whose interval strictly contains its point.
/// Touching a replace's boundary — inserting before its first line or after its
/// last — is compatible, because the two compose in an unambiguous order.
fn overlaps(hunk: Foot, cluster: Foot) -> bool {
    if hunk.len == 0 && cluster.len == 0 {
        return hunk.start == cluster.start;
    }
    if hunk.len == 0 {
        return hunk.start > cluster.start && hunk.start < cluster.end();
    }
    if cluster.len == 0 {
        return cluster.start > hunk.start && cluster.start < hunk.end();
    }
    hunk.start < cluster.end() && hunk.end() > cluster.start
}

/// Apply `hunks` to the base `region` and return its lines.
///
/// `region` is the slice `base_lines[region_start..]` for the cluster. Hunks
/// are ordered and non-overlapping; `region` spans all of them.
fn region_lines(region: &[String], region_start: usize, hunks: &[Hunk]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = region_start;
    for hunk in hunks {
        let start = hunk.old_start;
        let end = start + hunk.old.len();
        for index in cursor..start {
            if let Some(line) = region.get(index - region_start) {
                out.push(line.clone());
            }
        }
        out.extend(hunk.new.iter().cloned());
        cursor = end;
    }
    let region_end = region_start + region.len();
    for index in cursor..region_end {
        if let Some(line) = region.get(index - region_start) {
            out.push(line.clone());
        }
    }
    out
}

/// Join lines with the base's newline style, keeping its trailing newline.
fn join_lines(lines: &[String], prototype: &str) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let ending = if prototype.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut out = lines.join(ending);
    if prototype.is_empty() || prototype.ends_with('\n') {
        out.push_str(ending);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disjoint_changes_both_apply() {
        let base = "a\nb\nc\nd\n";
        let disk = "A\nb\nc\nd\n";
        let client = "a\nb\nC\nd\n";
        assert_eq!(merge3(base, disk, client), "A\nb\nC\nd\n");
    }

    #[test]
    fn an_overlapping_change_goes_to_the_client() {
        let base = "a\nb\nc\n";
        let disk = "a\nDISK\nc\n";
        let client = "a\nCLIENT\nc\n";
        assert_eq!(merge3(base, disk, client), "a\nCLIENT\nc\n");
    }

    #[test]
    fn one_side_unchanged_takes_the_other() {
        let base = "a\nb\n";
        assert_eq!(merge3(base, "a\nB\n", base), "a\nB\n");
        assert_eq!(merge3(base, base, "a\nC\n"), "a\nC\n");
    }

    #[test]
    fn adjacent_replaces_do_not_conflict() {
        let base = "one\ntwo\n";
        let disk = "ONE\ntwo\n";
        let client = "one\nTWO\n";
        assert_eq!(merge3(base, disk, client), "ONE\nTWO\n");
    }

    #[test]
    fn an_insert_adjacent_to_a_disk_delete_survives() {
        let base = "a\nb\nc\n";
        // Disk deletes "b"; client inserts "!" before "c".
        let disk = "a\nc\n";
        let client = "a\nb\n!\nc\n";
        // The insert's point is the delete's end boundary: "after b, before c".
        // The two compose, so both land and "b" stays deleted.
        assert_eq!(merge3(base, disk, client), "a\n!\nc\n");
    }

    #[test]
    fn an_insert_inside_a_replaced_line_goes_to_the_client() {
        let base = "a\nmid\nb\n";
        // Disk rewrites the middle line; the client typed inside it.
        let disk = "a\nMID\nb\n";
        let client = "a\nmid-edited\nb\n";
        // The client's edit covers the whole replaced line, so it wins.
        assert_eq!(merge3(base, disk, client), "a\nmid-edited\nb\n");
    }

    #[test]
    fn an_insert_into_the_agents_deleted_anchor_goes_to_the_client() {
        let base = "keep\nanchor\nkeep\n";
        // The agent deleted the anchor line; the user typed into it.
        let disk = "keep\nkeep\n";
        let client = "keep\nanchor edited\nkeep\n";
        assert_eq!(merge3(base, disk, client), "keep\nanchor edited\nkeep\n");
    }

    #[test]
    fn a_whole_file_replace_goes_to_the_client() {
        let base = "a\nb\nc\n";
        let disk = "x\ny\nz\n";
        let client = "1\n2\n3\n";
        assert_eq!(merge3(base, disk, client), "1\n2\n3\n");
    }

    #[test]
    fn a_client_that_matches_the_base_takes_the_disk() {
        let base = "a\nb\n";
        let disk = "a\nB\nC\n";
        assert_eq!(merge3(base, disk, base), "a\nB\nC\n");
    }

    #[test]
    fn a_trailing_newline_is_preserved() {
        let base = "a\nb\n";
        let disk = "a\nb\nc\n";
        let client = "a\nb\n";
        assert!(merge3(base, disk, client).ends_with('\n'));
    }
}
