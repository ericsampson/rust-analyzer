fn main() {
    match 92 {
        0 ... 100 => (),
        101 ..= 200 => (),
        200 .. 301 => (),
        302 .. => (),
    }

    match Some(10 as u8) {
        Some(0) | None => (),
        Some(1..) => ()
    }
}
