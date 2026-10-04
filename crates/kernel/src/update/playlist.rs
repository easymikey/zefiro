use crate::{
    cmd::Cmd,
    domain::{
        Direction,
        TrackIndex,
        ViewIndex,
        cycled,
        playlist::{PlayOrder, Playlist, RepeatMode},
    },
    update::machine::{Machine, Unhandled},
};

#[derive(Debug, Clone)]
pub enum PlaylistMessage {
    ToggleShuffle,
    ShuffleRolled(Vec<TrackIndex>),
    CycleRepeat,
}

impl Machine for Playlist {
    type Message = PlaylistMessage;
    type Effect = Cmd;

    fn transition(&mut self, message: PlaylistMessage) -> Result<Cmd, Unhandled> {
        Ok(match message {
            PlaylistMessage::ToggleShuffle => {
                self.play_order = match &self.play_order {
                    PlayOrder::Linear => PlayOrder::ShufflePending,
                    PlayOrder::ShufflePending | PlayOrder::Shuffle(_) => {
                        PlayOrder::Linear
                    }
                };
                Cmd::none()
            }
            PlaylistMessage::ShuffleRolled(order) => {
                self.play_order = match &self.play_order {
                    PlayOrder::Linear => PlayOrder::Linear,
                    PlayOrder::ShufflePending | PlayOrder::Shuffle(_) => {
                        PlayOrder::Shuffle(
                            order
                                .into_iter()
                                .map(|index| ViewIndex::new(index.get()))
                                .collect(),
                        )
                    }
                };
                Cmd::none()
            }
            PlaylistMessage::CycleRepeat => {
                self.repeat = cycle_repeat(self.repeat);
                Cmd::none()
            }
        })
    }
}

fn cycle_repeat(repeat: RepeatMode) -> RepeatMode {
    cycled(repeat, Direction::Next)
}
