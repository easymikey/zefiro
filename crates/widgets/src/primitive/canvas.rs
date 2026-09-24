use ratatui::{buffer::Buffer, layout::Rect};

#[derive(Debug)]
pub struct Canvas<'a> {
    pub area: Rect,
    pub buffer: &'a mut Buffer,
}
