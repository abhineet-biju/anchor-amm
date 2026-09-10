pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use instructions::*;
pub use state::*;

declare_id!("CNKWbEdckorAEQJVoDnC5HNLgsqGsotqMnCbV9h4jVne");

#[program]
pub mod anchor_amm {
    use super::*;

    #[instruction(discriminator = [1])]
    pub fn initialize_amm(
        ctx: Context<InitializeAmm>,
        id: u64,
        fee: u16,
        paused: u8,
    ) -> Result<()> {
        ctx.accounts.handler(id, fee, paused, &ctx.bumps)
    }

    #[instruction(discriminator = [2])]
    pub fn initialize_pool(ctx: Context<InitializePool>, _id: u64) -> Result<()> {
        ctx.accounts.handler(&ctx.bumps)
    }

    #[instruction(discriminator = [3])]
    pub fn deposit_to_pool(
        ctx: Context<Deposit>,
        max_a: u64,
        max_b: u64,
        min_lp_out: u64,
    ) -> Result<()> {
        ctx.accounts.handler(max_a, max_b, min_lp_out)
    }
}
