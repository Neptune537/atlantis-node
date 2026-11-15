//! Exhaustive finite transfer traces checked against an independent array model.
//! 2 accounts, 2 assets, amounts 0..=3, trace depth 3: 4096 terminal traces.
use tide_ledger::v2::*;
use uuid::Uuid;

fn visit(ledger: Ledger, model: [u64; 4], accounts: [Uuid; 2], depth: usize, traces: &mut usize) {
    if depth == 0 {
        *traces += 1;
        return;
    }
    for asset_index in 0..2 {
        let asset = [Asset::Cyan, Asset::Usdc][asset_index];
        for from in 0..2 {
            let to = 1 - from;
            for units in 0..=3 {
                let mut expected = model;
                let mut next = ledger.clone();
                let debit = asset_index * 2 + from;
                let credit = asset_index * 2 + to;
                let valid = units > 0 && model[debit] >= units;
                if valid {
                    expected[debit] -= units;
                    expected[credit] += units;
                }
                let result = next.execute(Request {
                    id: Uuid::new_v4(),
                    command: Command::Batch {
                        transfers: vec![Transfer {
                            from: accounts[from],
                            to: accounts[to],
                            asset,
                            units,
                        }],
                    },
                });
                assert_eq!(result.is_ok(), valid);
                if !valid {
                    assert_eq!(next, ledger);
                }
                for (ai, asset) in [Asset::Cyan, Asset::Usdc].into_iter().enumerate() {
                    for (index, account) in accounts.into_iter().enumerate() {
                        assert_eq!(
                            next.balance(asset, account),
                            i128::from(expected[ai * 2 + index])
                        );
                    }
                }
                next.audit().unwrap();
                visit(next, expected, accounts, depth - 1, traces);
            }
        }
    }
}

#[test]
fn exhaustive_transfer_model_conformance() {
    let accounts = [Uuid::new_v4(), Uuid::new_v4()];
    let mut ledger = Ledger::default();
    for asset in [Asset::Cyan, Asset::Usdc] {
        ledger
            .execute(Request {
                id: Uuid::new_v4(),
                command: Command::Fund {
                    account: accounts[0],
                    asset,
                    units: 2,
                },
            })
            .unwrap();
    }
    let mut traces = 0;
    visit(ledger, [2, 0, 2, 0], accounts, 3, &mut traces);
    assert_eq!(traces, 4096);
}
