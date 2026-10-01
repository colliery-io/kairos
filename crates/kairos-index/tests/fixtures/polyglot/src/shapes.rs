/// A shape with an area.
pub trait Shape {
    fn area(&self) -> f64;

    fn describe(&self) -> String {
        String::from("a shape")
    }
}

pub struct Square {
    pub side: f64,
}

impl Shape for Square {
    fn area(&self) -> f64 {
        self.side * self.side
    }
}
