use anchor_lang::prelude::*;

#[account(discriminator = [1])]
#[derive(InitSpace)]
pub struct AmmConfig {
    pub maker: Pubkey,
    pub admin: Pubkey,
    pub id: u64,
    pub fee: u16, // In basis points
    pub paused: u8,
    pub bump: u8,
}
