//! The Rust part of the polyglot fixture.

pub mod labels;
pub mod queue;
pub mod shapes;
pub mod stack;

use queue::Queue;
use shapes::Shape;

/// Pushes each item onto a new queue. It calls `Queue::push`, not `Stack::push`.
pub fn enqueue_all(items: &[u32]) -> Queue {
    let mut queue = Queue::new();
    for item in items {
        queue.push(*item);
    }
    queue
}

/// Calls a trait method on a generic type.
pub fn describe_all<S: Shape>(shapes: &[S]) -> Vec<String> {
    shapes.iter().map(|shape| shape.describe()).collect()
}

/// Calls a trait method with no body on a generic type.
pub fn total_area<S: Shape>(shapes: &[S]) -> f64 {
    shapes.iter().map(|shape| shape.area()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_all_keeps_each_item() {
        let queue = enqueue_all(&[1, 2]);
        assert_eq!(queue.len(), 2);
    }
}

// The 2 modules of the repeated-code scenarios (COLLIERY-T-1857). They are
// at the end, so that the lines above do not move.
pub mod checksum;
pub mod legacy;
