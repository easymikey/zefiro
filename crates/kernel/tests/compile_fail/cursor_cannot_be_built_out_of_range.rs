use kernel::domain::Cursor;

fn main() {
    let cursor = Cursor { index: 9, len: 0 };
    assert!(cursor.is_empty());
}
