extern crate std;

use super::*;
use soroban_sdk::{testutils::Address as _, token::StellarAssetClient, token::TokenClient, vec};

struct Setup<'a> {
    env: Env,
    escrow: EscrowClient<'a>,
    token: TokenClient<'a>,
    client: Address,
    freelancer: Address,
}

fn setup<'a>() -> Setup<'a> {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let client = Address::generate(&env);
    let freelancer = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    StellarAssetClient::new(&env, &sac.address()).mint(&client, &1_000);
    let id = env.register(Escrow, ());
    let escrow = EscrowClient::new(&env, &id);
    escrow.initialize(&admin);
    let token = TokenClient::new(&env, &sac.address());
    Setup {
        env,
        escrow,
        token,
        client,
        freelancer,
    }
}

#[test]
fn initialize_only_once() {
    let s = setup();
    let other = Address::generate(&s.env);
    assert_eq!(
        s.escrow.try_initialize(&other),
        Err(Ok(EscrowError::AlreadyInitialized))
    );
}

#[test]
fn full_happy_path() {
    let s = setup();
    let id = s.escrow.create_contract(
        &s.client,
        &s.freelancer,
        &s.token.address,
        &vec![&s.env, 100i128, 200i128],
    );
    assert_eq!(id, 1);
    s.escrow.deposit_funds(&id, &300);
    assert_eq!(s.token.balance(&s.client), 700);
    assert_eq!(s.escrow.get_contract(&id).status, ContractStatus::Funded);
    s.escrow.release_milestone(&id, &0);
    s.escrow.release_milestone(&id, &1);
    assert_eq!(s.token.balance(&s.freelancer), 300);
    assert_eq!(s.escrow.get_contract(&id).status, ContractStatus::Completed);
    s.escrow.issue_reputation(&id, &5);
    let rep = s.escrow.get_reputation(&s.freelancer);
    assert_eq!(rep.completed_contracts, 1);
    assert_eq!(rep.total_rating, 5);
}

#[test]
fn rejects_overfunding_and_double_release() {
    let s = setup();
    let id = s.escrow.create_contract(
        &s.client,
        &s.freelancer,
        &s.token.address,
        &vec![&s.env, 100i128],
    );
    assert_eq!(
        s.escrow.try_deposit_funds(&id, &101),
        Err(Ok(EscrowError::OverFunded))
    );
    s.escrow.deposit_funds(&id, &100);
    s.escrow.release_milestone(&id, &0);
    assert!(s.escrow.try_release_milestone(&id, &0).is_err());
}

#[test]
fn dispute_split_and_cancel() {
    let s = setup();
    let id = s.escrow.create_contract(
        &s.client,
        &s.freelancer,
        &s.token.address,
        &vec![&s.env, 400i128],
    );
    s.escrow.deposit_funds(&id, &400);
    s.escrow.open_dispute(&id, &s.freelancer);
    s.escrow.resolve_dispute(&id, &2_500);
    assert_eq!(s.token.balance(&s.freelancer), 100);
    assert_eq!(s.token.balance(&s.client), 900);

    let id2 = s.escrow.create_contract(
        &s.client,
        &s.freelancer,
        &s.token.address,
        &vec![&s.env, 50i128],
    );
    s.escrow.deposit_funds(&id2, &50);
    s.escrow.cancel_contract(&id2);
    assert_eq!(s.token.balance(&s.client), 900);
    assert_eq!(
        s.escrow.get_contract(&id2).status,
        ContractStatus::Cancelled
    );
}

#[test]
fn team_split_pays_three_way_with_lead_remainder() {
    let s = setup();
    let teammate_a = Address::generate(&s.env);
    let teammate_b = Address::generate(&s.env);
    let id = s.escrow.create_contract(
        &s.client,
        &s.freelancer,
        &s.token.address,
        &vec![&s.env, 101i128],
    );
    let payees = vec![
        &s.env,
        (s.freelancer.clone(), 5_000u32),
        (teammate_a.clone(), 3_000u32),
        (teammate_b.clone(), 2_000u32),
    ];
    s.escrow.set_team_split(&id, &payees);
    assert_eq!(s.escrow.get_team_split(&id), payees);

    s.escrow.deposit_funds(&id, &101);
    s.escrow.release_milestone(&id, &0);

    assert_eq!(s.token.balance(&s.freelancer), 51);
    assert_eq!(s.token.balance(&teammate_a), 30);
    assert_eq!(s.token.balance(&teammate_b), 20);
    assert_eq!(
        s.token.balance(&s.freelancer)
            + s.token.balance(&teammate_a)
            + s.token.balance(&teammate_b),
        101
    );
    assert_eq!(s.token.balance(&s.client), 899);
}

#[test]
fn rejects_invalid_team_splits() {
    let s = setup();
    let id = s.escrow.create_contract(
        &s.client,
        &s.freelancer,
        &s.token.address,
        &vec![&s.env, 100i128],
    );
    let other = Address::generate(&s.env);

    let bad_sum = vec![
        &s.env,
        (s.freelancer.clone(), 5_000u32),
        (other.clone(), 4_000u32),
    ];
    assert_eq!(
        s.escrow.try_set_team_split(&id, &bad_sum),
        Err(Ok(EscrowError::InvalidSplitConfig))
    );

    let zero_share = vec![&s.env, (s.freelancer.clone(), 0u32)];
    assert_eq!(
        s.escrow.try_set_team_split(&id, &zero_share),
        Err(Ok(EscrowError::InvalidSplitConfig))
    );

    let duplicate = vec![
        &s.env,
        (s.freelancer.clone(), 5_000u32),
        (s.freelancer.clone(), 5_000u32),
    ];
    assert_eq!(
        s.escrow.try_set_team_split(&id, &duplicate),
        Err(Ok(EscrowError::InvalidSplitConfig))
    );

    let six_payees = vec![
        &s.env,
        (Address::generate(&s.env), 1_666u32),
        (Address::generate(&s.env), 1_666u32),
        (Address::generate(&s.env), 1_666u32),
        (Address::generate(&s.env), 1_666u32),
        (Address::generate(&s.env), 1_666u32),
        (Address::generate(&s.env), 1_670u32),
    ];
    assert_eq!(
        s.escrow.try_set_team_split(&id, &six_payees),
        Err(Ok(EscrowError::InvalidSplitConfig))
    );

    let includes_client = vec![
        &s.env,
        (s.freelancer.clone(), 5_000u32),
        (s.client.clone(), 5_000u32),
    ];
    assert_eq!(
        s.escrow.try_set_team_split(&id, &includes_client),
        Err(Ok(EscrowError::InvalidSplitConfig))
    );

    let valid = vec![&s.env, (s.freelancer.clone(), 10_000u32)];
    s.escrow.set_team_split(&id, &valid);
    // A partial deposit keeps the status at Created, but splits are frozen once
    // any funds are in escrow.
    s.escrow.deposit_funds(&id, &40);
    assert_eq!(s.escrow.get_contract(&id).status, ContractStatus::Created);
    assert_eq!(
        s.escrow.try_set_team_split(&id, &valid),
        Err(Ok(EscrowError::InvalidStatus))
    );
}

#[test]
fn dispute_freelancer_share_respects_team_split() {
    let s = setup();
    let teammate_a = Address::generate(&s.env);
    let teammate_b = Address::generate(&s.env);
    let id = s.escrow.create_contract(
        &s.client,
        &s.freelancer,
        &s.token.address,
        &vec![&s.env, 406i128],
    );
    let payees = vec![
        &s.env,
        (s.freelancer.clone(), 5_000u32),
        (teammate_a.clone(), 3_000u32),
        (teammate_b.clone(), 2_000u32),
    ];
    s.escrow.set_team_split(&id, &payees);
    s.escrow.deposit_funds(&id, &406);
    s.escrow.open_dispute(&id, &s.freelancer);
    s.escrow.resolve_dispute(&id, &2_500);

    // 25% of 406 is 101; the split floors to 50/30/20 and the lead gets the 1-token remainder.
    assert_eq!(s.token.balance(&s.freelancer), 51);
    assert_eq!(s.token.balance(&teammate_a), 30);
    assert_eq!(s.token.balance(&teammate_b), 20);
    assert_eq!(s.token.balance(&s.client), 899);
    assert_eq!(s.escrow.get_contract(&id).status, ContractStatus::Resolved);
}

#[test]
fn pause_blocks_mutations() {
    let s = setup();
    s.escrow.set_paused(&true);
    assert_eq!(
        s.escrow.try_create_contract(
            &s.client,
            &s.freelancer,
            &s.token.address,
            &vec![&s.env, 1i128]
        ),
        Err(Ok(EscrowError::Paused))
    );
}
