use tide_ledger::v2::*;
use uuid::Uuid;

fn req(command: Command) -> Request {
    Request {
        id: Uuid::new_v4(),
        command,
    }
}
fn fund(ledger: &mut Ledger, account: Uuid, units: u64) {
    ledger
        .execute(req(Command::Fund {
            account,
            asset: Asset::Cyan,
            units,
        }))
        .unwrap();
}

#[test]
fn atomic_batches_asset_isolation_and_idempotency() {
    let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let mut ledger = Ledger::default();
    fund(&mut ledger, a, 100);
    let transfer = |asset, units, to| Transfer {
        from: a,
        to,
        asset,
        units,
    };
    let before = ledger.clone();
    let failed = req(Command::Batch {
        transfers: vec![transfer(Asset::Cyan, 20, b), transfer(Asset::Usdc, 1, c)],
    });
    assert_eq!(ledger.execute(failed), Err(Error::Insufficient));
    assert_eq!(ledger, before);
    let good = req(Command::Batch {
        transfers: vec![transfer(Asset::Cyan, 20, b), transfer(Asset::Cyan, 30, c)],
    });
    let batch = ledger.execute(good.clone()).unwrap();
    assert_eq!(ledger.execute(good.clone()).unwrap(), batch);
    assert_eq!(ledger.journal().len(), 2);
    assert_eq!(
        ledger.execute(Request {
            id: good.id,
            command: Command::Fund {
                account: a,
                asset: Asset::Cyan,
                units: 1
            }
        }),
        Err(Error::ReplayConflict)
    );
    assert_eq!(ledger.balance(Asset::Cyan, a), 50);
    assert_eq!(ledger.balance(Asset::Usdc, a), 0);
    ledger.audit().unwrap();
}

#[test]
fn escrow_burn_refund_and_channel_style_split_reconcile() {
    let (a, b, treasury) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let mut ledger = Ledger::default();
    fund(&mut ledger, a, 1000);
    let escrow = Uuid::new_v4();
    ledger
        .execute(req(Command::Lock {
            escrow,
            owner: a,
            asset: Asset::Cyan,
            units: 100,
        }))
        .unwrap();
    let before = ledger.clone();
    assert_eq!(
        ledger.execute(req(Command::Settle {
            escrow,
            payments: vec![Payment { to: b, units: 99 }],
            burn: 0
        })),
        Err(Error::Invalid)
    );
    assert_eq!(ledger, before);
    let settle = req(Command::Settle {
        escrow,
        payments: vec![
            Payment { to: b, units: 80 },
            Payment {
                to: treasury,
                units: 15,
            },
        ],
        burn: 5,
    });
    ledger.execute(settle.clone()).unwrap();
    ledger.execute(settle).unwrap();
    assert_eq!(ledger.balance(Asset::Cyan, BURN), 5);
    assert_eq!(ledger.balance(Asset::Cyan, CUSTODY), 0);
    assert_eq!(
        ledger.execute(req(Command::Refund { escrow })),
        Err(Error::EscrowState)
    );
    let second = Uuid::new_v4();
    ledger
        .execute(req(Command::Lock {
            escrow: second,
            owner: a,
            asset: Asset::Cyan,
            units: 100,
        }))
        .unwrap();
    // A channel close is one exact split: earned recipient share, fee, unspent refund.
    let payments = split(100, &[(b, 4900), (treasury, 100), (a, 5000)]).unwrap();
    ledger
        .execute(req(Command::Settle {
            escrow: second,
            payments,
            burn: 0,
        }))
        .unwrap();
    let third = Uuid::new_v4();
    ledger
        .execute(req(Command::Lock {
            escrow: third,
            owner: a,
            asset: Asset::Cyan,
            units: 1,
        }))
        .unwrap();
    ledger
        .execute(req(Command::Refund { escrow: third }))
        .unwrap();
    assert_eq!(ledger.escrow(third).unwrap().state, EscrowState::Refunded);
    ledger.audit().unwrap();
    assert_eq!(
        Ledger::from_journal(ledger.journal().to_vec()).unwrap(),
        ledger
    );
}

#[test]
fn edge_amounts_and_dust_do_not_create_units() {
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let mut ledger = Ledger::default();
    fund(&mut ledger, a, u64::MAX);
    let before = ledger.clone();
    assert_eq!(
        ledger.execute(req(Command::Fund {
            account: a,
            asset: Asset::Cyan,
            units: 1
        })),
        Err(Error::Overflow)
    );
    assert_eq!(ledger, before);
    for units in [1, 2, 3, 99, 100, u64::MAX] {
        let payouts = split(units, &[(a, 8000), (b, 2000)]).unwrap();
        assert_eq!(
            payouts.iter().map(|p| u128::from(p.units)).sum::<u128>(),
            u128::from(units)
        );
    }
    for id in [RESERVE, BURN, CUSTODY] {
        assert_eq!(
            ledger.execute(req(Command::Fund {
                account: id,
                asset: Asset::Cyan,
                units: 1
            })),
            Err(Error::Invalid)
        );
    }
    ledger.audit().unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn persistence_restarts_replay_and_refuses_stale_writers_or_legacy() {
    let path = std::env::temp_dir().join(format!("tide-v2-{}.sqlite", Uuid::new_v4()));
    let path = path.to_str().unwrap();
    let a = Uuid::new_v4();
    let mut ledger = Ledger::default();
    fund(&mut ledger, a, 100);
    ledger.persist(path).unwrap();
    let mut stale = Ledger::load(path).unwrap();
    let escrow = Uuid::new_v4();
    let lock = req(Command::Lock {
        escrow,
        owner: a,
        asset: Asset::Cyan,
        units: 25,
    });
    ledger.execute(lock.clone()).unwrap();
    ledger.persist(path).unwrap();
    let mut loaded = Ledger::load(path).unwrap();
    assert_eq!(loaded, ledger);
    loaded.execute(lock).unwrap();
    assert_eq!(loaded, ledger);
    fund(&mut stale, a, 1);
    assert_eq!(stale.persist(path), Err(Error::ReplayConflict));
    let mut bad = loaded.journal().to_vec();
    bad[0].entries[0].units += 1;
    assert!(Ledger::from_journal(bad).is_err());
    let mut duplicate = loaded.journal().to_vec();
    duplicate.push(duplicate[0].clone());
    assert!(Ledger::from_journal(duplicate).is_err());
    std::fs::remove_file(path).unwrap();
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch("CREATE TABLE accounts (id TEXT PRIMARY KEY, balance REAL);")
        .unwrap();
    drop(conn);
    assert_eq!(ledger.persist(path), Err(Error::LegacyDatabase));
    assert_eq!(Ledger::load(path), Err(Error::LegacyDatabase));
    std::fs::remove_file(path).unwrap();
}
