use {
    anchor_amm::{error::ErrorCode, AmmConfig},
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_program},
        AccountDeserialize, Discriminator, InstructionData, Space, ToAccountMetas,
    },
    litesvm::{types::TransactionResult, LiteSVM},
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::{versioned::VersionedTransaction, InstructionError, TransactionError},
};

struct TestFixture {
    svm: LiteSVM,
    maker: Keypair,
    admin: Keypair,
}

struct InitializeOutcome {
    result: TransactionResult,
    amm_config: Pubkey,
    bump: u8,
}

fn setup() -> TestFixture {
    let maker = Keypair::new();
    let admin = Keypair::new();
    let mut svm = LiteSVM::new();
    // Build the SBF program before running these tests with `anchor test`.
    let bytes = include_bytes!(concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/../deploy/anchor_amm.so"
    ));
    svm.add_program(anchor_amm::id(), bytes).unwrap();
    svm.airdrop(&maker.pubkey(), 1_000_000_000).unwrap();
    TestFixture { svm, maker, admin }
}

impl TestFixture {
    fn initialize_amm(&mut self, id: u64, fee: u16, paused: u8) -> InitializeOutcome {
        let program_id = anchor_amm::id();
        let (amm_config, bump) = Pubkey::find_program_address(
            &[b"amm", self.maker.pubkey().as_ref(), &id.to_le_bytes()],
            &program_id,
        );
        let instruction = Instruction::new_with_bytes(
            program_id,
            &anchor_amm::instruction::InitializeAmm { id, fee, paused }.data(),
            anchor_amm::accounts::InitializeAmm {
                maker: self.maker.pubkey(),
                admin: self.admin.pubkey(),
                amm_config,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        let message = Message::new_with_blockhash(
            &[instruction],
            Some(&self.maker.pubkey()),
            &self.svm.latest_blockhash(),
        );
        let transaction = VersionedTransaction::try_new(
            VersionedMessage::Legacy(message),
            &[&self.maker, &self.admin],
        )
        .unwrap();
        InitializeOutcome {
            result: self.svm.send_transaction(transaction),
            amm_config,
            bump,
        }
    }
}

#[test]
fn initialize_amm_creates_config() {
    let mut fixture = setup();
    // Exercise the full u64 ID rather than the old u16 seed encoding.
    let id = 65_537;
    let fee = 30;
    let paused = 1;
    let outcome = fixture.initialize_amm(id, fee, paused);
    outcome.result.expect("AMM initialization should succeed");

    let account = fixture
        .svm
        .get_account(&outcome.amm_config)
        .expect("config should exist");
    assert_eq!(account.owner, anchor_amm::id());
    assert_eq!(
        account.data.len(),
        AmmConfig::DISCRIMINATOR.len() + AmmConfig::INIT_SPACE,
    );
    let config = AmmConfig::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert_eq!(config.maker, fixture.maker.pubkey());
    assert_eq!(config.admin, fixture.admin.pubkey());
    assert_eq!(config.id, id);
    assert_eq!(config.fee, fee);
    assert_eq!(config.paused, paused);
    assert_eq!(config.bump, outcome.bump);
}

#[test]
fn rejects_invalid_fee() {
    let mut fixture = setup();
    let outcome = fixture.initialize_amm(65_537, 10_001, 0);
    let failure = outcome.result.expect_err("fee above 10,000 must fail");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ErrorCode::InvalidFee.into())
        ),
    );
    assert!(fixture.svm.get_account(&outcome.amm_config).is_none());
}

#[test]
fn rejects_invalid_paused() {
    let mut fixture = setup();
    let outcome = fixture.initialize_amm(65_537, 30, 2);
    let failure = outcome
        .result
        .expect_err("paused other than 0 or 1 must fail");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ErrorCode::InvalidPaused.into()),
        ),
    );
    assert!(fixture.svm.get_account(&outcome.amm_config).is_none());
}
