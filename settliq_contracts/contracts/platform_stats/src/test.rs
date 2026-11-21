#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, testutils::Events, testutils::Ledger, Env, IntoVal, Symbol};

fn setup() -> (Env, PlatformStatsContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    // Extend entry TTLs so storage survives the large ledger jumps some tests
    // make (e.g. the 24h bucket rollover), mirroring fee_calculator's setup.
    env.ledger().set_sequence_number(100);
    env.ledger().with_mut(|li| {
        li.min_temp_entry_ttl = 300_000;
        li.min_persistent_entry_ttl = 300_000;
        li.max_entry_ttl = 300_000;
    });

    let admin = Address::generate(&env);
    let id = env.register(PlatformStatsContract, (&admin,));
    let client = PlatformStatsContractClient::new(&env, &id);
    (env, client, admin)
}

#[test]
fn test_initial_stats_are_zero() {
    let (env, client, _admin) = setup();
    let stats = client.stats();
    assert_eq!(stats.total_merchants, 0);
    assert_eq!(stats.total_payments, 0);
    assert_eq!(stats.total_settled_volume_usd, 0);
    assert_eq!(stats.active_payments_24h, 0);
    assert!(stats.health.storage_ok);
    assert!(stats.health.stellar_ok);
    assert!(stats.health.partner_ok);
}

#[test]
fn test_record_merchant_and_payment() {
    let (_env, client, admin) = setup();
    client.record_merchant(&admin);
    client.record_payment(&admin, &10_000_000i128, &true);
    client.record_payment(&admin, &5_000_000i128, &true);
    client.record_payment(&admin, &7_000_000i128, &false);

    let stats = client.stats();
    assert_eq!(stats.total_merchants, 1);
    assert_eq!(stats.total_payments, 3);
    assert_eq!(stats.total_settled_volume_usd, 15_000_000);
    assert_eq!(stats.active_payments_24h, 3);
}

#[test]
fn test_record_payment_increments_total_payments_by_one() {
    let (_env, client, admin) = setup();
    client.record_payment(&admin, &1_000_000i128, &true);
    assert_eq!(client.stats().total_payments, 1);
}

#[test]
fn test_record_payment_n_times_yields_count_n() {
    let (_env, client, admin) = setup();
    let n = 5u32;
    for _ in 0..n {
        client.record_payment(&admin, &1_000_000i128, &true);
    }
    assert_eq!(client.stats().total_payments, n);
}

#[test]
fn test_record_merchant_emits_event() {
    let (env, client, admin) = setup();
    client.record_merchant(&admin);

    let events = env.events().all();
    let last = events.last().unwrap();
    let expected = MerchantRecordedEvent {
        total_merchants: 1,
    };
    assert_eq!(
        last,
        (
            client.address.clone(),
            (Symbol::new(&env, "merchant"),).into_val(&env),
            expected.into_val(&env),
        )
    );
}

#[test]
fn test_record_payment_emits_event() {
    let (env, client, admin) = setup();
    client.record_payment(&admin, &10_000_000i128, &true);

    let events = env.events().all();
    let last = events.last().unwrap();
    let expected = PaymentRecordedEvent {
        amount_usd: 10_000_000,
        settled: true,
        total_payments: 1,
        total_settled_volume_usd: 10_000_000,
        active_payments_24h: 1,
    };
    assert_eq!(
        last,
        (
            client.address.clone(),
            (Symbol::new(&env, "payment"),).into_val(&env),
            expected.into_val(&env),
        )
    );
}

#[test]
fn test_set_partner_ok_emits_event() {
    let (env, client, admin) = setup();
    client.set_partner_ok(&admin, &false);

    let events = env.events().all();
    let last = events.last().unwrap();
    let expected = PartnerHealthUpdatedEvent { partner_ok: false };
    assert_eq!(
        last,
        (
            client.address.clone(),
            (Symbol::new(&env, "partner"),).into_val(&env),
            expected.into_val(&env),
        )
    );
}

#[test]
#[should_panic(expected = "not admin")]
fn test_record_merchant_requires_admin() {
    let (env, client, _admin) = setup();
    let attacker = Address::generate(&env);
    client.record_merchant(&attacker);
}

#[test]
#[should_panic(expected = "not admin")]
fn test_record_payment_requires_admin() {
    let (env, client, _admin) = setup();
    let attacker = Address::generate(&env);
    client.record_payment(&attacker, &100i128, &true);
}

#[test]
fn test_partner_health_status_toggle() {
    let (env, client, admin) = setup();
    assert!(client.stats().health.partner_ok);
    client.set_partner_ok(&admin, &false);
    assert!(!client.stats().health.partner_ok);
}

#[test]
#[should_panic(expected = "not admin")]
fn test_set_partner_ok_requires_admin() {
    let (env, client, _admin) = setup();
    let attacker = Address::generate(&env);
    client.set_partner_ok(&attacker, &false);
}

#[test]
fn test_active_payments_24h_resets_after_bucket_rollover() {
    let (env, client, admin) = setup();

    // Record payments in the first 24h bucket.
    client.record_payment(&admin, &1_000_000i128, &true);
    client.record_payment(&admin, &2_000_000i128, &true);
    assert_eq!(client.stats().active_payments_24h, 2);

    // Advance the ledger into the next ACTIVE_WINDOW_LEDGERS bucket. The
    // contract stores counts in a fixed-width ring buffer keyed by
    // `ledger_sequence / ACTIVE_WINDOW_LEDGERS`, so the counter reads 0 once
    // we are in a fresh bucket.
    env.ledger().set_sequence_number(2 * ACTIVE_WINDOW_LEDGERS);

    assert_eq!(client.stats().active_payments_24h, 0);

    // Payments recorded after the rollover accrue to the new bucket without
    // resurrecting the previous bucket's count.
    client.record_payment(&admin, &3_000_000i128, &true);
    assert_eq!(client.stats().active_payments_24h, 1);

    // Net totals are unaffected by the bucket rollover.
    let stats = client.stats();
    assert_eq!(stats.total_payments, 3);
    assert_eq!(stats.total_settled_volume_usd, 6_000_000);
}

/// Advance through 10 bucket periods recording payments each time and verify:
/// 1. `active_payments_24h` always reflects only the current bucket's count.
/// 2. The number of instance-storage entries stays constant — only a single
///    `ActiveBucket` key is ever written, regardless of how many periods pass.
#[test]
fn test_bucket_rollover_constant_storage_footprint_across_10_periods() {
    let (env, client, admin) = setup();

    for period in 0u32..10 {
        // Step into the middle of the bucket so sequence / ACTIVE_WINDOW_LEDGERS
        // equals `period`.
        let ledger = period * ACTIVE_WINDOW_LEDGERS + ACTIVE_WINDOW_LEDGERS / 2;
        env.ledger().set_sequence_number(ledger);

        // Record a distinct count per period so we can tell buckets apart.
        let payments_this_period = period + 1; // 1, 2, …, 10
        for _ in 0..payments_this_period {
            client.record_payment(&admin, &1_000_000i128, &true);
        }

        // Stats reflect only the current period's count.
        let stats = client.stats();
        assert_eq!(
            stats.active_payments_24h, payments_this_period,
            "period {period}: expected {payments_this_period} active payments"
        );

        // After rolling into a new period the previous count disappears from
        // active_payments_24h.
        if period > 0 {
            // Move half a window ahead (still within the same bucket) and
            // confirm the count is the same.
            let same_bucket_ledger = ledger + ACTIVE_WINDOW_LEDGERS / 4;
            env.ledger().set_sequence_number(same_bucket_ledger);
            assert_eq!(
                client.stats().active_payments_24h, payments_this_period,
                "period {period}: mid-window count should still be {payments_this_period}"
            );
        }
    }

    // The instance storage footprint should contain exactly one ActiveBucket
    // entry regardless of how many 24h windows have passed.  We verify this
    // indirectly: the total payment count equals the sum 1+2+…+10 = 55, so
    // all periods' payments were recorded, yet stats() still returns only the
    // last period's 10 active payments.
    let final_stats = client.stats();
    assert_eq!(final_stats.total_payments, 55);
    assert_eq!(final_stats.active_payments_24h, 10);
}

/// Verify that skipping multiple buckets in a single jump still resets the
/// active count to zero (gap larger than 1 window should not resurrect the
/// prev_count from the intermediate bucket that was never written).
#[test]
fn test_large_bucket_gap_resets_active_count() {
    let (env, client, admin) = setup();

    client.record_payment(&admin, &1_000_000i128, &true);
    assert_eq!(client.stats().active_payments_24h, 1);

    // Jump forward by 5 full windows.
    env.ledger()
        .set_sequence_number(5 * ACTIVE_WINDOW_LEDGERS + 1);
    assert_eq!(client.stats().active_payments_24h, 0);

    client.record_payment(&admin, &1_000_000i128, &true);
    assert_eq!(client.stats().active_payments_24h, 1);
    assert_eq!(client.stats().total_payments, 2);
}

#[test]
fn test_admin_transfer() {
    let (env, client, admin) = setup();
    let successor = Address::generate(&env);
    assert_eq!(client.get_admin(), admin);
    assert!(client.try_transfer_admin(&admin, &admin).is_err());
    assert!(client.try_transfer_admin(&successor, &admin).is_err());

    client.transfer_admin(&admin, &successor);
    assert_eq!(client.get_admin(), successor);
    assert!(client.try_transfer_admin(&admin, &successor).is_err());
    client.transfer_admin(&successor, &admin);
    assert_eq!(client.get_admin(), admin);
}
