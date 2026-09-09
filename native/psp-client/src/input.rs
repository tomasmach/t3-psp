/// Navigation and keyboard backspace repeat. Other actions need a fresh press.
#[derive(Default)]
pub struct Input {
    previous: u32,
    blocked: u32,
    directions: Repeat,
    stick: Repeat,
    stick_direction: u32,
    stick_blocked: bool,
}

#[derive(Default)]
struct Repeat {
    held: u32,
    next: i64,
}

impl Repeat {
    fn sample(&mut self, held: u32, now: i64, delay: i64, interval: i64) -> u32 {
        if held != self.held {
            self.held = held;
            self.next = now + delay;
            held
        } else if held != 0 && now >= self.next {
            // A blocking network request must not accumulate a burst of repeats.
            self.next = now + interval;
            held
        } else {
            0
        }
    }
}

impl Input {
    /// Consume controls held across a screen transition or a blocking operation.
    pub fn suppress(&mut self, buttons: u32) {
        self.previous = buttons;
        self.blocked = buttons;
        self.directions = Repeat::default();
        self.stick = Repeat::default();
        self.stick_direction = 0;
        self.stick_blocked = true;
    }

    pub fn sample(
        &mut self,
        buttons: u32,
        repeat_mask: u32,
        analog_y: u8,
        now: i64,
    ) -> (u32, isize) {
        self.blocked &= buttons;
        let fresh = buttons & !self.previous & !self.blocked;
        self.previous = buttons;
        let directions =
            self.directions
                .sample(buttons & repeat_mask & !self.blocked, now, 350_000, 100_000);
        let y = i16::from(analog_y) - 128;
        if y.abs() <= 32 {
            self.stick_blocked = false;
            self.stick_direction = 0;
        } else if !self.stick_blocked {
            if y <= -48 {
                self.stick_direction = 1;
            } else if y >= 48 {
                self.stick_direction = 2;
            } else if (y > 0 && self.stick_direction == 1) || (y < 0 && self.stick_direction == 2) {
                self.stick_direction = 0;
            }
        }
        let interval = if y.abs() >= 96 { 70_000 } else { 140_000 };
        let scroll = match self
            .stick
            .sample(self.stick_direction, now, 200_000, interval)
        {
            1 => -1,
            2 => 1,
            _ => 0,
        };
        (fresh | directions, scroll)
    }
}

/// Wrap within a keyboard row/column, skipping empty cells in the final row.
pub fn move_key(key: usize, count: usize, dx: isize, dy: isize) -> usize {
    if count == 0 {
        return 0;
    }
    let key = key.min(count - 1);
    let row = key / 7;
    let column = key % 7;
    if dx != 0 {
        let width = (count - row * 7).min(7);
        row * 7 + (column as isize + dx).rem_euclid(width as isize) as usize
    } else {
        let height = (count - 1 - column) / 7 + 1;
        (row as isize + dy).rem_euclid(height as isize) as usize * 7 + column
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const UP: u32 = 1;
    const DOWN: u32 = 2;
    const CONFIRM: u32 = 4;
    const SQUARE: u32 = 8;
    const DIRECTIONS: u32 = UP | DOWN;

    #[test]
    fn held_backspace_deletes_unicode_after_a_delay_and_stops_on_release() {
        let mut input = Input::default();
        let mut draft = alloc::string::String::from("ačů");
        for (buttons, time, expected) in [
            (SQUARE, 0, "ač"),
            (SQUARE, 349_999, "ač"),
            (SQUARE, 350_000, "a"),
            (0, 450_000, "a"),
            (0, 900_000, "a"),
            (SQUARE, 1_000_000, ""),
            (SQUARE, 1_350_000, ""),
            (SQUARE, 1_450_000, ""),
        ] {
            if input.sample(buttons, DIRECTIONS | SQUARE, 128, time).0 & SQUARE != 0 {
                draft.pop();
            }
            assert_eq!(draft, expected);
        }
    }

    #[test]
    fn held_delete_does_not_start_recording_after_leaving_the_keyboard() {
        let mut input = Input::default();
        assert_eq!(input.sample(SQUARE, DIRECTIONS | SQUARE, 128, 0).0, SQUARE);
        input.suppress(SQUARE);
        assert_eq!(input.sample(SQUARE, DIRECTIONS, 128, 500_000).0, 0);
        input.sample(0, DIRECTIONS, 128, 600_000);
        assert_eq!(input.sample(SQUARE, DIRECTIONS, 128, 700_000).0, SQUARE);
        assert_eq!(input.sample(SQUARE, DIRECTIONS, 128, 1_050_000).0, 0);
    }

    #[test]
    fn keyboard_wraps_in_each_axis_without_selecting_empty_cells() {
        assert_eq!(move_key(0, 43, -1, 0), 6);
        assert_eq!(move_key(6, 43, 1, 0), 0);
        assert_eq!(move_key(42, 43, 1, 0), 42);
        assert_eq!(move_key(42, 43, -1, 0), 42);
        assert_eq!(move_key(42, 43, 0, 1), 0);
        assert_eq!(move_key(0, 43, 0, -1), 42);
        assert_eq!(move_key(6, 43, 0, -1), 41);
        assert_eq!(move_key(41, 43, 0, 1), 6);
        for key in 0..43 {
            for (dx, dy) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
                let next = move_key(key, 43, dx, dy);
                assert!(next < 43);
                assert_eq!(move_key(next, 43, -dx, -dy), key);
            }
        }
    }

    #[test]
    fn navigation_repeats_but_actions_require_release() {
        let mut input = Input::default();
        assert_eq!(
            input.sample(DOWN | CONFIRM, DIRECTIONS, 128, 0).0,
            DOWN | CONFIRM
        );
        assert_eq!(input.sample(DOWN | CONFIRM, DIRECTIONS, 128, 349_999).0, 0);
        assert_eq!(
            input.sample(DOWN | CONFIRM, DIRECTIONS, 128, 350_000).0,
            DOWN
        );
        assert_eq!(
            input.sample(DOWN | CONFIRM, DIRECTIONS, 128, 450_000).0,
            DOWN
        );
        assert_eq!(input.sample(UP | CONFIRM, DIRECTIONS, 128, 460_000).0, UP);
        input.sample(0, DIRECTIONS, 128, 470_000);
        assert_eq!(input.sample(CONFIRM, DIRECTIONS, 128, 480_000).0, CONFIRM);
    }

    #[test]
    fn waits_do_not_accumulate_navigation_steps() {
        let mut input = Input::default();
        input.sample(DOWN, DIRECTIONS, 128, 0);
        assert_eq!(input.sample(DOWN, DIRECTIONS, 128, 20_000_000).0, DOWN);
        assert_eq!(input.sample(DOWN, DIRECTIONS, 128, 20_000_001).0, 0);
    }

    #[test]
    fn analog_ignores_drift_and_repeats_in_both_directions() {
        let mut input = Input::default();
        for y in 90..=166 {
            assert_eq!(input.sample(0, DIRECTIONS, y, 0).1, 0);
        }
        assert_eq!(input.sample(0, DIRECTIONS, 64, 0).1, -1);
        assert_eq!(input.sample(0, DIRECTIONS, 87, 100_000).1, 0);
        assert_eq!(input.sample(0, DIRECTIONS, 64, 200_000).1, -1);
        assert_eq!(input.sample(0, DIRECTIONS, 255, 210_000).1, 1);
        assert_eq!(input.sample(0, DIRECTIONS, 255, 410_000).1, 1);
        assert_eq!(input.sample(0, DIRECTIONS, 255, 480_000).1, 1);
        assert_eq!(input.sample(0, DIRECTIONS, 128, 490_000).1, 0);
    }

    #[test]
    fn entering_a_screen_requires_release_and_centering() {
        let mut input = Input::default();
        input.suppress(DOWN | CONFIRM);
        assert_eq!(input.sample(DOWN | CONFIRM, DIRECTIONS, 255, 0), (0, 0));
        assert_eq!(
            input.sample(DOWN | CONFIRM, DIRECTIONS, 255, 1_000_000),
            (0, 0)
        );
        input.sample(0, DIRECTIONS, 128, 1_100_000);
        assert_eq!(
            input.sample(CONFIRM, DIRECTIONS, 255, 1_200_000),
            (CONFIRM, 1)
        );
    }

    #[test]
    fn crossing_center_between_samples_does_not_keep_the_old_direction() {
        let mut input = Input::default();
        assert_eq!(input.sample(0, DIRECTIONS, 64, 0).1, -1);
        assert_eq!(input.sample(0, DIRECTIONS, 168, 300_000).1, 0);
        assert_eq!(input.sample(0, DIRECTIONS, 192, 400_000).1, 1);
        assert_eq!(input.sample(0, DIRECTIONS, 88, 700_000).1, 0);
    }
}
