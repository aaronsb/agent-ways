/// Opens the store and keeps it open. In place: the citation sits in the
/// References block at the end of the doc comment.
///
/// References:
/// - ADR-101: the store stays open for the process lifetime.
/// - ADR-102 D1,
///   which sets the cache size this reads.
pub fn open() {}

/// Out of place: ADR-101 sits in the doc comment body.
pub fn close() {}

// A plain comment with no References block names ADR-102 in its body.
// References: ADR-101 on the same line does not open a block.
fn flush() {}

// The References line opens the block only for this comment.
// References:
// - ADR-101: the flush order.

// A new comment after a blank line: ADR-102 is out of place again.
fn sync() {
    let n = 1; // a trailing comment is its own comment: ADR-101
    let lifetime: &'static str = "plain"; // ADR-102 after a lifetime
    let c = '"'; // ADR-101 after a quote char literal
}
