use crate::{FeeDistributorContract, FeeDistributorContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn deploy(env: &Env) -> (FeeDistributorContractClient<'_>, Address) {
    let admin = Address::generate(env);
    env.mock_all_auths();
    let treasury = Address::generate(env);
    let lp = Address::generate(env);
    let token = Address::generate(env);
    let caller = Address::generate(env);
    let id = env.register(
        FeeDistributorContract,
        (&admin, &treasury, &lp, 5_000i128, &token, &caller),
    );
    env.set_auths(&[]);
    (FeeDistributorContractClient::new(env, &id), admin)
}

#[test]
fn test_get_admin_returns_deployment_admin() {
    let env = Env::default();
    let (client, admin) = deploy(&env);

    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_get_admin_returns_rotated_admin() {
    let env = Env::default();
    let (client, admin) = deploy(&env);
    let new_admin = Address::generate(&env);

    client.mock_all_auths().transfer_admin(&admin, &new_admin);
    assert_eq!(client.get_admin(), new_admin);
}
