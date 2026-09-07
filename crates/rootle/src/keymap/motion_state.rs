//! The stateful part of motion grammar, separate from binding definitions.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionPrefix {
    Start,
    Align,
}

#[derive(Debug, Default)]
pub struct MotionCount(Option<usize>);
impl MotionCount {
    // The preview's row cursor is u16; larger counts can only mean EOF.
    const MAXIMUM: usize = u16::MAX as usize;

    pub fn push(&mut self, digit: u32) -> bool {
        if self.0.is_none() && digit == 0 {
            return false;
        }
        self.0 = Some(
            self.0
                .unwrap_or(0)
                .saturating_mul(10)
                .saturating_add(digit as usize)
                .min(Self::MAXIMUM),
        );
        true
    }
    pub fn take(&mut self) -> Option<usize> {
        self.0.take()
    }
    pub fn clear(&mut self) {
        self.0 = None;
    }
}
