use std::collections::VecDeque;

/// First in, first out.
pub struct Queue {
    items: VecDeque<u32>,
}

impl Queue {
    pub fn new() -> Self {
        Queue {
            items: VecDeque::new(),
        }
    }

    pub fn push(&mut self, item: u32) {
        self.items.push_back(item);
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}
