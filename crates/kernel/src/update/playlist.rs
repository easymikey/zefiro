use crate::{
    cmd::Cmd,
    domain::{
        Direction,
        TrackIndex,
        cycled,
        playlist::{PlayOrder, Playlist, RepeatMode},
    },
};

#[derive(Debug, Clone)]
pub enum PlaylistMessage {
    ToggleShuffle,
    ShuffleRolled(Vec<TrackIndex>),
    CycleRepeat,
}

impl Playlist {
    pub fn apply(&mut self, message: PlaylistMessage) -> Cmd {
        match message {
            PlaylistMessage::ToggleShuffle => {
                self.play_order = match &self.play_order {
                    PlayOrder::Linear => PlayOrder::ShufflePending,
                    PlayOrder::ShufflePending | PlayOrder::Shuffle(_) => {
                        PlayOrder::Linear
                    }
                };
                Cmd::None
            }
            PlaylistMessage::ShuffleRolled(order) => {
                self.play_order = match &self.play_order {
                    PlayOrder::Linear => PlayOrder::Linear,
                    PlayOrder::ShufflePending | PlayOrder::Shuffle(_) => {
                        PlayOrder::Shuffle(order.into_iter().map(usize::from).collect())
                    }
                };
                Cmd::None
            }
            PlaylistMessage::CycleRepeat => {
                self.repeat = cycle_repeat(self.repeat);
                Cmd::None
            }
        }
    }
}

fn cycle_repeat(repeat: RepeatMode) -> RepeatMode {
    cycled(repeat, Direction::Next)
}
