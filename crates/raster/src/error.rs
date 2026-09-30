#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("pixel canvas {width}x{height} is empty or too large")]
    Canvas { width: u32, height: u32 },
}
