mod common;

use {
    anchor_amm::error::ErrorCode,
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, program_pack::Pack, system_program},
        InstructionData, ToAccountMetas,
    },
    anchor_spl::{
        associated_token::{self, get_associated_token_address_with_program_id},
        token::{self, spl_token},
    },
    common::pool::{send, setup_pool, setup_pool_with_program, PoolAddresses, PoolFixture},
    litesvm::types::TransactionResult,
    solana_signer::Signer,
    solana_transaction::{InstructionError, TransactionError},
};

struct DepositFixture {
    pool: PoolFixture,
    addresses: PoolAddresses,
    user_a: Pubkey,
    user_b: Pubkey,
    user_lp: Pubkey,
}

fn setup_deposit() -> DepositFixture {
    let mut pool = setup_pool(0);
    let (result, addresses) = pool.initialize(pool.mint_a, pool.mint_b);
    result.unwrap();
    let user = pool.payer.pubkey();
    let mut user_accounts = Vec::new();
    for mint in [pool.mint_a, pool.mint_b] {
        let ata = get_associated_token_address_with_program_id(&user, &mint, &token::ID);
        let instructions = [
            associated_token::spl_associated_token_account::instruction::create_associated_token_account(&user, &user, &mint, &token::ID),
            spl_token::instruction::mint_to(&token::ID, &mint, &ata, &user, &[], 10_000).unwrap(),
        ];
        send(&mut pool.base, &pool.payer, &instructions, &[]).unwrap();
        user_accounts.push(ata);
    }
    let user_lp =
        get_associated_token_address_with_program_id(&user, &addresses.lp_mint, &token::ID);
    DepositFixture {
        pool,
        addresses,
        user_a: user_accounts[0],
        user_b: user_accounts[1],
        user_lp,
    }
}

impl DepositFixture {
    fn instruction(&self, a: u64, b: u64, min_lp_out: u64) -> Instruction {
        Instruction::new_with_bytes(
            anchor_amm::id(),
            &anchor_amm::instruction::DepositToPool {
                max_a: a,
                max_b: b,
                min_lp_out,
            }
            .data(),
            anchor_amm::accounts::Deposit {
                user: self.pool.payer.pubkey(),
                mint_a: self.pool.mint_a,
                mint_b: self.pool.mint_b,
                amm_config: self.pool.amm_config,
                pool_config: self.addresses.config,
                mint_lp: self.addresses.lp_mint,
                vault_a: self.addresses.vault_a,
                vault_b: self.addresses.vault_b,
                user_ata_a: self.user_a,
                user_ata_b: self.user_b,
                user_ata_l: self.user_lp,
                system_program: system_program::ID,
                token_program: token::ID,
                associated_token_program: associated_token::ID,
            }
            .to_account_metas(None),
        )
    }
    fn deposit(&mut self, a: u64, b: u64, min_lp: u64) -> TransactionResult {
        let ix = self.instruction(a, b, min_lp);
        send(&mut self.pool.base, &self.pool.payer, &[ix], &[])
    }
    fn balance(&self, address: Pubkey) -> u64 {
        spl_token::state::Account::unpack(&self.pool.base.svm.get_account(&address).unwrap().data)
            .unwrap()
            .amount
    }
    fn supply(&self) -> u64 {
        spl_token::state::Mint::unpack(
            &self
                .pool
                .base
                .svm
                .get_account(&self.addresses.lp_mint)
                .unwrap()
                .data,
        )
        .unwrap()
        .supply
    }
    fn snapshot(&self) -> Vec<Option<Vec<u8>>> {
        [
            self.user_a,
            self.user_b,
            self.user_lp,
            self.addresses.vault_a,
            self.addresses.vault_b,
            self.addresses.lp_mint,
        ]
        .iter()
        .map(|key| self.pool.base.svm.get_account(key).map(|a| a.data))
        .collect()
    }
}

#[test]
fn initial_deposit_accepts_exact_minimum() {
    let mut f = setup_deposit();
    f.deposit(1_000, 4_000, 2_000).unwrap();
    assert_eq!(f.balance(f.addresses.vault_a), 1_000);
    assert_eq!(f.balance(f.addresses.vault_b), 4_000);
    assert_eq!(f.balance(f.user_a), 9_000);
    assert_eq!(f.balance(f.user_b), 6_000);
    assert_eq!(f.balance(f.user_lp), 2_000);
    assert_eq!(f.supply(), 2_000);
}

#[test]
fn subsequent_deposits_respect_both_maximums() {
    let mut f = setup_deposit();
    f.deposit(1_000, 4_000, 2_000).unwrap();
    // A limits this deposit: accept 100 A and 400 B, not 500 B.
    f.deposit(100, 500, 200).unwrap();
    // B limits this deposit: accept 50 A and 200 B, not 100 A.
    f.deposit(100, 200, 100).unwrap();
    assert_eq!(f.balance(f.addresses.vault_a), 1_150);
    assert_eq!(f.balance(f.addresses.vault_b), 4_600);
    assert_eq!(f.balance(f.user_a), 8_850);
    assert_eq!(f.balance(f.user_b), 5_400);
    assert_eq!(f.balance(f.user_lp), 2_300);
    assert_eq!(f.supply(), 2_300);
}

#[test]
fn rejects_initial_deposit_below_minimum() {
    let mut f = setup_deposit();
    let before = f.snapshot();
    assert_eq!(
        f.deposit(1_000, 4_000, 2_001).unwrap_err().err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ErrorCode::SlippageExceeded.into())
        )
    );
    assert_eq!(before, f.snapshot());
}

#[test]
fn rejects_subsequent_deposit_below_minimum() {
    let mut f = setup_deposit();
    f.deposit(1_000, 4_000, 2_000).unwrap();
    let before = f.snapshot();
    assert_eq!(
        f.deposit(100, 400, 201).unwrap_err().err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ErrorCode::SlippageExceeded.into())
        )
    );
    assert_eq!(before, f.snapshot());
}

#[test]
fn second_transfer_failure_rolls_back_first_transfer_and_lp_account() {
    let mut f = setup_deposit();
    let before = f.snapshot();
    // A is funded, but the second transfer needs more B than the user owns.
    let failure = f.deposit(1_000, 10_001, 1).unwrap_err();
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(spl_token::error::TokenError::InsufficientFunds as u32)
        )
    );
    assert!(
        failure
            .meta
            .logs
            .iter()
            .filter(|line| line.contains("Instruction: TransferChecked"))
            .count()
            >= 2
    );
    assert_eq!(before, f.snapshot());
}

#[test]
fn rejects_zero_initial_deposit() {
    let mut f = setup_deposit();
    let before = f.snapshot();
    assert_eq!(
        f.deposit(0, 1_000, 0).unwrap_err().err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ErrorCode::InvalidDepositAmount.into())
        )
    );
    assert_eq!(before, f.snapshot());
}

#[test]
fn rejects_token_2022_pool_creation() {
    let mut f = setup_pool_with_program(0, anchor_spl::token_2022::ID);
    let (result, addresses) = f.initialize(f.mint_a, f.mint_b);
    assert_eq!(
        result.unwrap_err().err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(anchor_lang::error::ErrorCode::ConstraintAddress.into())
        )
    );
    for key in [
        addresses.config,
        addresses.vault_a,
        addresses.vault_b,
        addresses.lp_mint,
    ] {
        assert!(f.base.svm.get_account(&key).is_none());
    }
}

#[test]
fn rejects_token_2022_deposit_program() {
    let mut f = setup_deposit();
    let before = f.snapshot();
    let mut ix = f.instruction(1_000, 4_000, 2_000);
    let meta = ix
        .accounts
        .iter_mut()
        .find(|m| m.pubkey == token::ID)
        .unwrap();
    meta.pubkey = anchor_spl::token_2022::ID;
    assert!(send(&mut f.pool.base, &f.pool.payer, &[ix], &[]).is_err());
    assert_eq!(before, f.snapshot());
}
