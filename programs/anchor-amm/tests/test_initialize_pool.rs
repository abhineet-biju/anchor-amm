mod common;

use {
    anchor_amm::{error::ErrorCode, PoolConfig},
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{
            instruction::Instruction, program_option::COption, program_pack::Pack,
            system_instruction, system_program,
        },
        AccountDeserialize, Discriminator, InstructionData, Space, ToAccountMetas,
    },
    anchor_spl::{
        associated_token::{self, get_associated_token_address_with_program_id},
        token::{self, spl_token},
    },
    common::{setup, TestFixture},
    litesvm::types::TransactionResult,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::{versioned::VersionedTransaction, InstructionError, TransactionError},
};

const AMM_ID: u64 = 65_537;

struct PoolFixture {
    base: TestFixture,
    payer: Keypair,
    amm_config: Pubkey,
    mint_a: Pubkey,
    mint_b: Pubkey,
}

struct PoolAddresses {
    config: Pubkey,
    bump: u8,
    vault_a: Pubkey,
    vault_b: Pubkey,
    lp_mint: Pubkey,
    lp_bump: u8,
}

fn send(
    base: &mut TestFixture,
    payer: &Keypair,
    instructions: &[Instruction],
    extra: &[&Keypair],
) -> TransactionResult {
    let message = Message::new_with_blockhash(
        instructions,
        Some(&payer.pubkey()),
        &base.svm.latest_blockhash(),
    );
    let mut signers = vec![payer];
    signers.extend_from_slice(extra);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(message), &signers).unwrap();
    base.svm.send_transaction(tx)
}

fn setup_pool(paused: u8) -> PoolFixture {
    let mut base = setup();
    let amm = base.initialize_amm(AMM_ID, 30, paused);
    amm.result.unwrap();
    // Pool creation is permissionless: the payer is distinct from the AMM maker.
    let payer = Keypair::new();
    base.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    let mut mints = Vec::new();
    for decimals in [6, 9] {
        let mint = Keypair::new();
        let instructions = [
            system_instruction::create_account(
                &payer.pubkey(),
                &mint.pubkey(),
                base.svm
                    .minimum_balance_for_rent_exemption(spl_token::state::Mint::LEN),
                spl_token::state::Mint::LEN as u64,
                &token::ID,
            ),
            spl_token::instruction::initialize_mint2(
                &token::ID,
                &mint.pubkey(),
                &payer.pubkey(),
                None,
                decimals,
            )
            .unwrap(),
        ];
        send(&mut base, &payer, &instructions, &[&mint]).unwrap();
        mints.push(mint.pubkey());
    }
    mints.sort();
    PoolFixture {
        base,
        payer,
        amm_config: amm.amm_config,
        mint_a: mints[0],
        mint_b: mints[1],
    }
}

impl PoolFixture {
    fn addresses(&self, mint_a: Pubkey, mint_b: Pubkey) -> PoolAddresses {
        let (config, bump) = Pubkey::find_program_address(
            &[
                b"pool",
                self.amm_config.as_ref(),
                mint_a.as_ref(),
                mint_b.as_ref(),
            ],
            &anchor_amm::id(),
        );
        let (lp_mint, lp_bump) =
            Pubkey::find_program_address(&[b"lp_mint", config.as_ref()], &anchor_amm::id());
        PoolAddresses {
            config,
            bump,
            lp_mint,
            lp_bump,
            vault_a: get_associated_token_address_with_program_id(&config, &mint_a, &token::ID),
            vault_b: get_associated_token_address_with_program_id(&config, &mint_b, &token::ID),
        }
    }

    fn initialize(&mut self, mint_a: Pubkey, mint_b: Pubkey) -> (TransactionResult, PoolAddresses) {
        let addresses = self.addresses(mint_a, mint_b);
        let ix = Instruction::new_with_bytes(
            anchor_amm::id(),
            &anchor_amm::instruction::InitializePool { id: AMM_ID }.data(),
            anchor_amm::accounts::InitializePool {
                payer: self.payer.pubkey(),
                maker: self.base.maker.pubkey(),
                amm_config: self.amm_config,
                mint_a,
                mint_b,
                pool_config: addresses.config,
                vault_a: addresses.vault_a,
                vault_b: addresses.vault_b,
                lp_mint: addresses.lp_mint,
                system_program: system_program::ID,
                token_program: token::ID,
                associated_token_program: associated_token::ID,
            }
            .to_account_metas(None),
        );
        (send(&mut self.base, &self.payer, &[ix], &[]), addresses)
    }
}

#[test]
fn creates_pool_vaults_and_lp_mint() {
    let mut fixture = setup_pool(0);
    let (result, addresses) = fixture.initialize(fixture.mint_a, fixture.mint_b);
    result.expect("pool initialization should succeed");
    let account = fixture.base.svm.get_account(&addresses.config).unwrap();
    assert_eq!(account.owner, anchor_amm::id());
    assert_eq!(
        account.data.len(),
        PoolConfig::DISCRIMINATOR.len() + PoolConfig::INIT_SPACE
    );
    let config = PoolConfig::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert_eq!(config.amm_config, fixture.amm_config);
    assert_eq!(config.mint_a, fixture.mint_a);
    assert_eq!(config.mint_b, fixture.mint_b);
    assert_eq!(config.bump, addresses.bump);
    assert_eq!(config.lp_bump, addresses.lp_bump);
    for (address, mint) in [
        (addresses.vault_a, fixture.mint_a),
        (addresses.vault_b, fixture.mint_b),
    ] {
        let account = fixture.base.svm.get_account(&address).unwrap();
        assert_eq!(account.owner, token::ID);
        let vault = spl_token::state::Account::unpack(&account.data).unwrap();
        assert_eq!(vault.mint, mint);
        assert_eq!(vault.owner, addresses.config);
        assert_eq!(vault.amount, 0);
    }
    let account = fixture.base.svm.get_account(&addresses.lp_mint).unwrap();
    assert_eq!(account.owner, token::ID);
    let mint = spl_token::state::Mint::unpack(&account.data).unwrap();
    assert_eq!(mint.mint_authority, COption::Some(addresses.config));
    assert_eq!(mint.freeze_authority, COption::None);
    assert_eq!(mint.decimals, 6);
    assert_eq!(mint.supply, 0);
}

#[test]
fn rejects_reversed_mints() {
    let mut fixture = setup_pool(0);
    let (result, addresses) = fixture.initialize(fixture.mint_b, fixture.mint_a);
    assert_eq!(
        result.unwrap_err().err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ErrorCode::InvalidMintPair.into())
        )
    );
    for key in [
        addresses.config,
        addresses.vault_a,
        addresses.vault_b,
        addresses.lp_mint,
    ] {
        assert!(fixture.base.svm.get_account(&key).is_none());
    }
}

#[test]
fn rejects_identical_mints() {
    let mut fixture = setup_pool(0);
    let (result, addresses) = fixture.initialize(fixture.mint_a, fixture.mint_a);
    // Duplicate mutable vaults may be rejected by Anchor before the handler runs.
    assert!(result.is_err());
    for key in [addresses.config, addresses.vault_a, addresses.lp_mint] {
        assert!(fixture.base.svm.get_account(&key).is_none());
    }
}

#[test]
fn rejects_paused_amm() {
    let mut fixture = setup_pool(1);
    let (result, addresses) = fixture.initialize(fixture.mint_a, fixture.mint_b);
    assert_eq!(
        result.unwrap_err().err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ErrorCode::InvalidAmmState.into())
        )
    );
    for key in [
        addresses.config,
        addresses.vault_a,
        addresses.vault_b,
        addresses.lp_mint,
    ] {
        assert!(fixture.base.svm.get_account(&key).is_none());
    }
}

#[test]
fn rejects_duplicate_pool_without_changing_state() {
    let mut fixture = setup_pool(0);
    let (result, addresses) = fixture.initialize(fixture.mint_a, fixture.mint_b);
    result.unwrap();
    let keys = [
        addresses.config,
        addresses.vault_a,
        addresses.vault_b,
        addresses.lp_mint,
    ];
    let before: Vec<_> = keys
        .iter()
        .map(|key| fixture.base.svm.get_account(key).unwrap())
        .collect();
    // Ensure this reaches account validation instead of duplicate-transaction detection.
    fixture.base.svm.expire_blockhash();
    let (result, _) = fixture.initialize(fixture.mint_a, fixture.mint_b);
    assert_eq!(
        result.unwrap_err().err,
        TransactionError::InstructionError(0, InstructionError::Custom(0))
    );
    let after: Vec<_> = keys
        .iter()
        .map(|key| fixture.base.svm.get_account(key).unwrap())
        .collect();
    assert_eq!(before, after);
}

#[test]
fn reuses_preexisting_vaults() {
    let mut fixture = setup_pool(0);
    let addresses = fixture.addresses(fixture.mint_a, fixture.mint_b);
    let instructions: Vec<_> = [fixture.mint_a, fixture.mint_b].iter().map(|mint|
        associated_token::spl_associated_token_account::instruction::create_associated_token_account(
            &fixture.payer.pubkey(), &addresses.config, mint, &token::ID)
    ).collect();
    send(&mut fixture.base, &fixture.payer, &instructions, &[]).unwrap();
    let before_a = fixture.base.svm.get_account(&addresses.vault_a).unwrap();
    let before_b = fixture.base.svm.get_account(&addresses.vault_b).unwrap();
    let (result, _) = fixture.initialize(fixture.mint_a, fixture.mint_b);
    result.unwrap();
    assert_eq!(
        fixture.base.svm.get_account(&addresses.vault_a).unwrap(),
        before_a
    );
    assert_eq!(
        fixture.base.svm.get_account(&addresses.vault_b).unwrap(),
        before_b
    );
}
