use crate::{
    cmd::Cmd,
    domain::{
        cursor_over::cycled,
        direction::Direction,
        index::ViewIndex,
        playlist::{PlayOrder, Playlist},
    },
    update::machine::{Machine, Unhandled},
};

#[derive(Debug, Clone)]
pub enum PlaylistMessage {
    ToggleShuffle,
    ShuffleRolled(Vec<ViewIndex>),
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
            PlaylistMessage::ShuffleRolled(order) => match &self.play_order {
                PlayOrder::Linear => return Err(Unhandled),
                PlayOrder::ShufflePending | PlayOrder::Shuffle(_) => {
                    self.play_order = PlayOrder::Shuffle(order);
                    Cmd::none()
                }
            },
            PlaylistMessage::CycleRepeat => {
                self.repeat = cycled(self.repeat, Direction::Next);
                Cmd::none()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        cmd::Cmd,
        domain::{
            index::ViewIndex,
            playlist::{PlayOrder, Playlist, RepeatMode},
        },
        update::{
            machine::{Machine, Unhandled},
            playlist::PlaylistMessage,
        },
    };

    fn ordered(play_order: PlayOrder, repeat_mode: RepeatMode) -> Playlist {
        Playlist {
            play_order,
            repeat: repeat_mode,
            ..Playlist::default()
        }
    }

    fn shuffled(positions: &[usize]) -> PlayOrder {
        PlayOrder::Shuffle(positions.iter().copied().map(ViewIndex::new).collect())
    }

    fn rolled(positions: &[usize]) -> PlaylistMessage {
        PlaylistMessage::ShuffleRolled(
            positions.iter().copied().map(ViewIndex::new).collect(),
        )
    }

    #[rstest]
    #[case::toggle_shuffle_from_linear(
        ordered(PlayOrder::Linear, RepeatMode::Off),
        PlaylistMessage::ToggleShuffle,
        ordered(PlayOrder::ShufflePending, RepeatMode::Off)
    )]
    #[case::toggle_shuffle_from_pending(
        ordered(PlayOrder::ShufflePending, RepeatMode::Off),
        PlaylistMessage::ToggleShuffle,
        ordered(PlayOrder::Linear, RepeatMode::Off)
    )]
    #[case::toggle_shuffle_from_shuffled(ordered(shuffled(&[1, 0]), RepeatMode::Off), PlaylistMessage::ToggleShuffle, ordered(PlayOrder::Linear, RepeatMode::Off))]
    #[case::shuffle_rolled_while_pending(ordered(PlayOrder::ShufflePending, RepeatMode::Off), rolled(&[2, 0, 1]), ordered(shuffled(&[2, 0, 1]), RepeatMode::Off))]
    #[case::shuffle_rolled_while_shuffled(ordered(shuffled(&[1, 0]), RepeatMode::Off), rolled(&[2, 0, 1]), ordered(shuffled(&[2, 0, 1]), RepeatMode::Off))]
    #[case::cycle_repeat_from_off(
        ordered(PlayOrder::Linear, RepeatMode::Off),
        PlaylistMessage::CycleRepeat,
        ordered(PlayOrder::Linear, RepeatMode::All)
    )]
    #[case::cycle_repeat_from_all(
        ordered(PlayOrder::Linear, RepeatMode::All),
        PlaylistMessage::CycleRepeat,
        ordered(PlayOrder::Linear, RepeatMode::One)
    )]
    #[case::cycle_repeat_from_one(
        ordered(PlayOrder::Linear, RepeatMode::One),
        PlaylistMessage::CycleRepeat,
        ordered(PlayOrder::Linear, RepeatMode::Off)
    )]
    fn a_playlist_message_updates_the_play_order_and_repeat(
        #[case] mut playlist: Playlist,
        #[case] message: PlaylistMessage,
        #[case] after: Playlist,
    ) {
        assert_eq!(playlist.transition(message), Ok(Cmd::none()));
        assert_eq!(
            (playlist.play_order, playlist.repeat),
            (after.play_order, after.repeat)
        );
    }

    #[test]
    fn a_shuffle_roll_while_linear_is_refused() {
        let mut playlist = ordered(PlayOrder::Linear, RepeatMode::Off);

        assert_eq!(playlist.transition(rolled(&[2, 0, 1])), Err(Unhandled));
        assert_eq!(playlist.play_order, PlayOrder::Linear);
    }
}
