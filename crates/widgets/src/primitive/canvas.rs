use ratatui::{buffer::Buffer, layout::Rect};

#[derive(Debug)]
pub(crate) struct Canvas<'a> {
    pub(crate) area: Rect,
    pub(crate) buffer: &'a mut Buffer,
}

#[cfg(test)]
pub(crate) fn find_text(buffer: &Buffer, needle: &str) -> Option<(u16, u16)> {
    let wanted: Vec<char> = needle.chars().collect();
    let width = wanted.len();
    if width == 0 {
        return None;
    }
    for y in 0..buffer.area.height {
        let symbols: Vec<&str> = (0..buffer.area.width)
            .filter_map(|x| buffer.cell((x, y)))
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        if symbols.len() < width {
            continue;
        }
        for start in 0..=symbols.len() - width {
            let is_match = wanted.iter().enumerate().all(|(offset, glyph)| {
                symbols
                    .get(start + offset)
                    .is_some_and(|symbol| *symbol == glyph.to_string())
            });
            if is_match {
                return u16::try_from(start).ok().map(|x| (x, y));
            }
        }
    }
    None
}
