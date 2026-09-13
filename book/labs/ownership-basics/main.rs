use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Frame {
    data: Vec<u8>,
}

#[derive(Debug, PartialEq)]
enum ParseError {
    Empty,
}

fn parse_op(text: &str) -> Result<char, ParseError> {
    let op = text.trim().chars().next().ok_or(ParseError::Empty)?;
    Ok(op)
}

fn consume(frame: Frame) -> usize {
    frame.data.len()
}

fn needs_send<T: Send>(_: T) {}
fn needs_static<T: 'static>(_: T) {}

fn main() {
    let mut frame = Frame {
        data: vec![1, 2, 3],
    };
    let view = &frame.data;
    assert_eq!(view.len(), 3);
    // view is no longer used, so the following exclusive borrow is valid.
    let writable = &mut frame.data;
    writable.push(4);
    let mut copied = frame.clone();
    copied.data[0] = 9;
    assert_eq!(frame.data[0], 1);
    assert_eq!(consume(frame), 4);
    // consume(frame);
    assert_eq!(consume(copied), 4);

    needs_static(String::from("owned"));
    let local = String::from("borrowed");
    assert_eq!(local.as_str(), "borrowed");
    // needs_static(local.as_str());
    needs_send(Arc::new(1));
    // needs_send(std::rc::Rc::new(1));

    let counter = Arc::new(Mutex::new(0usize));
    let worker_counter = Arc::clone(&counter);
    assert!(Arc::ptr_eq(&counter, &worker_counter));
    let worker = std::thread::spawn(move || {
        let mut guard = worker_counter.lock().expect("counter lock poisoned");
        *guard += 1;
    });
    worker.join().expect("worker panicked");
    assert_eq!(*counter.lock().expect("counter lock poisoned"), 1);
    assert_eq!(Arc::strong_count(&counter), 1);

    assert_eq!(parse_op(" + "), Ok('+'));
    assert_eq!(parse_op(" "), Err(ParseError::Empty));
    // This parser takes the first character; it does not validate supported operators.
    assert_eq!(parse_op("unknown"), Ok('u'));
    println!("移动、借用、克隆、线程共享与错误分支：通过");
}
