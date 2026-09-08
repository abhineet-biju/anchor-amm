use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    #[msg("Fee exceeds the allowed basis-point limit")]
    InvalidFee,
    #[msg("Paused must be 0 or 1")]
    InvalidPaused,
}
