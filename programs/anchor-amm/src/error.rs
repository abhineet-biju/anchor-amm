use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    #[msg("Fee exceeds the allowed basis-point limit")]
    InvalidFee,
    #[msg("Paused must be 0 or 1")]
    InvalidPaused,
    #[msg("Provided mints must be ordered correctly, and not be identical")]
    InvalidMintPair,
    #[msg("Cannot initialize pool with a paused AMM Config")]
    InvalidAmmState,
}
