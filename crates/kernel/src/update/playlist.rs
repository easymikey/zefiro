use crate::{
    cmd::Cmd,
    domain::{
        Direction,
        cycled,
        playlist::{PlayOrder, Playlist, RepeatMode},
    },
    update::machine::{Machine, Rejected},
};

#[derive(Debug, Clone)]
pub enum PlaylistMessage {
    ToggleShuffle,
    ShuffleRolled(Vec<usize>),
    CycleRepeat,
}

impl Machine for Playlist {
    type Message = PlaylistMessage;
    type Error = std::convert::Infallible;
    type Effect = Cmd;

    fn transition(
        mut self,
        message: PlaylistMessage,
    ) -> Result<(Self, Cmd), Rejected<Self>> {
        let cmd = match message {
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
                        PlayOrder::Shuffle(order)
                    }
                };
                Cmd::None
            }
            PlaylistMessage::CycleRepeat => {
                self.repeat = cycle_repeat(self.repeat);
                Cmd::None
            }
        };
        Ok((self, cmd))
    }
}

fn cycle_repeat(repeat: RepeatMode) -> RepeatMode {
    cycled(repeat, Direction::Next)
}
