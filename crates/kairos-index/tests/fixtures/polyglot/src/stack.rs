/// The capacity of a new stack.
pub const DEFAULT_CAPACITY: usize = 8;

/// Last in, first out.
pub struct Stack {
    items: Vec<u32>,
}

impl Stack {
    pub fn new() -> Self {
        Stack {
            items: Vec::with_capacity(DEFAULT_CAPACITY),
        }
    }

    pub fn push(&mut self, item: u32) {
        self.items.push(item);
    }

    pub fn pop(&mut self) -> Option<u32> {
        self.items.pop()
    }
}
