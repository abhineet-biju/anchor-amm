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
}
