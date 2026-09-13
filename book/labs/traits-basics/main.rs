use std::any::Any;

fn maximum<T: PartialOrd>(left: T, right: T) -> T {
    if left > right {
        left
    } else {
        right
    }
}

struct Slot<T> {
    value: T,
}

trait Area {
    fn area(&self) -> f64;
    fn describe(&self) -> String {
        format!("area = {}", self.area())
    }
    // This method is only callable when the concrete Self type is known.
    fn repeat<T>(&self, value: T) -> T
    where
        Self: Sized,
    {
        value
    }
}

struct Circle {
    radius: f64,
}
struct Square {
    side: f64,
}

impl Area for Circle {
    fn area(&self) -> f64 {
        std::f64::consts::PI * self.radius * self.radius
    }
}

impl Area for Square {
    fn area(&self) -> f64 {
        self.side * self.side
    }
}

fn main() {
    assert_eq!(maximum(2i32, 7i32), 7);
    assert_eq!(maximum("apple", "pear"), "pear");
    let slot = Slot {
        value: String::from("frame"),
    };
    assert_eq!(slot.value, "frame");
    // PartialOrd permits incomparable values: this helper does not define a NaN policy.
    assert!(maximum(1.0f64, f64::NAN).is_nan());

    let square = Square { side: 2.0 };
    assert_eq!(square.repeat(9u32), 9);
    let borrowed: &dyn Area = &square;
    assert_eq!(borrowed.area(), 4.0);
    assert_eq!(borrowed.describe(), "area = 4");
    // borrowed.repeat(9u32);

    let shapes: Vec<Box<dyn Area>> = vec![Box::new(Circle { radius: 1.0 }), Box::new(square)];
    assert!((shapes[0].area() - std::f64::consts::PI).abs() < 1e-12);
    assert_eq!(shapes[1].area(), 4.0);

    let mut erased: Box<dyn Any> = Box::new(String::from("frame"));
    assert!(erased.downcast_ref::<u32>().is_none());
    erased.downcast_mut::<String>().unwrap().push_str("-7");
    let erased = match erased.downcast::<u32>() {
        Ok(_) => panic!("String cannot become u32 by downcasting"),
        Err(original) => original,
    };
    let recovered = erased.downcast::<String>().unwrap();
    assert_eq!(*recovered, "frame-7");
    println!("泛型、借用与装箱 trait 对象、Any 借用及所有权恢复：通过");
}
